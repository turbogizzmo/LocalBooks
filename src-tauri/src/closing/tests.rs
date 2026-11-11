use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, void_transaction, JournalEntry, JournalLine};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn post(pool: &SqlitePool, date: &str) -> Result<i64, String> {
    let (checking, sales) = (account(pool, "1010").await, account(pool, "4000").await);
    post_journal_entry(
        pool,
        JournalEntry {
            id: None,
            date: date.parse().unwrap(),
            description: String::new(),
            lines: vec![JournalLine::debit(checking, 100), JournalLine::credit(sales, 100)],
            currency: None,
        },
    )
    .await
}

fn lock(date: Option<&str>, current: Option<&str>, new: Option<&str>) -> ClosingInput {
    ClosingInput { lock_date: date.map(|d| d.parse().unwrap()), current_passcode: current.map(String::from), new_passcode: new.map(String::from) }
}

#[test]
fn passcodes_hash_with_pbkdf2_and_verify_in_constant_time() {
    // RFC 7914 §11 PBKDF2-HMAC-SHA256 test vector (first 32 bytes, 1 iteration).
    assert_eq!(
        hex(&pbkdf2_sha256(b"passwd", b"salt", 1)),
        "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
    );
    let stored = hash_passcode("year-end 2026");
    assert!(stored.starts_with("pbkdf2-sha256$"));
    assert_ne!(stored, hash_passcode("year-end 2026"), "each hash has its own salt");
    assert!(verify_passcode("year-end 2026", &stored));
    assert!(!verify_passcode("year-end 2025", &stored));
    assert!(!verify_passcode("anything", "pbkdf2-sha256$0$00$00"));
    assert!(!verify_passcode("anything", "plaintext"));
}

#[tokio::test]
async fn closed_periods_refuse_posts_and_voids_until_reopened_with_the_passcode() {
    let pool = test_pool().await;
    let early = post(&pool, "2026-03-31").await.unwrap();
    post(&pool, "2026-04-01").await.unwrap();

    let status = set_closing_date(&pool, &lock(Some("2026-03-31"), None, Some("q1-close"))).await.unwrap();
    assert_eq!(status.lock_date, NaiveDate::from_ymd_opt(2026, 3, 31));
    assert!(status.passcode_protected);

    assert_eq!(post(&pool, "2026-03-31").await.unwrap_err(), CLOSED_MESSAGE, "the closing date itself is closed");
    assert_eq!(post(&pool, "2025-12-01").await.unwrap_err(), CLOSED_MESSAGE);
    post(&pool, "2026-04-02").await.expect("the open period still posts");
    assert_eq!(void_transaction(&pool, early).await.unwrap_err(), CLOSED_MESSAGE, "a void would change the closed period");

    // The database refuses too, even if application code is bypassed.
    let draft: i64 = sqlx::query_scalar("INSERT INTO journal_entries (entry_number, entry_date) VALUES (99, '2026-01-15') RETURNING id").fetch_one(&pool).await.unwrap();
    let (checking, sales) = (account(&pool, "1010").await, account(&pool, "4000").await);
    sqlx::query("INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents) VALUES (?1, 1, ?2, 5)").bind(draft).bind(checking).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO journal_lines (journal_entry_id, line_number, account_id, credit_cents) VALUES (?1, 2, ?2, 5)").bind(draft).bind(sales).execute(&pool).await.unwrap();
    let err = sqlx::query("UPDATE journal_entries SET status = 'POSTED', posted_at = 'now' WHERE id = ?1").bind(draft).execute(&pool).await.map_err(db_err).unwrap_err();
    assert_eq!(err, CLOSED_MESSAGE);

    // Moving the date needs the passcode.
    let err = set_closing_date(&pool, &lock(Some("2026-02-28"), None, None)).await.unwrap_err();
    assert!(err.starts_with("CLOSE_PASSCODE"), "{err}");
    let err = set_closing_date(&pool, &lock(Some("2026-02-28"), Some("wrong"), None)).await.unwrap_err();
    assert!(err.starts_with("CLOSE_PASSCODE"), "{err}");
    let status = set_closing_date(&pool, &lock(Some("2026-02-28"), Some("q1-close"), None)).await.unwrap();
    assert!(status.passcode_protected, "the passcode is kept when not changed");
    post(&pool, "2026-03-15").await.expect("March reopened");

    // Reopening everything clears the passcode; history keeps every change.
    let status = set_closing_date(&pool, &lock(None, Some("q1-close"), None)).await.unwrap();
    assert_eq!((status.lock_date, status.passcode_protected), (None, false));
    assert_eq!(status.history.len(), 3);
    void_transaction(&pool, early).await.expect("voids work once the period is open");
    assert!(sqlx::query("DELETE FROM closing_locks").execute(&pool).await.map_err(db_err).unwrap_err().starts_with("CLOSE_HISTORY_IMMUTABLE"));

    assert!(set_closing_date(&pool, &lock(Some("2026-01-31"), None, Some("abc"))).await.unwrap_err().starts_with("CLOSE_PASSCODE_SHORT"));
    assert!(set_closing_date(&pool, &lock(None, None, None)).await.unwrap_err().starts_with("CLOSE_UNCHANGED"));
}

#[tokio::test]
async fn an_external_accountant_posts_in_a_closed_period_and_the_report_says_who() {
    let pool = test_pool().await;
    let d = |t: &str| t.parse::<NaiveDate>().unwrap();
    let report = |pool: &SqlitePool| {
        let pool = pool.clone();
        async move { crate::list_reports::run(&pool, "closing-date-exceptions", Some(d("2026-01-01")), Some(d("2026-12-31")), d("2026-12-31")).await.unwrap() }
    };
    post(&pool, "2026-02-10").await.unwrap();
    set_closing_date(&pool, &lock(Some("2026-03-31"), None, None)).await.unwrap();
    assert_eq!(report(&pool).await.report.line_count, 0, "entries made before the closing date was set aren't exceptions");
    assert_eq!(post(&pool, "2026-03-15").await.unwrap_err(), CLOSED_MESSAGE);

    crate::permissions::set_signed_in_role(Some(crate::users::Role::ExternalAccountant));
    crate::audit_chain::set_signed_in(Some("cpa".into()));
    let adjusting = post(&pool, "2026-03-15").await.expect("an External Accountant posts in the closed period");
    crate::permissions::set_signed_in_role(None);
    crate::audit_chain::set_signed_in(None);
    post(&pool, "2026-04-02").await.unwrap();
    assert_eq!(post(&pool, "2026-03-15").await.unwrap_err(), CLOSED_MESSAGE, "closed again for everyone else");

    // The database backstop is back for everyone once that transaction committed.
    let flag: Option<i64> = sqlx::query_scalar("SELECT value FROM closing_exception_in_progress").fetch_one(&pool).await.unwrap();
    assert_eq!(flag, None, "commit_write clears the pass");
    let draft: i64 = sqlx::query_scalar("INSERT INTO journal_entries (entry_number, entry_date) VALUES (98, '2026-03-20') RETURNING id").fetch_one(&pool).await.unwrap();
    let refused = sqlx::query("UPDATE journal_entries SET status = 'POSTED' WHERE id = ?1").bind(draft).execute(&pool).await.unwrap_err().to_string();
    assert!(refused.contains("PERIOD_CLOSED"), "{refused}");

    // Closing Date Exceptions lists it, with who entered it.
    let found = report(&pool).await;
    let rows: Vec<_> = found.report.rows.iter().filter(|r| r.kind == crate::custom_reports::RowKind::Detail).collect();
    assert_eq!(rows.len(), 1, "{:?}", found.notes);
    assert_eq!(rows[0].journal_entry_id, Some(adjusting));
    let by = found.report.columns.iter().position(|c| c.label == "Entered By").unwrap();
    assert!(rows[0].cells[by].as_str().is_some_and(|who| who.starts_with("cpa as ")), "{:?}", rows[0].cells);
}

/// Closing Date Exceptions orders a close and an entry by the entry ids the close recorded, not by
/// clock time: on Windows the clock moves in ~15 ms steps, so the two often carry the same time.
#[tokio::test]
async fn an_entry_made_in_the_same_clock_step_as_a_close_is_ordered_by_entry_id() {
    let pool = test_pool().await;
    let date = |s: &str| s.parse::<chrono::NaiveDate>().unwrap();
    let exceptions = |pool: SqlitePool| async move {
        let report = crate::list_reports::run(&pool, "closing-date-exceptions", Some(date("2025-01-01")), Some(date("2025-12-31")), date("2026-01-15")).await.unwrap();
        report.report.rows.iter().filter_map(|r| r.journal_entry_id).collect::<Vec<i64>>()
    };
    let before = post(&pool, "2025-11-10").await.unwrap();
    set_closing_date(&pool, &lock(Some("2025-11-30"), None, None)).await.unwrap();
    set_closing_date(&pool, &lock(None, None, None)).await.unwrap();
    let late = post(&pool, "2025-11-20").await.unwrap();
    set_closing_date(&pool, &lock(Some("2025-11-30"), None, None)).await.unwrap();

    // The worst case for clock times: every change to the closing date at the very instant the
    // late entry was made, which the clock's ~15 ms step on Windows produces. Compared by time, the
    // late entry would not be after the first close. (The history is immutable in the app; the
    // test lifts the guard to set the times.)
    sqlx::query("DROP TRIGGER trg_closing_locks_no_update").execute(&pool).await.unwrap();
    sqlx::query("UPDATE closing_locks SET set_at = (SELECT created_at FROM journal_entries WHERE id = ?1)").bind(late).execute(&pool).await.unwrap();

    assert_eq!(exceptions(pool.clone()).await, vec![late], "posted after the first close, though at the same time; the entry made before it is not an exception");
    assert!(!exceptions(pool.clone()).await.contains(&before));
}
