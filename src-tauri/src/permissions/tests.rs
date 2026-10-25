use super::*;

fn signed_in(role: Role) -> Gate {
    Gate::Users(Some(SignedIn { role, must_change_password: false }))
}

#[test]
fn commands_are_classified() {
    assert_eq!(need("sign_in"), Need::Open);
    assert_eq!(need("list_invoices"), Need::Read);
    assert_eq!(need("get_profit_and_loss"), Need::Read);
    assert_eq!(need("print_report"), Need::Read);
    assert_eq!(need("create_invoice"), Need::Write);
    assert_eq!(need("void_transaction"), Need::Write);
    assert_eq!(need("set_preference"), Need::Admin);
    assert_eq!(need("get_mcp_status"), Need::Admin, "named get_ but shows the access token");
    assert_eq!(need("a_command_nobody_classified"), Need::Write);
}

#[test]
fn every_role_gets_what_it_should() {
    for gate in [Gate::NoCompany, Gate::NoUsers] {
        assert!(check(&gate, Need::Admin).is_ok(), "{gate:?}");
    }
    assert!(check(&Gate::Unchecked, Need::Open).is_ok());
    assert!(check(&Gate::Unchecked, Need::Read).is_err());
    assert!(check(&Gate::Users(None), Need::Read).unwrap_err().starts_with("SIGN_IN_REQUIRED"));
    assert!(check(&Gate::Users(None), Need::Open).is_ok());

    let temporary = Gate::Users(Some(SignedIn { role: Role::Admin, must_change_password: true }));
    assert!(check(&temporary, Need::Read).unwrap_err().starts_with("PASSWORD_CHANGE_REQUIRED"));
    assert!(check(&temporary, Need::Open).is_ok(), "change_my_password is open");

    assert!(check(&signed_in(Role::ViewOnly), Need::Read).is_ok());
    assert!(check(&signed_in(Role::ViewOnly), Need::Write).unwrap_err().starts_with("ROLE_VIEW_ONLY"));
    assert!(check(&signed_in(Role::Standard), Need::Write).is_ok());
    assert!(check(&signed_in(Role::Standard), Need::Admin).unwrap_err().starts_with("ROLE_ADMIN_ONLY"));
    assert!(check(&signed_in(Role::Admin), Need::Admin).is_ok());
}

#[tokio::test]
async fn the_gate_follows_the_open_file_and_its_sign_in() {
    let dir = std::env::temp_dir().join(format!("localbooks-gate-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S%f")));
    let db = crate::db::Db::open_at(dir.join("books.localbooks"), crate::db::test_config(&dir)).await.unwrap();
    db.refresh_gate().await;
    assert!(db.check(Need::Admin, true).is_ok(), "no users: everything runs");

    let (pool, slot) = db.pool_and_session().await.unwrap();
    let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let admin = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    assert!(db.check(Need::Admin, true).is_ok());

    let viewer = crate::users::add(&pool, slot.lock().await.as_ref().unwrap(), crate::users::NewUserInput {
        username: "viewer".into(),
        display_name: None,
        role: Role::ViewOnly,
        can_see_sensitive: false,
        password: "temporary pw".into(),
    })
    .await
    .unwrap();
    *slot.lock().await = None;
    db.refresh_gate().await;
    assert!(db.check(Need::Read, true).unwrap_err().starts_with("SIGN_IN_REQUIRED"));
    assert!(db.readable_pool().await.acquire().await.is_ok());
    let hidden = sqlx::query("SELECT 1 FROM vendors").execute(&db.readable_pool().await).await;
    assert!(hidden.is_err(), "the MCP server sees no books while signed out");

    let mut session = crate::users::sign_in(&pool, "viewer", "temporary pw").await.unwrap();
    assert_eq!(session.user.id, viewer.id);
    crate::users::change_own_password(&pool, &mut session, "temporary pw", "viewer's own pw").await.unwrap();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    assert!(db.check(Need::Read, true).is_ok());
    assert!(db.check(Need::Write, true).unwrap_err().starts_with("ROLE_VIEW_ONLY"));

    // Idle past the company's limit (15 minutes unless an Admin changes it), counting time asleep.
    db.backdate_activity(std::time::Duration::from_secs(14 * 60));
    assert!(db.check(Need::Read, false).is_ok());
    assert!(!db.idle_expired());
    db.backdate_activity(std::time::Duration::from_secs(14 * 60));
    assert!(db.check(Need::Read, true).is_ok(), "activity resets the clock");
    db.backdate_activity(std::time::Duration::from_secs(16 * 60));
    assert!(db.idle_expired());
    assert_eq!(db.check(Need::Read, false).unwrap_err(), IDLE_SIGNED_OUT);
    assert_eq!(db.check(Need::Read, true).unwrap_err(), IDLE_SIGNED_OUT, "too late to count as activity");
    assert!(db.check(Need::Open, false).is_ok(), "signing out and in still work");
    crate::preferences::set(&pool, "auto_lock_minutes", "0").await.unwrap();
    db.refresh_gate().await;
    db.backdate_activity(std::time::Duration::from_secs(24 * 3600));
    assert!(db.check(Need::Read, false).is_ok() && !db.idle_expired(), "0 never locks");
    assert!(crate::preferences::set(&pool, "auto_lock_minutes", "300").await.unwrap_err().starts_with("PREF_BAD_AUTO_LOCK"));

    // A remembered sign-in is tried once per opening, and never after a sign-out.
    assert!(db.take_remembered_attempt().await);
    assert!(!db.take_remembered_attempt().await);

    // The audit trail names whoever is signed in.
    *slot.lock().await = Some(crate::users::sign_in(&pool, "Owner", "owner password").await.unwrap());
    db.refresh_gate().await;
    let cash: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let sales: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(&pool).await.unwrap();
    crate::ledger::post_journal_entry(&pool, crate::ledger::JournalEntry {
        id: None,
        date: chrono::NaiveDate::from_ymd_opt(2026, 10, 24).unwrap(),
        description: "Sale".into(),
        lines: vec![crate::ledger::JournalLine::debit(cash, 500), crate::ledger::JournalLine::credit(sales, 500)],
        currency: None,
    })
    .await
    .unwrap();
    let actor: String = sqlx::query_scalar("SELECT actor FROM ledger_audit_log WHERE action_type = 'POST' ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    assert!(actor.starts_with(&format!("{} as ", admin.username)), "{actor}");

    db.close().await;
    assert!(db.check(Need::Admin, true).is_ok(), "no company open: commands answer for themselves");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every command the app registers is classified on purpose: the lists name only real commands.
#[test]
fn the_lists_name_registered_commands() {
    let main = include_str!("../main.rs");
    for name in OPEN.iter().chain(ADMIN).chain(READ) {
        assert!(main.contains(&format!("commands::{name},")), "{name} is not a registered command");
    }
}
