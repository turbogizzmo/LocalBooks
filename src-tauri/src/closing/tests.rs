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
