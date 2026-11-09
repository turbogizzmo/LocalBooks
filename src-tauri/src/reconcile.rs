//! Statement reconciliation. Amounts are expressed on the account's normal side, so a bank's
//! balance is positive when money is in the account and a credit card's when money is owed.
//!
//! Clearing a line sets `journal_lines.reconciliation_id`; locking stamps `reconciled_at`, after
//! which the schema freezes those lines and blocks voiding their entries.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, format_cents, rollback, EntrySource, JournalEntry, JournalLine, DISPLAY_SOURCE_TYPE};

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

/// What a charge entered at the start of a reconciliation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChargeKind {
    /// A bank or card fee: the reconciled account goes down by it (a card owes more).
    ServiceCharge,
    /// Interest the account earned: the reconciled account goes up by it (a card owes less).
    InterestEarned,
}

/// A service charge or interest earned from the statement, entered when the reconciliation starts.
/// It posts as a journal entry between the reconciled account and `account_id` (an expense for a
/// charge, income for interest), and its line on the reconciled account is cleared at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileCharge {
    pub kind: ChargeKind,
    pub amount_cents: i64,
    pub date: NaiveDate,
    pub account_id: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
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
        "SELECT MAX(statement_end_date) FROM reconciliations r WHERE account_id = ?1 AND is_locked = 1 AND NOT EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = r.id)",
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
        "SELECT * FROM reconciliations r WHERE account_id = ?1 AND is_locked = 1
           AND NOT EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = r.id)
         ORDER BY statement_end_date DESC",
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
    start_with_charges(pool, account_id, statement_end_date, statement_ending_balance_cents, &[]).await
}

/// [`start`], posting the statement's service charge and interest earned (`charges`) in the same
/// transaction: each is a journal entry through the ledger, dated on or before the statement end,
/// and its line on the reconciled account is cleared into this reconciliation. Nothing posts if
/// any of it fails.
pub async fn start_with_charges(
    pool: &SqlitePool,
    account_id: i64,
    statement_end_date: NaiveDate,
    statement_ending_balance_cents: i64,
    charges: &[ReconcileCharge],
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
        for charge in charges {
            post_charge(&mut tx, id, account_id, statement_end_date, charge).await?;
        }
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

/// Posts one charge (see [`ReconcileCharge`]) and clears its line on the reconciled account.
async fn post_charge(
    tx: &mut Transaction<'_, Sqlite>,
    reconciliation_id: i64,
    account_id: i64,
    statement_end_date: NaiveDate,
    charge: &ReconcileCharge,
) -> Result<(), String> {
    let label = match charge.kind {
        ChargeKind::ServiceCharge => "Service charge",
        ChargeKind::InterestEarned => "Interest earned",
    };
    if charge.amount_cents <= 0 {
        return Err(format!("REC_CHARGE_AMOUNT: the {} must be more than zero", label.to_lowercase()));
    }
    if charge.date > statement_end_date {
        return Err(format!("REC_CHARGE_DATE: the {} must be dated on or before the statement end date", label.to_lowercase()));
    }
    if charge.account_id == account_id {
        return Err(format!("REC_CHARGE_ACCOUNT: choose an account other than the one being reconciled for the {}", label.to_lowercase()));
    }
    let amount = charge.amount_cents;
    // A charge lowers what an asset account holds (credit) and raises what a card owes (also a
    // credit); interest is the other way round.
    let (reconciled, other) = match charge.kind {
        ChargeKind::ServiceCharge => (JournalLine::credit(account_id, amount), JournalLine::debit(charge.account_id, amount)),
        ChargeKind::InterestEarned => (JournalLine::debit(account_id, amount), JournalLine::credit(charge.account_id, amount)),
    };
    let entry = JournalEntry {
        id: None,
        date: charge.date,
        description: label.to_string(),
        lines: vec![
            JournalLine { memo: Some(label.to_string()), ..reconciled },
            JournalLine { memo: Some(label.to_string()), class_id: charge.class_id, ..other },
        ],
        currency: None,
    };
    let entry_id = crate::ledger::post_entry_in_tx(tx, &entry, EntrySource::Journal).await?;
    let line_id: i64 = sqlx::query_scalar("SELECT id FROM journal_lines WHERE journal_entry_id = ?1 AND account_id = ?2")
        .bind(entry_id)
        .bind(account_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    write_cleared(tx, reconciliation_id, &[line_id], true).await?;
    Ok(())
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

/// Undo Last Reconciliation (docs/plans/undo-reconciliation.md): the locked reconciliation is left
/// as it was; an append-only `reconciliation_undos` row records the undo (its trigger refuses all
/// but the account's last reconciliation, or one while another is in progress, and writes the Audit
/// Log row), then the lines it cleared are released, and a sealed record goes on the ledger audit
/// trail. Nothing is posted. Returns how many lines were released.
pub async fn undo_last(pool: &SqlitePool, reconciliation_id: i64, reason: Option<&str>) -> Result<i64, String> {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let (account_id, account_name, statement_end_date): (i64, String, String) = sqlx::query_as(
            "SELECT r.account_id, a.name, r.statement_end_date FROM reconciliations r JOIN chart_of_accounts a ON a.id = r.account_id WHERE r.id = ?1",
        )
        .bind(reconciliation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("REC_NOT_FOUND: reconciliation {reconciliation_id} does not exist"))?;
        let (count, total): (i64, i64) = sqlx::query_as(&format!(
            "SELECT COUNT(*), COALESCE(SUM({NORMAL_SIGNED}), 0)
             FROM journal_lines l
             JOIN chart_of_accounts a ON a.id = l.account_id
             JOIN account_types t ON t.code = a.account_type
             WHERE l.reconciliation_id = ?1"
        ))
        .bind(reconciliation_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query("INSERT INTO reconciliation_undos (reconciliation_id, released_count, released_total_cents, reason) VALUES (?1, ?2, ?3, ?4)")
            .bind(reconciliation_id)
            .bind(count)
            .bind(total)
            .bind(reason)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query("UPDATE journal_lines SET reconciliation_id = NULL, reconciled_at = NULL WHERE reconciliation_id = ?1")
            .bind(reconciliation_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let payload = serde_json::json!({
            "kind": "UNDO_RECONCILIATION",
            "reconciliationId": reconciliation_id,
            "accountId": account_id,
            "statementEndDate": statement_end_date,
            "releasedCount": count,
            "amountCents": total,
            "detail": format!("Undid the reconciliation of {account_name} through {statement_end_date}"),
            "reason": reason,
        });
        sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('EDIT', ?1, ?2)")
            .bind(reconciliation_id)
            .bind(payload.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok(count)
    }
    .await;
    match result {
        Ok(count) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(count)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Abandons an in-progress reconciliation; its ticks are released (the FK sets them to NULL).
///
/// Statement files attached to it go with it: they belonged only to a reconciliation that no
/// longer exists (they are marked deleted, then removed, as `attachments_library` requires).
pub async fn cancel(pool: &SqlitePool, reconciliation_id: i64) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let removed = sqlx::query("DELETE FROM reconciliations WHERE id = ?1 AND is_locked = 0")
            .bind(reconciliation_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        if removed.rows_affected() != 1 {
            return Err(format!("REC_NOT_OPEN: reconciliation {reconciliation_id} is not in progress"));
        }
        let files = "record_kind = 'RECONCILIATION' AND record_id = ?1";
        sqlx::query(&format!("UPDATE record_attachments SET deleted_at = COALESCE(deleted_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) WHERE {files}"))
            .bind(reconciliation_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query(&format!("DELETE FROM record_attachments WHERE {files}")).bind(reconciliation_id).execute(&mut *tx).await.map_err(db_err)?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

#[cfg(test)]
mod tests;
