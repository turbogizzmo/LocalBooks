//! Closing date: locks the books through a date so closed periods cannot change.
//!
//! The lock is enforced by [`check_open`] inside [`crate::ledger::post_entry_in_tx`] (every posting
//! path, including voids, whose reversals carry the original date) and by the
//! `trg_je_closed_period` trigger as a backstop. Changes are appended to `closing_locks`, whose
//! latest row is in force; moving or removing the closing date requires the current passcode.

use chrono::NaiveDate;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::db_err;

pub const CLOSED_MESSAGE: &str = "PERIOD_CLOSED: Cannot write transaction: Target period is legally closed.";
const MIN_PASSCODE_CHARS: usize = 4;
/// PBKDF2 work factor (OWASP's 2023 recommendation for HMAC-SHA256). Stored with each hash, so it
/// can be raised later without invalidating existing passcodes.
const ITERATIONS: u32 = if cfg!(test) { 1_000 } else { 600_000 };

// ------------------------------------------------------------------------------------------------
// Passcode hashing: PBKDF2-HMAC-SHA256 (RFC 8018) with a per-passcode salt
// ------------------------------------------------------------------------------------------------

fn pbkdf2_sha256(passcode: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let prf = Hmac::<Sha256>::new_from_slice(passcode).expect("HMAC accepts any key length");
    let mut u = prf.clone();
    u.update(salt);
    u.update(&1u32.to_be_bytes()); // one 32-byte block is the whole derived key
    let mut block: [u8; 32] = u.finalize().into_bytes().into();
    let mut out = block;
    for _ in 1..iterations {
        let mut next = prf.clone();
        next.update(&block);
        block = next.finalize().into_bytes().into();
        out.iter_mut().zip(block.iter()).for_each(|(o, b)| *o ^= b);
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len()).step_by(2).map(|i| text.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect()
}

pub fn hash_passcode(passcode: &str) -> String {
    let salt = unhex(&crate::mcp_server::new_token()[..32]).expect("token is hex");
    format!("pbkdf2-sha256${ITERATIONS}${}${}", hex(&salt), hex(&pbkdf2_sha256(passcode.as_bytes(), &salt, ITERATIONS)))
}

pub fn verify_passcode(passcode: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("pbkdf2-sha256"), Some(iterations), Some(salt), Some(expected), None) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(iterations), Some(salt), Some(expected)) = (iterations.parse::<u32>(), unhex(salt), unhex(expected)) else {
        return false;
    };
    if iterations == 0 {
        return false;
    }
    let actual = pbkdf2_sha256(passcode.as_bytes(), &salt, iterations);
    // Constant-time comparison.
    expected.len() == actual.len() && expected.iter().zip(actual.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

// ------------------------------------------------------------------------------------------------
// Lock state
// ------------------------------------------------------------------------------------------------

async fn current(tx: &mut sqlx::SqliteConnection) -> Result<Option<(Option<NaiveDate>, Option<String>)>, String> {
    sqlx::query_as("SELECT lock_date, hashed_passcode FROM closing_locks ORDER BY id DESC LIMIT 1")
        .fetch_optional(tx)
        .await
        .map_err(db_err)
}

/// Refuses an entry dated on or before the closing date in force.
pub async fn check_open(tx: &mut Transaction<'_, Sqlite>, date: NaiveDate) -> Result<(), String> {
    match current(&mut **tx).await? {
        Some((Some(lock_date), _)) if date <= lock_date => Err(CLOSED_MESSAGE.to_string()),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ClosingChange {
    pub id: i64,
    pub lock_date: Option<NaiveDate>,
    pub set_at: String,
    pub passcode_protected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosingStatus {
    /// Entries dated on or before this cannot be posted or voided.
    pub lock_date: Option<NaiveDate>,
    pub passcode_protected: bool,
    pub history: Vec<ClosingChange>,
}

pub async fn status(pool: &SqlitePool) -> Result<ClosingStatus, String> {
    let history: Vec<ClosingChange> = sqlx::query_as(
        "SELECT id, lock_date, set_at, hashed_passcode IS NOT NULL AS passcode_protected FROM closing_locks ORDER BY id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let (lock_date, passcode_protected) = history.first().map(|c| (c.lock_date, c.passcode_protected)).unwrap_or((None, false));
    Ok(ClosingStatus { lock_date, passcode_protected, history })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosingInput {
    /// The new closing date; `None` reopens every period.
    pub lock_date: Option<NaiveDate>,
    /// Passcode of the lock in force (required when it has one).
    #[serde(default)]
    pub current_passcode: Option<String>,
    /// `None` keeps the current passcode, `""` removes it, anything else replaces it.
    #[serde(default)]
    pub new_passcode: Option<String>,
}

pub async fn set_closing_date(pool: &SqlitePool, input: &ClosingInput) -> Result<ClosingStatus, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let (current_date, current_hash) = current(&mut tx).await?.unwrap_or((None, None));
    if let Some(hash) = &current_hash {
        let given = input.current_passcode.as_deref().unwrap_or_default();
        if !verify_passcode(given, hash) {
            return Err("CLOSE_PASSCODE: the closing date passcode is incorrect".into());
        }
    }
    let next_hash = match input.new_passcode.as_deref() {
        None => current_hash.clone(),
        Some("") => None,
        Some(p) if p.chars().count() < MIN_PASSCODE_CHARS => {
            return Err(format!("CLOSE_PASSCODE_SHORT: use at least {MIN_PASSCODE_CHARS} characters"));
        }
        Some(p) => Some(hash_passcode(p)),
    };
    if input.lock_date == current_date && input.new_passcode.is_none() {
        return Err("CLOSE_UNCHANGED: that is already the closing date".into());
    }
    if input.lock_date.is_none() && next_hash.is_some() && input.new_passcode.is_some() {
        return Err("CLOSE_PASSCODE_WITHOUT_DATE: set a closing date to protect it with a passcode".into());
    }
    let next_hash = if input.lock_date.is_none() { None } else { next_hash };
    sqlx::query("INSERT INTO closing_locks (lock_date, hashed_passcode) VALUES (?1, ?2)")
        .bind(input.lock_date.map(|d| d.to_string()))
        .bind(next_hash)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    status(pool).await
}

#[cfg(test)]
mod tests {
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
}
