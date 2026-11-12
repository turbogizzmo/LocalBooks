//! The extra segments of the money bars on the sales and bills screens (gap T61): what is earned
//! but not yet invoiced and what is quoted, beside the sales bar's open, overdue and paid figures;
//! what is ordered and what is owed on credit cards, beside the bills bar's. Each segment lists the
//! transactions behind it, and each total comes from the same function as its report or list:
//!
//! | Segment | Ties to |
//! |---|---|
//! | Unbilled | Sales › Bill Costs (`billable_costs::list_unbilled` and `list_unbilled_time`) |
//! | Estimates | The estimates list: pending or accepted and not yet invoiced (as the information panel counts them) |
//! | Purchase orders | Open purchase orders (`workflows::list_purchase_orders`, status OPEN) |
//! | Card balances | Balance Sheet: what each credit card account is owed, as of today |

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// One transaction (or card account) behind a segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarItem {
    /// `COST` (a billable expense or bill line), `TIME`, `ESTIMATE`, `PURCHASE_ORDER` or `CARD`.
    pub kind: &'static str,
    /// The document's id (the time activity's, the account's for a card).
    pub id: i64,
    pub date: Option<String>,
    pub name: String,
    pub number: Option<String>,
    pub memo: Option<String>,
    pub amount_cents: i64,
    /// The customer, for unbilled costs and time (Bill Costs opens on them).
    pub customer_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarSegment {
    /// `UNBILLED`, `ESTIMATES`, `PURCHASE_ORDERS` or `CARDS`.
    pub key: &'static str,
    pub label: &'static str,
    pub cents: i64,
    pub count: i64,
    pub items: Vec<BarItem>,
}

fn total(items: &[BarItem]) -> Result<i64, String> {
    items.iter().try_fold(0i64, |acc, i| acc.checked_add(i.amount_cents)).ok_or_else(|| "BAR_OVERFLOW: amount out of range".to_string())
}

fn segment(key: &'static str, label: &'static str, items: Vec<BarItem>) -> Result<BarSegment, String> {
    Ok(BarSegment { key, label, cents: total(&items)?, count: i64::try_from(items.len()).unwrap_or(i64::MAX), items })
}

/// Whether a feature switch in Preferences is on (unset means on, as the screens read it).
async fn feature_on(pool: &SqlitePool, key: &str) -> Result<bool, String> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(key).fetch_optional(pool).await.map_err(db_err)?;
    Ok(value.as_deref() != Some("false"))
}

/// The sales bar's extra segments: unbilled costs and time, and open estimates (when estimates are
/// turned on).
pub async fn sales_segments(pool: &SqlitePool) -> Result<Vec<BarSegment>, String> {
    let mut unbilled: Vec<BarItem> = crate::billable_costs::list_unbilled(pool, None)
        .await?
        .into_iter()
        .map(|c| BarItem {
            kind: "COST",
            id: c.line_id,
            date: Some(c.date),
            name: c.customer_name,
            number: None,
            memo: c.memo.or(c.vendor_name).or(Some(c.account_name)),
            amount_cents: c.amount_cents,
            customer_id: Some(c.customer_id),
        })
        .collect();
    unbilled.extend(crate::billable_costs::list_unbilled_time(pool, None).await?.into_iter().map(|t| BarItem {
        kind: "TIME",
        id: t.id,
        date: Some(t.date),
        name: t.customer_name,
        number: None,
        memo: Some(t.service_name.unwrap_or(t.employee_name)),
        amount_cents: t.amount_cents,
        customer_id: Some(t.customer_id),
    }));
    unbilled.sort_by(|a, b| a.date.cmp(&b.date));
    let mut out = vec![segment("UNBILLED", "Unbilled", unbilled)?];
    if !feature_on(pool, crate::sales_prefs::ESTIMATES).await? {
        return Ok(out);
    }
    let estimates: Vec<BarItem> = crate::estimates::list_estimates(pool)
        .await?
        .into_iter()
        .filter(|e| matches!(e.status.as_str(), "PENDING" | "ACCEPTED") && e.converted_to_invoice_id.is_none())
        .map(|e| BarItem {
            kind: "ESTIMATE",
            id: e.id,
            date: Some(e.estimate_date),
            name: e.customer_name,
            number: Some(e.estimate_number.to_string()),
            memo: e.memo,
            amount_cents: e.total_cents,
            customer_id: None,
        })
        .collect();
    out.push(segment("ESTIMATES", "Estimates", estimates)?);
    Ok(out)
}

/// The bills bar's extra segments: open purchase orders (when inventory and purchase orders are
/// turned on), and what the credit cards owe as of `today`.
pub async fn bill_segments(pool: &SqlitePool, today: NaiveDate) -> Result<Vec<BarSegment>, String> {
    let mut out = Vec::new();
    if feature_on(pool, crate::preferences::tax_time::INVENTORY).await? {
        let orders: Vec<BarItem> = crate::workflows::list_purchase_orders(pool)
            .await?
            .into_iter()
            .filter(|o| o.status == "OPEN")
            .map(|o| BarItem {
                kind: "PURCHASE_ORDER",
                id: o.id,
                date: Some(o.po_date),
                name: o.vendor_name,
                number: Some(o.po_number.to_string()),
                memo: o.memo,
                amount_cents: o.total_cents.get(),
                customer_id: None,
            })
            .collect();
        out.push(segment("PURCHASE_ORDERS", "Purchase orders", orders)?);
    }
    let cards: Vec<i64> = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'CREDIT_CARD'").fetch_all(pool).await.map_err(db_err)?;
    let bs = crate::statements::balance_sheet(pool, today).await?;
    let owed: Vec<BarItem> = bs
        .lines
        .iter()
        .filter(|l| l.kind == crate::statements::LineKind::Account && l.account_id.is_some_and(|id| cards.contains(&id)) && l.amount_cents.unwrap_or(0) != 0)
        .map(|l| BarItem {
            kind: "CARD",
            id: l.account_id.unwrap_or_default(),
            date: None,
            name: l.label.clone(),
            number: None,
            memo: None,
            amount_cents: l.amount_cents.unwrap_or(0),
            customer_id: None,
        })
        .collect();
    out.push(segment("CARDS", "Card balances", owed)?);
    Ok(out)
}

#[cfg(test)]
mod tests;
