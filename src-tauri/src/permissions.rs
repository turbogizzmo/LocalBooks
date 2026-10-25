//! What each signed-in role may do, checked in one place before any command runs (see `main.rs`).
//!
//! A company file without users lets everything through, as before users existed. With users on:
//! - [`Need::Open`] commands (signing in, opening or closing a company file, windows) always run;
//! - everything else needs someone signed in, who has replaced any temporary password;
//! - [`Need::Read`] commands run for every role;
//! - [`Need::Write`] commands (anything that changes the books) need Admin or Standard;
//! - [`Need::Admin`] commands (users, company-wide settings, imports and restores, bank connections,
//!   the AI access server) need an Admin.
//!
//! An unlisted command counts as a write, so a new command is closed to View-only users until it is
//! classified here. The rules are LocalBooks' own: the company file is still a SQLite file that
//! another program can read and change; only encrypted values ([`crate::crypto`]) stay protected,
//! and the ledger audit trail shows changes made outside LocalBooks.

use crate::users::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Open,
    Read,
    Write,
    Admin,
}

const OPEN: &[&str] = &[
    "get_startup_failure",
    "get_storage_status",
    "get_sign_in_state",
    "turn_on_users",
    "sign_in",
    "sign_out",
    "change_my_password",
    "recover_with_recovery_key",
    "remove_all_users",
    "get_start_screen",
    "open_company_file",
    "close_company_file",
    "create_company_file",
    "forget_recent_company",
    "find_company_files",
    "restore_backup_as",
    "open_window",
];

const ADMIN: &[&str] = &[
    // Users and the recovery key.
    "list_users",
    "add_user",
    "update_user",
    "replace_recovery_key",
    // Company-wide settings.
    "set_preference",
    "set_company_logo",
    "clear_company_logo",
    "set_closing_date",
    "create_form_style",
    "rename_form_style",
    "set_default_form_style",
    "delete_form_style",
    "save_form_template",
    "reset_form_template",
    "get_smtp_settings",
    "save_smtp_settings",
    "test_smtp_settings",
    "send_smtp_test_email",
    "get_mcp_status",
    "set_mcp_enabled",
    "regenerate_mcp_token",
    // Where the file lives, whole-file copies, restores and repairs.
    "inspect_database_directory",
    "update_database_directory",
    "export_company_file",
    "import_company_file",
    "file_dentist_rebuild",
    "apply_forensic_correction",
    // Bank connections (their access tokens).
    "simplefin_connect",
    "simplefin_reconnect",
    "simplefin_refresh_accounts",
    "simplefin_link",
    "simplefin_unlink",
    "simplefin_disconnect",
    // Importing other programs' books.
    "ingest_raw_qbw",
    "set_qbw_mapping",
    "auto_create_qbw_accounts",
    "post_qbw_batch",
    "discard_qbw_batch",
    "reverse_qbw_batch",
    "set_ledger_account",
    "auto_map_ledger_accounts",
    "import_ledger_lists",
    "post_ledger_batch",
    "reverse_ledger_batch",
    "discard_ledger_batch",
    "start_over_ledger_batch",
];

/// Commands that only read, besides those named `get_…`, `list_…`, `inspect_…` and `preview_…`.
const READ: &[&str] = &[
    "search_books",
    "verify_ledger_integrity",
    "file_dentist_checkup",
    "propose_forensic_corrections",
    "simplefin_suggested_start",
    "report_csv",
    "save_report_to_disk",
    "print_report",
    "render_form",
    "save_form_pdf",
    "save_attachment",
    "backup_now",
];

pub fn need(command: &str) -> Need {
    if OPEN.contains(&command) {
        Need::Open
    } else if ADMIN.contains(&command) {
        Need::Admin
    } else if READ.contains(&command) || ["get_", "list_", "inspect_", "preview_"].iter().any(|p| command.starts_with(p)) {
        Need::Read
    } else {
        Need::Write
    }
}

/// Who may do what in the open company file, kept current by [`crate::db::Db::refresh_gate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Not worked out yet: only [`Need::Open`] commands run.
    Unchecked,
    /// No company file open: commands report that themselves.
    NoCompany,
    /// A company file without users.
    NoUsers,
    /// Users are on; who is signed in, if anyone.
    Users(Option<SignedIn>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub role: Role,
    pub must_change_password: bool,
}

pub fn check(gate: &Gate, need: Need) -> Result<(), String> {
    if need == Need::Open {
        return Ok(());
    }
    let user = match gate {
        Gate::NoCompany | Gate::NoUsers => return Ok(()),
        Gate::Unchecked => return Err("SIGN_IN_REQUIRED: the company file is still opening; try again".into()),
        Gate::Users(None) => return Err(crate::users::SIGN_IN_REQUIRED.into()),
        Gate::Users(Some(user)) => user,
    };
    if user.must_change_password {
        return Err("PASSWORD_CHANGE_REQUIRED: choose your own password first".into());
    }
    match (need, user.role) {
        (Need::Write, Role::ViewOnly) => Err("ROLE_VIEW_ONLY: your user can look at the books but not change them".into()),
        (Need::Admin, Role::Standard | Role::ViewOnly) => Err("ROLE_ADMIN_ONLY: only an Admin can do this".into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
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
        assert!(db.check(Need::Admin).is_ok(), "no users: everything runs");

        let (pool, slot) = db.pool_and_session().await.unwrap();
        let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
        let admin = session.user.clone();
        *slot.lock().await = Some(session);
        db.refresh_gate().await;
        assert!(db.check(Need::Admin).is_ok());

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
        assert!(db.check(Need::Read).unwrap_err().starts_with("SIGN_IN_REQUIRED"));
        assert!(db.readable_pool().await.acquire().await.is_ok());
        let hidden = sqlx::query("SELECT 1 FROM vendors").execute(&db.readable_pool().await).await;
        assert!(hidden.is_err(), "the MCP server sees no books while signed out");

        let mut session = crate::users::sign_in(&pool, "viewer", "temporary pw").await.unwrap();
        assert_eq!(session.user.id, viewer.id);
        crate::users::change_own_password(&pool, &mut session, "temporary pw", "viewer's own pw").await.unwrap();
        *slot.lock().await = Some(session);
        db.refresh_gate().await;
        assert!(db.check(Need::Read).is_ok());
        assert!(db.check(Need::Write).unwrap_err().starts_with("ROLE_VIEW_ONLY"));

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
        assert!(db.check(Need::Admin).is_ok(), "no company open: commands answer for themselves");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every command the app registers is classified on purpose: the lists name only real commands.
    #[test]
    fn the_lists_name_registered_commands() {
        let main = include_str!("main.rs");
        for name in OPEN.iter().chain(ADMIN).chain(READ) {
            assert!(main.contains(&format!("commands::{name},")), "{name} is not a registered command");
        }
    }
}
