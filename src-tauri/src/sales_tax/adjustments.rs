//! Sales tax adjustments (gaps row T70).
//!
//! An adjustment corrects what is owed on one rate without a sale: a rounding difference on the
//! return, a vendor's discount for filing on time, a penalty or interest, tax owed on an untaxed
//! sale. It raises or lowers the rate's liability account against an income or expense account
//! the user chooses (Dr the liability / Cr income to lower it; Dr expense / Cr the liability to
//! raise it), and the sales tax return lists it under the rate's agency for its date. It is a
//! journal entry made by this screen: voided, not edited, and left off Journal Entries' Edit….

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesTaxAdjustmentInput {
    /// An ordinary rate (a combined rate's parts are adjusted one by one).
    pub tax_rate_id: i64,
    pub adjustment_date: NaiveDate,
    /// Positive raises what is owed; negative lowers it.
    pub amount_cents: i64,
    /// The income or expense account on the other side.
    pub account_id: i64,
    #[serde(default)]
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesTaxAdjustmentView {
    pub id: i64,
    pub tax_rate_id: i64,
    pub tax_rate_name: String,
    pub agency_name: Option<String>,
    pub adjustment_date: String,
    pub amount_cents: i64,
    pub account_name: String,
    pub memo: Option<String>,
    pub journal_entry_id: i64,
    pub is_voided: bool,
}

/// Posts an adjustment; returns its id.
pub async fn adjust_sales_tax(pool: &SqlitePool, input: &SalesTaxAdjustmentInput) -> Result<i64, String> {
    let amount = input.amount_cents;
    let cents = amount.checked_abs().filter(|a| (1..=MAX_LINE_CENTS).contains(a)).ok_or("TAX_BAD_AMOUNT: enter the amount to raise or lower the tax owed by")?;
    let memo = input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let rate: Option<(String, i64, bool)> = sqlx::query_as("SELECT name, liability_account_id, is_group FROM tax_rates WHERE id = ?1")
            .bind(input.tax_rate_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        let (name, liability) = match rate {
            None => return Err(format!("TAX_NOT_FOUND: tax rate {} does not exist", input.tax_rate_id)),
            Some((_, _, true)) => return Err("TAX_ADJUST_COMBINED: adjust each rate in a combined rate on its own, so each agency's return is right".into()),
            Some((name, liability, false)) => (name, liability),
        };
        let kind: Option<String> = sqlx::query_scalar("SELECT a.classification FROM v_chart_of_accounts a WHERE a.id = ?1")
            .bind(input.account_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        if !matches!(kind.as_deref(), Some("INCOME" | "EXPENSE")) {
            return Err("TAX_ADJUST_ACCOUNT: choose an income or expense account for the other side of the adjustment".into());
        }
        let lines = if amount > 0 {
            vec![JournalLine::debit(input.account_id, cents), JournalLine::credit(liability, cents)]
        } else {
            vec![JournalLine::debit(liability, cents), JournalLine::credit(input.account_id, cents)]
        };
        let entry = JournalEntry {
            id: None,
            date: input.adjustment_date,
            description: format!("Sales tax adjustment - {name}{}", memo.map(|m| format!(" - {m}")).unwrap_or_default()),
            lines,
            currency: None,
        };
        let entry_id = post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?;
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO sales_tax_adjustments (tax_rate_id, adjustment_date, amount_cents, liability_account_id, account_id, memo, journal_entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
        )
        .bind(input.tax_rate_id)
        .bind(input.adjustment_date)
        .bind(amount)
        .bind(liability)
        .bind(input.account_id)
        .bind(memo)
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)
    }
    .await;
    match result {
        Ok(id) => crate::ledger::commit_write(tx).await.map(|()| id).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

pub async fn list_sales_tax_adjustments(pool: &SqlitePool) -> Result<Vec<SalesTaxAdjustmentView>, String> {
    sqlx::query_as(
        "SELECT x.id, x.tax_rate_id, r.name AS tax_rate_name, v.name AS agency_name, x.adjustment_date, x.amount_cents,
                a.full_name AS account_name, x.memo, x.journal_entry_id, e.is_voided
         FROM sales_tax_adjustments x JOIN tax_rates r ON r.id = x.tax_rate_id LEFT JOIN vendors v ON v.id = r.agency_vendor_id
         JOIN v_chart_of_accounts a ON a.id = x.account_id JOIN journal_entries e ON e.id = x.journal_entry_id
         ORDER BY x.adjustment_date DESC, x.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Adjustments dated in the period that aren't voided, summed per rate.
pub(crate) async fn adjustments_by_rate(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<Vec<(i64, i64)>, String> {
    sqlx::query_as(
        "SELECT x.tax_rate_id, SUM(x.amount_cents) FROM sales_tax_adjustments x JOIN journal_entries e ON e.id = x.journal_entry_id
         WHERE e.is_voided = 0 AND x.adjustment_date BETWEEN ?1 AND ?2 GROUP BY x.tax_rate_id",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
