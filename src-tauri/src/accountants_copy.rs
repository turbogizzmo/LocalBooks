//! Accountant's Copy (gaps row R14; design in `docs/plans/accountants-copy.md`).
//!
//! The client makes a copy of the books for the accountant with a **dividing date**. While it is
//! out, the client can't post on or before that date, and the accountant, in the copy, can't post
//! after it ([`posting_limit`], checked by `closing::check_open` with every posting). The accountant
//! exports the journal entries and accounts they added as a changes file; the client imports it
//! (only for the copy that is out), which posts the entries, creates the accounts, marks the copy
//! returned and keeps an audit record, in one write transaction.

use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, Cents, EntrySource, JournalEntry, JournalLine};

const FORMAT: &str = "localbooks-accountant-changes-1";
pub const COPY_LOCKED: &str = "ACCOUNTANT_COPY_LOCKED: an Accountant's Copy is out for this period; post after its dividing date, or import or cancel the copy first";
pub const COPY_LIMIT: &str = "ACCOUNTANT_COPY_LIMIT: this is an Accountant's Copy; post only on or before its dividing date";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ------------------------------------------------------------------------------------------------
// The dividing date
// ------------------------------------------------------------------------------------------------

/// Refuses a posting dated on the wrong side of a dividing date: on or before it in a client file
/// with a copy out, after it in the copy itself.
pub async fn check_posting(tx: &mut Transaction<'_, Sqlite>, date: NaiveDate) -> Result<(), String> {
    let out: Option<NaiveDate> = sqlx::query_scalar("SELECT dividing_date FROM accountants_copies WHERE status = 'OUTSTANDING'").fetch_optional(&mut **tx).await.map_err(db_err)?;
    if out.is_some_and(|d| date <= d) {
        return Err(COPY_LOCKED.into());
    }
    let copy: Option<NaiveDate> = sqlx::query_scalar("SELECT dividing_date FROM accountants_copy_info WHERE id = 1").fetch_optional(&mut **tx).await.map_err(db_err)?;
    if copy.is_some_and(|d| date > d) {
        return Err(COPY_LIMIT.into());
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// The client's side
// ------------------------------------------------------------------------------------------------

/// A copy made for the accountant, as the client's file records it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CopyRecord {
    pub id: i64,
    pub dividing_date: NaiveDate,
    pub file_name: String,
    pub status: String,
    pub created_at: String,
    pub created_by: String,
    pub closed_at: Option<String>,
    pub closed_by: Option<String>,
    pub entries_posted: i64,
    pub accounts_created: i64,
    pub changes_sha256: Option<String>,
}

const RECORD_SELECT: &str = "SELECT id, dividing_date, file_name, status, created_at, created_by, closed_at, closed_by, entries_posted, accounts_created, changes_sha256 FROM accountants_copies";

pub async fn copies(pool: &SqlitePool) -> Result<Vec<CopyRecord>, String> {
    sqlx::query_as(&format!("{RECORD_SELECT} ORDER BY id DESC")).fetch_all(pool).await.map_err(db_err)
}

/// What the open file is: the copy's dividing date when it is an Accountant's Copy.
pub async fn copy_dividing_date(pool: &SqlitePool) -> Result<Option<NaiveDate>, String> {
    sqlx::query_scalar("SELECT dividing_date FROM accountants_copy_info WHERE id = 1").fetch_optional(pool).await.map_err(db_err)
}

/// Writes an Accountant's Copy to `destination` and records it as out. Only one at a time, and not
/// from a copy.
pub async fn create(pool: &SqlitePool, live_path: &Path, destination: &Path, dividing_date: NaiveDate, by: &str) -> Result<CopyRecord, String> {
    if copy_dividing_date(pool).await?.is_some() {
        return Err("ACCOUNTANT_COPY_IN_COPY: this is itself an Accountant's Copy".into());
    }
    let out: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM accountants_copies WHERE status = 'OUTSTANDING')").fetch_one(pool).await.map_err(db_err)?;
    if out {
        return Err("ACCOUNTANT_COPY_OUT: a copy is already out; import or cancel it first".into());
    }
    let token = hex(&crate::crypto::random::<16>()?);
    crate::backup::snapshot(pool, live_path, destination).await?;
    // Mark the copy, opened on its own (it is closed again before anything else uses it).
    let marked = async {
        let copy = crate::db::open(destination).await.map_err(|e| format!("ACCOUNTANT_COPY_WRITE: {e}"))?;
        let result = sqlx::query(
            "INSERT INTO accountants_copy_info (id, token, dividing_date, base_entry_id, base_account_id)
             VALUES (1, ?1, ?2, (SELECT COALESCE(MAX(id), 0) FROM journal_entries), (SELECT COALESCE(MAX(id), 0) FROM chart_of_accounts))",
        )
        .bind(&token)
        .bind(dividing_date)
        .execute(&copy)
        .await
        .map_err(db_err);
        crate::db::close_pool(&copy).await;
        result
    }
    .await;
    if let Err(e) = marked {
        let _ = std::fs::remove_file(destination);
        return Err(e);
    }
    let file_name = destination.file_name().map_or_else(|| destination.display().to_string(), |n| n.to_string_lossy().into_owned());
    let id: i64 = sqlx::query_scalar("INSERT INTO accountants_copies (token, dividing_date, file_name, created_by) VALUES (?1, ?2, ?3, ?4) RETURNING id")
        .bind(&token)
        .bind(dividing_date)
        .bind(&file_name)
        .bind(by)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    record(pool, id).await
}

async fn record(pool: &SqlitePool, id: i64) -> Result<CopyRecord, String> {
    sqlx::query_as(&format!("{RECORD_SELECT} WHERE id = ?1")).bind(id).fetch_one(pool).await.map_err(db_err)
}

/// Lifts the client's lock without importing anything; the copy's changes are then refused.
pub async fn cancel(pool: &SqlitePool, by: &str) -> Result<(), String> {
    let done = sqlx::query("UPDATE accountants_copies SET status = 'CANCELLED', closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), closed_by = ?1 WHERE status = 'OUTSTANDING'")
        .bind(by)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("ACCOUNTANT_COPY_NONE: no Accountant's Copy is out".into());
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// The changes file
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedAccount {
    pub number: Option<String>,
    pub name: String,
    pub full_name: String,
    pub parent_full_name: Option<String>,
    pub account_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedLine {
    pub account_number: Option<String>,
    pub account_full_name: String,
    pub debit_cents: i64,
    pub credit_cents: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedEntry {
    pub date: NaiveDate,
    pub memo: String,
    pub adjusting: bool,
    pub lines: Vec<ChangedLine>,
}

/// What the accountant sends back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesFile {
    pub format: String,
    pub token: String,
    pub dividing_date: NaiveDate,
    pub accounts: Vec<ChangedAccount>,
    pub entries: Vec<ChangedEntry>,
    /// What the accountant did in the copy that the file can't carry, in words.
    pub left_out: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct CopyInfo {
    token: String,
    dividing_date: NaiveDate,
    base_entry_id: i64,
    base_account_id: i64,
}

/// In the copy: the journal entries and accounts added since it was made, as the changes file.
pub async fn changes(pool: &SqlitePool) -> Result<ChangesFile, String> {
    let info: CopyInfo = sqlx::query_as("SELECT token, dividing_date, base_entry_id, base_account_id FROM accountants_copy_info WHERE id = 1")
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "ACCOUNTANT_COPY_NOT_COPY: this isn't an Accountant's Copy".to_string())?;
    let accounts: Vec<ChangedAccount> = sqlx::query_as::<_, (Option<String>, String, String, Option<String>, String)>(
        "SELECT a.account_number, a.name, a.full_name, p.full_name, a.account_type FROM v_chart_of_accounts a LEFT JOIN v_chart_of_accounts p ON p.id = a.parent_id
         WHERE a.id > ?1 ORDER BY a.depth, a.id",
    )
    .bind(info.base_account_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    .into_iter()
    .map(|(number, name, full_name, parent_full_name, account_type)| ChangedAccount { number, name, full_name, parent_full_name, account_type })
    .collect();

    // Journal entries posted since, still standing: not voided, and not a reversal.
    // (id, date, memo, adjusting, source type, voided, the entry it reverses)
    type EntryRow = (i64, NaiveDate, String, bool, String, bool, Option<i64>);
    let rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT id, entry_date, COALESCE(memo, ''), is_adjusting = 1, source_type, is_voided = 1, reverses_entry_id FROM journal_entries
         WHERE id > ?1 AND status = 'POSTED' ORDER BY id",
    )
    .bind(info.base_entry_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut entries = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    for (id, date, memo, adjusting, source, voided, reverses) in rows {
        if source != "JOURNAL" {
            left_out.push(format!("{} dated {date} (only journal entries travel)", source.to_lowercase().replace('_', " ")));
            continue;
        }
        if voided {
            let what = if memo.is_empty() { "a journal entry".to_string() } else { format!("journal entry \"{memo}\"") };
            left_out.push(format!("{what} dated {date}, voided in the copy (it and its void stay behind)"));
            continue;
        }
        if let Some(original) = reverses {
            if original <= info.base_entry_id {
                left_out.push(format!("the void, dated {date}, of an entry the client made (only new entries travel)"));
            }
            continue;
        }
        let lines: Vec<ChangedLine> = sqlx::query_as::<_, (Option<String>, String, i64, i64, Option<String>)>(
            "SELECT a.account_number, a.full_name, l.debit_cents, l.credit_cents, l.memo FROM journal_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id
             WHERE l.journal_entry_id = ?1 ORDER BY l.line_number",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(account_number, account_full_name, debit_cents, credit_cents, memo)| ChangedLine { account_number, account_full_name, debit_cents, credit_cents, memo })
        .collect();
        entries.push(ChangedEntry { date, memo, adjusting, lines });
    }
    Ok(ChangesFile { format: FORMAT.into(), token: info.token, dividing_date: info.dividing_date, accounts, entries, left_out })
}

/// In the copy: writes the changes file to `destination`.
pub async fn export(pool: &SqlitePool, destination: &Path) -> Result<ChangesFile, String> {
    let file = changes(pool).await?;
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    crate::export_engine::write_atomically(destination, text.as_bytes())?;
    Ok(file)
}

// ------------------------------------------------------------------------------------------------
// Bringing the changes back
// ------------------------------------------------------------------------------------------------

/// What an import would do, for the preview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub dividing_date: NaiveDate,
    pub accounts: Vec<ChangedAccount>,
    /// Accounts the file adds that the books already have (by number or full name): reused.
    pub accounts_existing: Vec<String>,
    pub entries: Vec<ChangedEntry>,
    pub left_out: Vec<String>,
}

fn read(bytes: &[u8]) -> Result<ChangesFile, String> {
    let file: ChangesFile = serde_json::from_slice(bytes).map_err(|_| "ACCOUNTANT_CHANGES_BAD: this isn't an accountant's changes file".to_string())?;
    if file.format != FORMAT {
        return Err("ACCOUNTANT_CHANGES_BAD: this changes file is from another version of LocalBooks".into());
    }
    Ok(file)
}

async fn matching_copy(conn: &mut sqlx::SqliteConnection, file: &ChangesFile) -> Result<i64, String> {
    let row: Option<(i64, String, NaiveDate)> = sqlx::query_as("SELECT id, status, dividing_date FROM accountants_copies WHERE token = ?1").bind(&file.token).fetch_optional(&mut *conn).await.map_err(db_err)?;
    match row {
        None => Err("ACCOUNTANT_CHANGES_OTHER: these changes are from a copy of other books".into()),
        Some((_, status, _)) if status == "RETURNED" => Err("ACCOUNTANT_CHANGES_DONE: these changes were already imported".into()),
        Some((_, status, _)) if status == "CANCELLED" => Err("ACCOUNTANT_CHANGES_CANCELLED: that copy was cancelled; ask the accountant to work on a new one".into()),
        Some((_, _, d)) if d != file.dividing_date => Err("ACCOUNTANT_CHANGES_BAD: the file's dividing date doesn't match its copy".into()),
        Some((id, _, _)) => Ok(id),
    }
}

async fn account_id(conn: &mut sqlx::SqliteConnection, number: Option<&str>, full_name: &str) -> Result<Option<i64>, String> {
    if let Some(n) = number.filter(|n| !n.trim().is_empty()) {
        let by_number: Option<i64> = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_optional(&mut *conn).await.map_err(db_err)?;
        if by_number.is_some() {
            return Ok(by_number);
        }
    }
    sqlx::query_scalar("SELECT id FROM v_chart_of_accounts WHERE full_name = ?1 COLLATE NOCASE").bind(full_name).fetch_optional(&mut *conn).await.map_err(db_err)
}

/// Reads a changes file and shows what importing it would do, refusing a file that isn't for the
/// copy that is out.
pub async fn preview(pool: &SqlitePool, bytes: &[u8]) -> Result<ImportPreview, String> {
    let file = read(bytes)?;
    let mut conn = pool.acquire().await.map_err(db_err)?;
    matching_copy(&mut conn, &file).await?;
    let mut existing = Vec::new();
    for a in &file.accounts {
        if account_id(&mut conn, a.number.as_deref(), &a.full_name).await?.is_some() {
            existing.push(a.full_name.clone());
        }
    }
    Ok(ImportPreview { dividing_date: file.dividing_date, accounts: file.accounts, accounts_existing: existing, entries: file.entries, left_out: file.left_out })
}

/// Imports the accountant's changes: creates their accounts, posts their entries, marks the copy
/// returned and keeps the audit record, all or nothing.
pub async fn import(pool: &SqlitePool, bytes: &[u8], by: &str) -> Result<CopyRecord, String> {
    let file = read(bytes)?;
    let sha = hex(&Sha256::digest(bytes));
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<i64, String> = async {
        let copy_id = matching_copy(&mut tx, &file).await?;
        // Returned first, so the client's lock no longer refuses the accountant's dates.
        sqlx::query("UPDATE accountants_copies SET status = 'RETURNED' WHERE id = ?1").bind(copy_id).execute(&mut *tx).await.map_err(db_err)?;
        let mut created = Vec::new();
        for a in &file.accounts {
            if account_id(&mut tx, a.number.as_deref(), &a.full_name).await?.is_some() {
                continue;
            }
            let parent = match &a.parent_full_name {
                Some(p) => Some(account_id(&mut tx, None, p).await?.ok_or_else(|| format!("ACCOUNTANT_CHANGES_ACCOUNT: the parent account '{p}' isn't in the books"))?),
                None => None,
            };
            let id: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type, parent_id) VALUES (?1, ?2, ?3, ?4) RETURNING id")
                .bind(a.number.as_deref().filter(|n| !n.trim().is_empty()))
                .bind(&a.name)
                .bind(&a.account_type)
                .bind(parent)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| format!("ACCOUNTANT_CHANGES_ACCOUNT: the account '{}' couldn't be added ({})", a.full_name, db_err(e)))?;
            created.push(id);
        }
        let mut posted = Vec::new();
        for (i, e) in file.entries.iter().enumerate() {
            if e.date > file.dividing_date {
                return Err(format!("ACCOUNTANT_CHANGES_BAD: entry {} is dated after the dividing date", i + 1));
            }
            let mut lines = Vec::with_capacity(e.lines.len());
            for l in &e.lines {
                let account = account_id(&mut tx, l.account_number.as_deref(), &l.account_full_name)
                    .await?
                    .ok_or_else(|| format!("ACCOUNTANT_CHANGES_ACCOUNT: the account '{}' isn't in the books", l.account_full_name))?;
                lines.push(JournalLine { account_id: account, debit: Cents(l.debit_cents), credit: Cents(l.credit_cents), memo: l.memo.clone(), class_id: None });
            }
            let memo = if e.memo.trim().is_empty() { "From the accountant".to_string() } else { format!("{} (from the accountant)", e.memo.trim()) };
            let entry = JournalEntry { id: None, date: e.date, description: memo, lines, currency: None };
            posted.push(crate::ledger::post_entry_in_tx_flagged(&mut tx, &entry, EntrySource::Journal, e.adjusting).await?);
        }
        let summary = serde_json::json!({ "journalEntryIds": posted, "accountIds": created, "leftOut": file.left_out });
        sqlx::query(
            "UPDATE accountants_copies SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), closed_by = ?1, entries_posted = ?2, accounts_created = ?3,
                    changes_sha256 = ?4, import_summary_json = ?5 WHERE id = ?6",
        )
        .bind(by)
        .bind(posted.len() as i64)
        .bind(created.len() as i64)
        .bind(&sha)
        .bind(summary.to_string())
        .bind(copy_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        Ok(copy_id)
    }
    .await;
    match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            record(pool, id).await
        }
        Err(e) => Err(crate::ledger::rollback(tx, e).await),
    }
}

#[cfg(test)]
mod tests;
