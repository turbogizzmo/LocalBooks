//! Period-end revaluation of open foreign-currency receivables (unrealized exchange gains/losses).
//!
//! Each open foreign invoice is carried in Accounts Receivable at its booked rate. Restating it at a
//! period-end rate gives an unrealized gain or loss. LocalBooks posts the net difference on the
//! revaluation date (A/R against Unrealized Exchange Gain or Loss) and an exact reversal the next
//! day. The period's statements therefore show receivables at the period-end rate, while the books
//! return to booked rates, so payments still realize the right gain or loss against the booked rate.
//!
//! Only receivables can be in a foreign currency today (bills are home-currency, and foreign
//! expenses and checks are paid at once), so there are no open foreign payables to revalue.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{self, db_err, to_home_cents, EntrySource, JournalEntry, JournalLine, RATE_SCALE};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevaluationLine {
    pub invoice_id: i64,
    pub invoice_number: i64,
    pub customer_name: String,
    pub currency_code: String,
    /// Still owed, in the invoice currency.
    pub foreign_open_cents: i64,
    pub booked_rate_e6: i64,
    /// What A/R carries for it now (home currency, at the booked rate).
    pub carrying_cents: i64,
    pub rate_e6: Option<i64>,
    pub revalued_cents: Option<i64>,
    /// Revalued minus carrying: positive is a gain.
    pub change_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevaluationPreview {
    pub as_of: NaiveDate,
    pub lines: Vec<RevaluationLine>,
    /// Currencies with open balances but no rate given.
    pub missing_rates: Vec<String>,
    pub net_change_cents: i64,
}

/// Open foreign invoices as of `as_of`, restated at `rates` (code → home units per foreign unit ×1,000,000).
pub async fn preview(pool: &SqlitePool, as_of: NaiveDate, rates: &BTreeMap<String, i64>) -> Result<RevaluationPreview, String> {
    let open: Vec<(i64, i64, String, String, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT i.id, i.invoice_number, i.customer_name, i.currency_code, i.exchange_rate_e6, i.total_cents, i.home_total_cents,
                COALESCE((SELECT SUM(p.amount_cents) FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0),
                COALESCE((SELECT SUM(p.ar_relief_cents) FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
         FROM invoices i
         WHERE i.currency_code IS NOT NULL AND i.status <> 'DRAFT' AND i.is_voided = 0 AND i.issue_date <= ?1
         ORDER BY i.currency_code, i.invoice_number",
    )
    .bind(as_of.to_string())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut lines = Vec::new();
    let mut missing = Vec::new();
    let mut net = 0i64;
    for (invoice_id, invoice_number, customer_name, currency_code, booked_rate_e6, total, home_total, paid, relieved) in open {
        let foreign_open_cents = total - paid;
        if foreign_open_cents <= 0 {
            continue;
        }
        let carrying_cents = home_total - relieved;
        let rate_e6 = rates.get(&currency_code).copied();
        let revalued_cents = match rate_e6 {
            Some(r) if (1..=1_000_000 * RATE_SCALE).contains(&r) => {
                Some(to_home_cents(foreign_open_cents, r).ok_or("FX_REVAL_OVERFLOW: amount out of range")?)
            }
            Some(_) => return Err(format!("FX_REVAL_BAD_RATE: the {currency_code} rate must be greater than zero")),
            None => {
                if !missing.contains(&currency_code) {
                    missing.push(currency_code.clone());
                }
                None
            }
        };
        let change_cents = revalued_cents.map(|v| v - carrying_cents);
        net += change_cents.unwrap_or(0);
        lines.push(RevaluationLine {
            invoice_id,
            invoice_number,
            customer_name,
            currency_code,
            foreign_open_cents,
            booked_rate_e6,
            carrying_cents,
            rate_e6,
            revalued_cents,
            change_cents,
        });
    }
    Ok(RevaluationPreview { as_of, lines, missing_rates: missing, net_change_cents: net })
}

async fn account_named(tx: &mut sqlx::SqliteConnection, name: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = ?1 AND parent_id IS NULL AND is_active = 1")
        .bind(name)
        .fetch_optional(tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("COA_SYSTEM_ACCOUNT_MISSING: no active '{name}' account"))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RevaluationView {
    pub id: i64,
    pub as_of: String,
    pub rates: String,
    pub net_change_cents: i64,
    pub entry_number: i64,
    pub reversal_entry_number: i64,
    pub is_voided: bool,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<RevaluationView>, String> {
    sqlx::query_as(
        "SELECT r.id, r.as_of, r.rates, r.net_change_cents, e.entry_number, x.entry_number AS reversal_entry_number, e.is_voided
         FROM fx_revaluations r
         JOIN journal_entries e ON e.id = r.journal_entry_id
         JOIN journal_entries x ON x.id = r.reversal_entry_id
         ORDER BY r.as_of DESC, r.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Posts the revaluation as of `as_of` and its reversal the next day, atomically.
pub async fn post(pool: &SqlitePool, as_of: NaiveDate, rates: &BTreeMap<String, i64>) -> Result<RevaluationView, String> {
    let preview = preview(pool, as_of, rates).await?;
    if !preview.missing_rates.is_empty() {
        return Err(format!("FX_REVAL_RATES: enter a rate for {}", preview.missing_rates.join(", ")));
    }
    if preview.net_change_cents == 0 {
        return Err("FX_REVAL_NOTHING: at these rates the open balances are already carried correctly".into());
    }
    let reversal_date = as_of.checked_add_days(Days::new(1)).ok_or("FX_REVAL_DATE: date out of range")?;
    let used: BTreeMap<&String, i64> = preview.lines.iter().map(|l| (&l.currency_code, l.rate_e6.expect("checked"))).collect();
    let rate_text = used
        .iter()
        .map(|(code, r)| format!("{code} {}.{:06}", r / RATE_SCALE, r % RATE_SCALE).trim_end_matches('0').to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let live: Option<i64> = sqlx::query_scalar(
            "SELECT r.id FROM fx_revaluations r JOIN journal_entries e ON e.id = r.journal_entry_id WHERE r.as_of = ?1 AND e.is_voided = 0",
        )
        .bind(as_of.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        if live.is_some() {
            return Err(format!("FX_REVAL_EXISTS: {as_of} has already been revalued; void that revaluation first"));
        }
        let ar: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_RECEIVABLE' AND is_system = 1 ORDER BY id LIMIT 1")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        let fx = account_named(&mut tx, "Unrealized Exchange Gain or Loss").await?;
        let net = preview.net_change_cents;
        let (gain_lines, loss_lines) = (
            vec![JournalLine::debit(ar, net), JournalLine::credit(fx, net)],
            vec![JournalLine::debit(fx, -net), JournalLine::credit(ar, -net)],
        );
        let lines = if net > 0 { gain_lines } else { loss_lines };
        let entry = JournalEntry { id: None, date: as_of, description: format!("Unrealized FX revaluation as of {as_of} ({rate_text})"), lines: lines.clone(), currency: None };
        let entry_id = ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?;
        let reversal = JournalEntry {
            id: None,
            date: reversal_date,
            description: format!("Reversal of unrealized FX revaluation as of {as_of}"),
            lines: lines.into_iter().map(|l| JournalLine { debit: l.credit, credit: l.debit, ..l }).collect(),
            currency: None,
        };
        let reversal_id = ledger::post_entry_in_tx(&mut tx, &reversal, EntrySource::Journal).await?;
        let rates_json = serde_json::to_string(&used).map_err(|e| e.to_string())?;
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO fx_revaluations (as_of, rates, net_change_cents, journal_entry_id, reversal_entry_id) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
        )
        .bind(as_of.to_string())
        .bind(rates_json)
        .bind(net)
        .bind(entry_id)
        .bind(reversal_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)
    }
    .await;
    let id = match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            id
        }
        Err(err) => return Err(ledger::rollback(tx, err).await),
    };
    list(pool).await?.into_iter().find(|r| r.id == id).ok_or_else(|| "FX_REVAL_NOT_FOUND".into())
}

/// Undoes a revaluation by voiding both its entry and its reversal, atomically.
pub async fn void(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let pair: Option<(i64, i64)> = sqlx::query_as("SELECT journal_entry_id, reversal_entry_id FROM fx_revaluations WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let (entry, reversal) = pair.ok_or("FX_REVAL_NOT_FOUND: that revaluation does not exist")?;
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        ledger::void_in_tx(&mut tx, reversal).await?;
        ledger::void_in_tx(&mut tx, entry).await
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err),
        Err(err) => Err(ledger::rollback(tx, err).await),
    }
}

#[cfg(test)]
mod tests;
