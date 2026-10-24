//! Statement reconciliation. Amounts are expressed on the account's normal side, so a bank's
//! balance is positive when money is in the account and a credit card's when money is owed.
//!
//! Clearing a line sets `journal_lines.reconciliation_id`; locking stamps `reconciled_at`, after
//! which the schema freezes those lines and blocks voiding their entries.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, format_cents, rollback};

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
        "SELECT l.id AS line_id, e.id AS journal_entry_id, e.entry_number, e.entry_date, e.source_type,
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
    let mut tx = pool.begin().await.map_err(db_err)?;
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
            tx.commit().await.map_err(db_err)?;
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
    let mut tx = pool.begin().await.map_err(db_err)?;
    match write_cleared(&mut tx, reconciliation_id, line_ids, cleared).await {
        Ok(progress) => {
            tx.commit().await.map_err(db_err)?;
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
    let mut tx = pool.begin().await.map_err(db_err)?;
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
            tx.commit().await.map_err(db_err)?;
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
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
            .bind(number)
            .fetch_one(pool)
            .await
            .expect("seeded account exists")
    }

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    async fn post(pool: &SqlitePool, date: NaiveDate, debit: i64, credit: i64, cents: i64) -> i64 {
        post_journal_entry(
            pool,
            JournalEntry {
                id: None,
                date,
                description: String::new(),
                lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)],
                currency: None,
            },
        )
        .await
        .expect("posts")
    }

    #[tokio::test]
    async fn reconcile_balances_locks_and_freezes() {
        let pool = test_pool().await;
        let (checking, owner, rent) = (
            account_id(&pool, "1010").await,
            account_id(&pool, "3100").await,
            account_id(&pool, "6300").await,
        );
        let deposit = post(&pool, d(9, 2), checking, owner, 500_000).await;
        post(&pool, d(9, 10), rent, checking, 120_000).await;
        post(&pool, d(9, 28), rent, checking, 5_000).await; // not yet on the statement
        post(&pool, d(10, 3), rent, checking, 7_500).await; // after statement date

        let rec = start(&pool, checking, d(9, 30), 380_000).await.expect("starts");
        assert_eq!(rec.beginning_balance_cents, 0);

        let lines = unreconciled_lines(&pool, checking).await.unwrap();
        assert_eq!(lines.iter().map(|l| l.amount_cents).collect::<Vec<_>>(), vec![500_000, -120_000, -5_000, -7_500]);

        let progress = set_lines_cleared(&pool, rec.id, &[lines[0].line_id, lines[1].line_id], true).await.unwrap();
        assert_eq!(progress, ReconcileProgress { cleared_balance_cents: 380_000, difference_cents: 0 });

        let err = set_lines_cleared(&pool, rec.id, &[lines[3].line_id], true).await.expect_err("after statement");
        assert!(err.starts_with("REC_AFTER_STATEMENT"), "{err}");

        // Ticking the $50 line puts it out of balance, so locking must fail.
        set_lines_cleared(&pool, rec.id, &[lines[2].line_id], true).await.unwrap();
        let err = lock(&pool, rec.id).await.expect_err("out of balance");
        assert!(err.starts_with("REC_NOT_BALANCED: difference is $50.00"), "{err}");
        let progress = set_lines_cleared(&pool, rec.id, &[lines[2].line_id], false).await.unwrap();
        assert_eq!(progress.difference_cents, 0);

        let locked = lock(&pool, rec.id).await.expect("locks");
        assert!(locked.is_locked);

        let remaining = unreconciled_lines(&pool, checking).await.unwrap();
        assert_eq!(remaining.len(), 2);
        let state = account_state(&pool, checking).await.unwrap();
        assert_eq!(state.reconciled_balance_cents, 380_000);
        assert_eq!(state.last_statement_end_date.as_deref(), Some("2026-09-30"));
        assert!(state.in_progress.is_none());

        // Frozen: reconciled lines cannot be unticked, their entries cannot be voided,
        // and a new statement must end after the last one.
        let err = set_lines_cleared(&pool, rec.id, &[lines[0].line_id], false).await.expect_err("frozen");
        assert!(err.starts_with("REC_LOCKED") || err.starts_with("REC_LINE_UNAVAILABLE"), "{err}");
        let void = sqlx::query("UPDATE journal_entries SET status = 'VOID', voided_at = '2026-10-01T00:00:00.000Z' WHERE id = ?1")
            .bind(deposit)
            .execute(&pool)
            .await
            .expect_err("void blocked");
        assert!(void.to_string().contains("REC_LOCKED"), "{void}");
        let err = start(&pool, checking, d(9, 15), 0).await.expect_err("date must advance");
        assert!(err.starts_with("REC_DATE_NOT_AFTER_LAST"), "{err}");

        let next = start(&pool, checking, d(10, 31), 367_500).await.expect("next statement");
        assert_eq!(next.beginning_balance_cents, 380_000);
        let ids: Vec<i64> = remaining.iter().map(|l| l.line_id).collect();
        let progress = set_lines_cleared(&pool, next.id, &ids, true).await.unwrap();
        assert_eq!(progress.difference_cents, 0);

        cancel(&pool, next.id).await.expect("cancels");
        assert!(unreconciled_lines(&pool, checking).await.unwrap().iter().all(|l| l.reconciliation_id.is_none()));
    }

    #[tokio::test]
    async fn credit_card_balances_on_the_credit_side() {
        let pool = test_pool().await;
        let (card, fuel, checking) = (
            account_id(&pool, "2100").await,
            account_id(&pool, "6100").await,
            account_id(&pool, "1010").await,
        );
        post(&pool, d(9, 5), fuel, card, 6_000).await; // charge raises the amount owed
        post(&pool, d(9, 20), card, checking, 2_500).await; // payment lowers it

        let rec = start(&pool, card, d(9, 30), 3_500).await.unwrap();
        let ids: Vec<i64> = unreconciled_lines(&pool, card).await.unwrap().iter().map(|l| l.line_id).collect();
        let progress = set_lines_cleared(&pool, rec.id, &ids, true).await.unwrap();
        assert_eq!(progress, ReconcileProgress { cleared_balance_cents: 3_500, difference_cents: 0 });
        lock(&pool, rec.id).await.expect("locks");
    }

    #[tokio::test]
    async fn rejects_accounts_that_cannot_be_reconciled() {
        let pool = test_pool().await;
        let ar = account_id(&pool, "1200").await;
        let err = start(&pool, ar, d(9, 30), 0).await.expect_err("A/R is not reconcilable");
        assert!(err.starts_with("REC_ACCOUNT_TYPE"), "{err}");
    }
}
