//! Statement reconciliation. Amounts are expressed on the account's normal side, so a bank's
//! balance is positive when money is in the account and a credit card's when money is owed.
//!
//! Clearing a line sets `journal_lines.reconciliation_id`; locking stamps `reconciled_at`, after
//! which the schema freezes those lines and blocks voiding their entries.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, format_cents, rollback, DISPLAY_SOURCE_TYPE};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Reconciliation {
    pub id: i64,
    pub account_id: i64,
    pub statement_end_date: String,
    pub statement_ending_balance_cents: i64,
    pub beginning_balance_cents: i64,
    pub is_locked: bool,
    pub created_at: String,
    pub locked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileAccountState {
    pub account_id: i64,
    pub normal_balance: String,
    /// Sum of every line already locked into a completed reconciliation.
    pub reconciled_balance_cents: i64,
    pub last_statement_end_date: Option<String>,
    pub in_progress: Option<Reconciliation>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnreconciledLine {
    pub line_id: i64,
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub source_type: String,
    pub memo: Option<String>,
    pub debit_cents: i64,
    pub credit_cents: i64,
    /// Effect on the account's normal-side balance: positive increases it.
    pub amount_cents: i64,
    pub reconciliation_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileProgress {
    pub cleared_balance_cents: i64,
    pub difference_cents: i64,
}

const NORMAL_SIGNED: &str = "CASE t.normal_balance WHEN 'DEBIT' THEN l.debit_cents - l.credit_cents
                                                 ELSE l.credit_cents - l.debit_cents END";

async fn normal_balance(pool: &SqlitePool, account_id: i64) -> Result<String, String> {
    sqlx::query_scalar(
        "SELECT t.normal_balance FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("REC_UNKNOWN_ACCOUNT: account {account_id} does not exist"))
}

async fn reconciled_balance<'e, E: sqlx::Executor<'e, Database = Sqlite>>(executor: E, account_id: i64) -> Result<i64, String> {
    sqlx::query_scalar(&format!(
        "SELECT COALESCE(SUM({NORMAL_SIGNED}), 0)
         FROM journal_lines l
         JOIN chart_of_accounts a ON a.id = l.account_id
         JOIN account_types t ON t.code = a.account_type
         WHERE l.account_id = ?1 AND l.reconciled_at IS NOT NULL"
    ))
    .bind(account_id)
    .fetch_one(executor)
    .await
    .map_err(db_err)
}

async fn cleared_balance<'e, E: sqlx::Executor<'e, Database = Sqlite>>(executor: E, reconciliation_id: i64) -> Result<ReconcileProgress, String> {
    let (beginning, ending, cleared): (i64, i64, i64) = sqlx::query_as(&format!(
        "SELECT r.beginning_balance_cents, r.statement_ending_balance_cents,
                COALESCE((SELECT SUM({NORMAL_SIGNED})
                          FROM journal_lines l
                          JOIN chart_of_accounts a ON a.id = l.account_id
                          JOIN account_types t ON t.code = a.account_type
                          WHERE l.reconciliation_id = r.id), 0)
         FROM reconciliations r WHERE r.id = ?1"
    ))
    .bind(reconciliation_id)
    .fetch_optional(executor)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("REC_NOT_FOUND: reconciliation {reconciliation_id} does not exist"))?;
    let cleared_balance_cents = beginning + cleared;
    Ok(ReconcileProgress { cleared_balance_cents, difference_cents: ending - cleared_balance_cents })
}

pub async fn account_state(pool: &SqlitePool, account_id: i64) -> Result<ReconcileAccountState, String> {
    let normal_balance = normal_balance(pool, account_id).await?;
    let reconciled_balance_cents = reconciled_balance(pool, account_id).await?;
    let last_statement_end_date: Option<String> = sqlx::query_scalar(
        "SELECT MAX(statement_end_date) FROM reconciliations WHERE account_id = ?1 AND is_locked = 1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let in_progress = sqlx::query_as::<_, Reconciliation>(
        "SELECT * FROM reconciliations WHERE account_id = ?1 AND is_locked = 0",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(ReconcileAccountState { account_id, normal_balance, reconciled_balance_cents, last_statement_end_date, in_progress })
}

pub async fn list_reconciliations(pool: &SqlitePool, account_id: i64) -> Result<Vec<Reconciliation>, String> {
    sqlx::query_as::<_, Reconciliation>(
        "SELECT * FROM reconciliations WHERE account_id = ?1 AND is_locked = 1 ORDER BY statement_end_date DESC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// All posted lines on the account that are not yet part of a completed reconciliation.
/// `reconciliation_id` is set on lines ticked in the reconciliation currently in progress.
pub async fn unreconciled_lines(pool: &SqlitePool, account_id: i64) -> Result<Vec<UnreconciledLine>, String> {
    sqlx::query_as::<_, UnreconciledLine>(&format!(
        "SELECT l.id AS line_id, e.id AS journal_entry_id, e.entry_number, e.entry_date, {DISPLAY_SOURCE_TYPE} AS source_type,
                COALESCE(l.memo, e.memo) AS memo, l.debit_cents, l.credit_cents,
                {NORMAL_SIGNED} AS amount_cents, l.reconciliation_id
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         JOIN account_types t ON t.code = a.account_type
         WHERE l.account_id = ?1 AND l.reconciled_at IS NULL AND e.status = 'POSTED'
           -- A voided entry and its reversal net to zero, so neither is offered for clearing.
           AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
         ORDER BY e.entry_date, e.entry_number, l.line_number"
    ))
    .bind(account_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Starts a reconciliation, or updates the statement details of the one already in progress.
pub async fn start(
    pool: &SqlitePool,
    account_id: i64,
    statement_end_date: NaiveDate,
    statement_ending_balance_cents: i64,
) -> Result<Reconciliation, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM reconciliations WHERE account_id = ?1 AND is_locked = 0")
                .bind(account_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
        let id = match existing {
            Some(id) => {
                sqlx::query(
                    "UPDATE reconciliations SET statement_end_date = ?1, statement_ending_balance_cents = ?2 WHERE id = ?3",
                )
                .bind(statement_end_date)
                .bind(statement_ending_balance_cents)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
                id
            }
            None => {
                let beginning = reconciled_balance(&mut *tx, account_id).await?;
                sqlx::query_scalar(
                    "INSERT INTO reconciliations (account_id, statement_end_date, statement_ending_balance_cents, beginning_balance_cents)
                     VALUES (?1, ?2, ?3, ?4) RETURNING id",
                )
                .bind(account_id)
                .bind(statement_end_date)
                .bind(statement_ending_balance_cents)
                .bind(beginning)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?
            }
        };
        sqlx::query_as::<_, Reconciliation>("SELECT * FROM reconciliations WHERE id = ?1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)
    }
    .await;
    match result {
        Ok(rec) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(rec)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Ticks (`cleared = true`) or unticks lines in an in-progress reconciliation.
pub async fn set_lines_cleared(
    pool: &SqlitePool,
    reconciliation_id: i64,
    line_ids: &[i64],
    cleared: bool,
) -> Result<ReconcileProgress, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match write_cleared(&mut tx, reconciliation_id, line_ids, cleared).await {
        Ok(progress) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(progress)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_cleared(
    tx: &mut Transaction<'_, Sqlite>,
    reconciliation_id: i64,
    line_ids: &[i64],
    cleared: bool,
) -> Result<ReconcileProgress, String> {
    for &line_id in line_ids {
        let result = sqlx::query(
            "UPDATE journal_lines
             SET reconciliation_id = CASE WHEN ?3 THEN ?1 ELSE NULL END
             WHERE id = ?2 AND reconciled_at IS NULL
               AND (reconciliation_id IS NULL OR reconciliation_id = ?1)",
        )
        .bind(reconciliation_id)
        .bind(line_id)
        .bind(cleared)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() != 1 {
            return Err(format!("REC_LINE_UNAVAILABLE: line {line_id} is not available to this reconciliation"));
        }
    }
    cleared_balance(&mut **tx, reconciliation_id).await
}

/// Completes the reconciliation once the difference is exactly zero, freezing every cleared line.
pub async fn lock(pool: &SqlitePool, reconciliation_id: i64) -> Result<Reconciliation, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let progress = cleared_balance(&mut *tx, reconciliation_id).await?;
        if progress.difference_cents != 0 {
            return Err(format!(
                "REC_NOT_BALANCED: difference is {}; it must be $0.00 to finish",
                format_cents(progress.difference_cents)
            ));
        }
        sqlx::query(
            "UPDATE journal_lines SET reconciled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE reconciliation_id = ?1",
        )
        .bind(reconciliation_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let locked = sqlx::query(
            "UPDATE reconciliations SET is_locked = 1, locked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND is_locked = 0",
        )
        .bind(reconciliation_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if locked.rows_affected() != 1 {
            return Err(format!("REC_LOCKED: reconciliation {reconciliation_id} is already completed"));
        }
        sqlx::query_as::<_, Reconciliation>("SELECT * FROM reconciliations WHERE id = ?1")
            .bind(reconciliation_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)
    }
    .await;
    match result {
        Ok(rec) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(rec)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Abandons an in-progress reconciliation; its ticks are released (the FK sets them to NULL).
pub async fn cancel(pool: &SqlitePool, reconciliation_id: i64) -> Result<(), String> {
    let result = sqlx::query("DELETE FROM reconciliations WHERE id = ?1 AND is_locked = 0")
        .bind(reconciliation_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if result.rows_affected() != 1 {
        return Err(format!("REC_NOT_OPEN: reconciliation {reconciliation_id} is not in progress"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
