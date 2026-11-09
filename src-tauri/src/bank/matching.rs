//! Matching downloaded bank lines to transactions already entered (migration 0088).
//!
//! A downloaded line whose amount is what an entered check, deposit, payment or transfer moved in
//! the same account, dated near it, is that transaction coming through the bank: matching it posts
//! nothing and takes the line out of the review queue. The import matches a line by itself only
//! when exactly one transaction agrees (and, for a check, its number agrees); the review screen can
//! match, unmatch and show matched lines. Unmatching, or voiding the matched transaction, puts the
//! line back in the queue.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, rollback, DISPLAY_SOURCE_TYPE};

/// How far before the bank's date an entered transaction may be dated (a check clears weeks after
/// it is written), and how far after (a deposit entered a day or two late), for the import to match
/// it by itself.
const AUTO_DAYS_BEFORE: i64 = 45;
const AUTO_DAYS_AFTER: i64 = 7;
/// The wider window the Match list offers by hand.
const MANUAL_DAYS: i64 = 120;

/// An entered transaction a bank line could be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MatchCandidate {
    pub journal_entry_id: i64,
    pub date: String,
    /// As journal entries show it (`CHECK`, `DEPOSIT`, `BILL_PAYMENT`, ...).
    pub source_type: String,
    pub source_id: Option<i64>,
    pub num: Option<String>,
    pub name: Option<String>,
    pub memo: Option<String>,
    pub amount_cents: i64,
}

/// A matched bank line and what it matched.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedLine {
    pub id: i64,
    pub bank_account_id: i64,
    pub date: String,
    pub description: String,
    pub amount_cents: i64,
    /// `AUTO` (on import) or `MANUAL`.
    pub origin: String,
    pub matched: MatchCandidate,
}

/// The check number a bank's description carries ("CHECK 1042", "CHK #1042", "Check# 1042"), if any.
pub fn check_number(description: &str) -> Option<String> {
    let upper = description.to_uppercase();
    let at = upper.find("CHECK").map(|i| i + 5).or_else(|| upper.find("CHK").map(|i| i + 3))?;
    let rest = &upper[at..];
    let start = rest.find(|c: char| c.is_ascii_digit())?;
    // Only separators between the word and the number ("CHECK #1042", "CHK-1042").
    if !rest[..start].chars().all(|c| matches!(c, ' ' | '#' | '-' | ':' | '.' | 'N' | 'O')) {
        return None;
    }
    let digits: String = rest[start..].chars().take_while(char::is_ascii_digit).collect();
    let digits = digits.trim_start_matches('0');
    (!digits.is_empty()).then(|| digits.to_string())
}

/// Entered transactions in each line's account for its amount, dated `before`..`after` days around
/// it, not matched to another line and not added from the feed: (line, candidate), nearest date
/// first for each line. One query for any number of lines.
async fn candidates_for_lines(conn: &mut sqlx::SqliteConnection, transaction_ids: &[i64], before: i64, after: i64) -> Result<Vec<(i64, MatchCandidate)>, String> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT t.id AS line_id, e.id AS journal_entry_id, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type, e.source_id,
                {doc} AS num, p.party_name AS name, e.memo, t.amount_cents
         FROM bank_transactions t
         JOIN journal_lines l ON l.account_id = t.bank_account_id
         JOIN journal_entries e ON e.id = l.journal_entry_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
         WHERE t.id IN (SELECT value FROM json_each(?1)) AND e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
           AND e.entry_date BETWEEN date(t.txn_date, '-' || ?2 || ' days') AND date(t.txn_date, '+' || ?3 || ' days')
           AND NOT EXISTS (SELECT 1 FROM bank_matches m WHERE m.journal_entry_id = e.id AND m.is_undone = 0)
           AND NOT EXISTS (SELECT 1 FROM bank_transactions f WHERE f.journal_entry_id = e.id)
         GROUP BY t.id, e.id
         HAVING SUM(l.debit_cents - l.credit_cents) = MAX(t.amount_cents)
         ORDER BY t.id, abs(julianday(e.entry_date) - julianday(MAX(t.txn_date))), e.id",
        doc = crate::registers::DOC_NUMBER,
    );
    #[derive(sqlx::FromRow)]
    struct Row {
        line_id: i64,
        #[sqlx(flatten)]
        candidate: MatchCandidate,
    }
    let ids = serde_json::to_string(transaction_ids).map_err(|e| e.to_string())?;
    let rows: Vec<Row> = sqlx::query_as(&sql).bind(ids).bind(before).bind(after).fetch_all(conn).await.map_err(db_err)?;
    Ok(rows.into_iter().map(|r| (r.line_id, r.candidate)).collect())
}

/// The one candidate the import would match a line to: the only one that agrees, and, when the
/// bank names a check number, the only check with that number.
fn pick(description: &str, found: Vec<MatchCandidate>) -> Option<MatchCandidate> {
    let found: Vec<MatchCandidate> = match check_number(description) {
        Some(number) => found.into_iter().filter(|c| c.num.as_deref().map(|n| n.trim().trim_start_matches('0')) == Some(number.as_str())).collect(),
        None => found,
    };
    match <[MatchCandidate; 1]>::try_from(found) {
        Ok([one]) => Some(one),
        Err(_) => None,
    }
}

async fn auto_candidate(conn: &mut sqlx::SqliteConnection, transaction_id: i64, description: &str) -> Result<Option<MatchCandidate>, String> {
    let found = candidates_for_lines(conn, &[transaction_id], AUTO_DAYS_BEFORE, AUTO_DAYS_AFTER).await?;
    Ok(pick(description, found.into_iter().map(|(_, c)| c).collect()))
}

/// Marks a pending line matched to an entry, inside the caller's transaction.
async fn write_match(tx: &mut Transaction<'_, Sqlite>, transaction_id: i64, journal_entry_id: i64, origin: &str) -> Result<(), String> {
    let done = sqlx::query(
        "UPDATE bank_transactions SET status = 'EXCLUDED', reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND status = 'PENDING'",
    )
    .bind(transaction_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err(format!("BANK_TXN_NOT_PENDING: bank line {transaction_id} is not waiting for review"));
    }
    sqlx::query("INSERT INTO bank_matches (bank_transaction_id, journal_entry_id, origin) VALUES (?1, ?2, ?3)")
        .bind(transaction_id)
        .bind(journal_entry_id)
        .bind(origin)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") { "BANK_MATCH_TAKEN: that transaction is already matched to another bank line".into() } else { msg }
        })?;
    Ok(())
}

/// Matches a line the import just added when exactly one entered transaction agrees; returns
/// whether it did.
pub(crate) async fn auto_match(tx: &mut Transaction<'_, Sqlite>, transaction_id: i64, description: &str) -> Result<bool, String> {
    match auto_candidate(&mut **tx, transaction_id, description).await? {
        Some(c) => write_match(tx, transaction_id, c.journal_entry_id, "AUTO").await.map(|()| true),
        None => Ok(false),
    }
}

/// What the import would match each pending line to now (lines entered after the download).
pub async fn suggested_matches(pool: &SqlitePool, transaction_ids: &[(i64, String)]) -> Result<Vec<Option<MatchCandidate>>, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let ids: Vec<i64> = transaction_ids.iter().map(|(id, _)| *id).collect();
    let mut by_line: std::collections::HashMap<i64, Vec<MatchCandidate>> = std::collections::HashMap::new();
    for (line, candidate) in candidates_for_lines(&mut conn, &ids, AUTO_DAYS_BEFORE, AUTO_DAYS_AFTER).await? {
        by_line.entry(line).or_default().push(candidate);
    }
    Ok(transaction_ids.iter().map(|(id, description)| pick(description, by_line.remove(id).unwrap_or_default())).collect())
}

/// The entered transactions a pending line could be matched to by hand, nearest date first.
pub async fn candidates(pool: &SqlitePool, transaction_id: i64) -> Result<Vec<MatchCandidate>, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    Ok(candidates_for_lines(&mut conn, &[transaction_id], MANUAL_DAYS, MANUAL_DAYS).await?.into_iter().map(|(_, c)| c).collect())
}

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Matches a pending line to an entered transaction by hand.
pub async fn match_line(pool: &SqlitePool, transaction_id: i64, journal_entry_id: i64) -> Result<(), String> {
    run(pool, async |tx| write_match(tx, transaction_id, journal_entry_id, "MANUAL").await).await
}

/// Undoes a line's match: the line goes back to the review queue.
pub async fn unmatch(pool: &SqlitePool, transaction_id: i64) -> Result<(), String> {
    run(pool, async |tx| {
        let undone = sqlx::query("UPDATE bank_matches SET is_undone = 1 WHERE bank_transaction_id = ?1 AND is_undone = 0")
            .bind(transaction_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        if undone.rows_affected() != 1 {
            return Err(format!("BANK_MATCH_NOT_FOUND: bank line {transaction_id} is not matched"));
        }
        sqlx::query("UPDATE bank_transactions SET status = 'PENDING', reviewed_at = NULL WHERE id = ?1")
            .bind(transaction_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        Ok(())
    })
    .await
}

/// Matched lines, newest first, with what each matched.
pub async fn list_matched(pool: &SqlitePool) -> Result<Vec<MatchedLine>, String> {
    let sql = format!(
        "SELECT t.id, t.bank_account_id, t.txn_date, t.description, t.amount_cents, m.origin,
                e.id, e.entry_date, {DISPLAY_SOURCE_TYPE}, e.source_id, {doc}, p.party_name, e.memo
         FROM bank_matches m
         JOIN bank_transactions t ON t.id = m.bank_transaction_id
         JOIN journal_entries e ON e.id = m.journal_entry_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
         WHERE m.is_undone = 0
         ORDER BY t.txn_date DESC, t.id DESC",
        doc = crate::registers::DOC_NUMBER,
    );
    type Row = (i64, i64, NaiveDate, String, i64, String, i64, NaiveDate, String, Option<i64>, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = sqlx::query_as(&sql).fetch_all(pool).await.map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, bank_account_id, date, description, amount_cents, origin, entry, entry_date, source_type, source_id, num, name, memo)| MatchedLine {
            id,
            bank_account_id,
            date: date.to_string(),
            description,
            amount_cents,
            origin,
            matched: MatchCandidate { journal_entry_id: entry, date: entry_date.to_string(), source_type, source_id, num, name, memo, amount_cents },
        })
        .collect())
}

#[cfg(test)]
mod tests;
