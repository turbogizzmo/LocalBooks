//! Cash Flow Forecast (gap T47): the money on hand, and what open invoices will bring in and open
//! bills will take out, week by week from a date.
//!
//! Money in is each open invoice's balance (and, negative, each customer credit not yet used) on
//! its due date; money out is each open bill's balance (and vendor credit) on its due date. These
//! are exactly the items of the A/R and A/P aging reports as of the same date
//! ([`crate::report_center::aging`]), so the forecast's totals are the agings' totals. Documents
//! already past due go in a first column (expected now); those due after the last week go in a
//! last column, **Later**. Cash on hand is the bank accounts' balance, plus payments received but
//! not yet deposited (Undeposited Funds), as of the date.

use chrono::{Duration, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::report_center::{aging, AgingItem};

/// One column of the forecast.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastPeriod {
    /// "Past due", "Week of Nov 9", or "Later".
    pub label: String,
    /// `PAST_DUE`, `WEEK` or `LATER`.
    pub kind: &'static str,
    /// The week's first and last day (none for past due and later).
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
    pub money_in_cents: i64,
    pub money_out_cents: i64,
    pub net_cents: i64,
    /// Cash on hand once this column's money has come in and gone out.
    pub ending_cash_cents: i64,
    /// The documents in the column: (A/R or A/P, the aging item).
    pub items: Vec<ForecastItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastItem {
    /// `IN` (receivable) or `OUT` (payable).
    pub direction: &'static str,
    pub party_name: String,
    pub kind: String,
    pub number: Option<String>,
    pub due_date: NaiveDate,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashFlowForecast {
    pub as_of: NaiveDate,
    pub weeks: u32,
    pub bank_cents: i64,
    pub undeposited_cents: i64,
    pub starting_cash_cents: i64,
    pub periods: Vec<ForecastPeriod>,
    /// All money in and out: the A/R and A/P aging totals as of `as_of`.
    pub receivable_cents: i64,
    pub payable_cents: i64,
    pub ending_cash_cents: i64,
}

/// The most weeks a forecast spans.
pub const MAX_WEEKS: u32 = 26;

fn add(total: i64, cents: i64) -> Result<i64, String> {
    total.checked_add(cents).ok_or_else(|| "FORECAST_OVERFLOW: the amounts are too large to add up".to_string())
}

/// The forecast from `as_of`, `weeks` weeks long (1 to [`MAX_WEEKS`]).
pub async fn forecast(pool: &SqlitePool, as_of: NaiveDate, weeks: u32) -> Result<CashFlowForecast, String> {
    if !(1..=MAX_WEEKS).contains(&weeks) {
        return Err(format!("FORECAST_WEEKS: forecast 1 to {MAX_WEEKS} weeks"));
    }
    let (bank_cents, undeposited_cents): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN a.account_type = 'BANK' THEN l.debit_cents - l.credit_cents END), 0),
                COALESCE(SUM(CASE WHEN a.account_type = 'OTHER_CURRENT_ASSET' AND a.is_system = 1 THEN l.debit_cents - l.credit_cents END), 0)
         FROM v_posted_lines l JOIN chart_of_accounts a ON a.id = l.account_id
         WHERE l.entry_date <= ?1 AND (a.account_type = 'BANK' OR (a.account_type = 'OTHER_CURRENT_ASSET' AND a.is_system = 1))",
    )
    .bind(as_of)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let starting = add(bank_cents, undeposited_cents)?;

    let mut periods: Vec<ForecastPeriod> = Vec::with_capacity(weeks as usize + 2);
    let blank = |label: String, kind: &'static str, start: Option<NaiveDate>, end: Option<NaiveDate>| ForecastPeriod {
        label,
        kind,
        start,
        end,
        money_in_cents: 0,
        money_out_cents: 0,
        net_cents: 0,
        ending_cash_cents: 0,
        items: Vec::new(),
    };
    periods.push(blank("Past due".into(), "PAST_DUE", None, Some(as_of - Duration::days(1))));
    for w in 0..i64::from(weeks) {
        let start = as_of + Duration::days(7 * w);
        let end = start + Duration::days(6);
        periods.push(blank(format!("Week of {}", start.format("%b %-d")), "WEEK", Some(start), Some(end)));
    }
    let horizon = as_of + Duration::days(7 * i64::from(weeks));
    periods.push(blank("Later".into(), "LATER", Some(horizon), None));

    let column = |due: NaiveDate| -> usize {
        if due < as_of {
            0
        } else if due >= horizon {
            weeks as usize + 1
        } else {
            1 + usize::try_from((due - as_of).num_days() / 7).unwrap_or(0)
        }
    };
    let receivable = aging(pool, false, as_of).await?;
    let payable = aging(pool, true, as_of).await?;
    let place = |periods: &mut Vec<ForecastPeriod>, direction: &'static str, item: &AgingItem| -> Result<(), String> {
        let p = &mut periods[column(item.due_date)];
        if direction == "IN" {
            p.money_in_cents = add(p.money_in_cents, item.open_cents)?;
        } else {
            p.money_out_cents = add(p.money_out_cents, item.open_cents)?;
        }
        p.items.push(ForecastItem {
            direction,
            party_name: item.party_name.clone(),
            kind: item.kind.clone(),
            number: item.number.clone(),
            due_date: item.due_date,
            amount_cents: item.open_cents,
        });
        Ok(())
    };
    for item in &receivable.items {
        place(&mut periods, "IN", item)?;
    }
    for item in &payable.items {
        place(&mut periods, "OUT", item)?;
    }
    let mut cash = starting;
    for p in &mut periods {
        p.net_cents = p.money_in_cents.checked_sub(p.money_out_cents).ok_or("FORECAST_OVERFLOW: the amounts are too large to add up")?;
        cash = add(cash, p.net_cents)?;
        p.ending_cash_cents = cash;
        p.items.sort_by(|a, b| a.due_date.cmp(&b.due_date).then(a.direction.cmp(b.direction)).then(a.party_name.cmp(&b.party_name)));
    }
    Ok(CashFlowForecast {
        as_of,
        weeks,
        bank_cents,
        undeposited_cents,
        starting_cash_cents: starting,
        periods,
        receivable_cents: receivable.total_cents,
        payable_cents: payable.total_cents,
        ending_cash_cents: cash,
    })
}

#[cfg(test)]
mod tests;
