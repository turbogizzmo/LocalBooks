//! Test-only harnesses that guard the books themselves, on top of each module's example tests:
//!
//! - `ledger_props`: property tests that drive random sequences of postings, payments, voids,
//!   edits and closing-date changes through the real public functions and check the ledger's
//!   invariants after every step.
//! - `golden`: a deterministic fixture company (`fixture`) whose reports are snapshotted as text,
//!   so any change to a reported number shows up as a reviewable diff (`cargo insta review`).

mod fixture;
mod golden;
mod ledger_props;

use sqlx::SqlitePool;

/// Id of a seeded account by its number.
pub(crate) async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("account {number} exists: {e}"))
}

/// Id of the first system account of a type (what the posting code itself picks).
pub(crate) async fn system_account(pool: &SqlitePool, account_type: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_system = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("system {account_type} account exists: {e}"))
}

/// A single-threaded runtime for driving the async ledger from a synchronous property test.
pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime")
}
