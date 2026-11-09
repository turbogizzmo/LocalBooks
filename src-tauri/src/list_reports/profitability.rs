//! T49: Inventory Turnover by Item, and Invoice Profitability by Customer and in Detail.
//!
//! Both read what the ledger already holds, so they tie out to it:
//! - turnover reads the stock ledger (`stock_ledger_entries`), whose values are what the inventory
//!   asset account and Cost of Goods Sold were posted with;
//! - an invoice's profit is the income its entry credited less the cost of goods its entry debited
//!   (the stock it sold at average cost). A line for a product or service whose stock isn't tracked
//!   has no cost here: LocalBooks keeps no purchase cost for it.

use chrono::NaiveDate;
use serde_json::json;
use sqlx::SqlitePool;

use super::{done, head, labeled, plural, row, section};
use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{ColumnKind, RowKind};
use crate::ledger::{db_err, mul_div_round};

fn add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b).ok_or_else(|| "RPT_OVERFLOW: amount out of range".to_string())
}

/// `part / whole` as a percentage with one decimal ("38.2%"), or "" when `whole` is zero or the
/// percentage is too large to show.
pub(super) fn percent(part: i64, whole: i64) -> String {
    if whole == 0 {
        return String::new();
    }
    // Tenths of a percent, rounded half away from zero.
    let Some(tenths) = mul_div_round(part, 1_000, whole) else { return String::new() };
    let sign = if tenths < 0 { "-" } else { "" };
    format!("{sign}{}.{}%", tenths.unsigned_abs() / 10, tenths.unsigned_abs() % 10)
}

/// `numerator / denominator` to two decimals ("4.21"), or "" when the denominator is zero or the
/// ratio is too large to show.
pub(super) fn ratio(numerator: i64, denominator: i64) -> String {
    if denominator == 0 {
        return String::new();
    }
    let Some(hundredths) = mul_div_round(numerator, 100, denominator) else { return String::new() };
    let sign = if hundredths < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", hundredths.unsigned_abs() / 100, hundredths.unsigned_abs() % 100)
}

// ------------------------------------------------------------------------------------------------
// Inventory Turnover by Item
// ------------------------------------------------------------------------------------------------

/// Per tracked item over the range: inventory value at the start and end, their average, the cost
/// of goods sold (sales less voided sales, at the value the stock ledger took out), how many times
/// the average stock turned over, and the days of stock on hand that implies.
pub(super) async fn inventory_turnover(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let items: Vec<(i64, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT i.id, p.name,
                COALESCE((SELECT SUM(s.value_change_cents) FROM stock_ledger_entries s WHERE s.item_id = i.id AND s.movement_date < ?1), 0),
                COALESCE((SELECT SUM(s.value_change_cents) FROM stock_ledger_entries s WHERE s.item_id = i.id AND s.movement_date <= ?2), 0),
                -COALESCE((SELECT SUM(s.value_change_cents) FROM stock_ledger_entries s LEFT JOIN stock_ledger_entries r ON r.id = s.reverses_id
                           WHERE s.item_id = i.id AND s.movement_date BETWEEN ?1 AND ?2
                             AND (s.kind = 'SALE' OR (s.kind = 'VOID' AND r.kind = 'SALE'))), 0)
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id
         ORDER BY p.name COLLATE NOCASE",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let days = (end - start).num_days() + 1;
    let columns = vec![
        head("Item", ColumnKind::Text),
        head("Beginning Value", ColumnKind::Money),
        head("Ending Value", ColumnKind::Money),
        head("Average Value", ColumnKind::Money),
        head("Cost of Goods Sold", ColumnKind::Money),
        head("Turnover", ColumnKind::Text),
        head("Days on Hand", ColumnKind::Text),
    ];
    let mut rows = Vec::new();
    let mut totals = (0i64, 0i64, 0i64);
    let mut shown = 0;
    for (_, name, begin, finish, cogs) in &items {
        if *begin == 0 && *finish == 0 && *cogs == 0 {
            continue;
        }
        let average = mul_div_round(add(*begin, *finish)?, 1, 2).ok_or("RPT_OVERFLOW: amount out of range")?;
        totals = (add(totals.0, *begin)?, add(totals.1, *finish)?, add(totals.2, *cogs)?);
        rows.push(row(
            RowKind::Detail,
            0,
            vec![json!(name), json!(begin), json!(finish), json!(average), json!(cogs), json!(ratio(*cogs, average)), json!(days_on_hand(days, average, *cogs))],
        ));
        shown += 1;
    }
    let average = mul_div_round(add(totals.0, totals.1)?, 1, 2).ok_or("RPT_OVERFLOW: amount out of range")?;
    rows.push(row(
        RowKind::Total,
        0,
        vec![json!("TOTAL"), json!(totals.0), json!(totals.1), json!(average), json!(totals.2), json!(ratio(totals.2, average)), json!(days_on_hand(days, average, totals.2))],
    ));
    let notes = vec![format!(
        "{} with stock on hand or sold in the period. Turnover is the cost of goods sold over the {days} days divided by the average of the beginning and ending value; days on hand is how long that average stock lasts at that rate.",
        plural(shown, "item", "items")
    )];
    Ok(done(Some(start), end, columns, rows, shown, notes))
}

/// Days the average stock lasts at the period's rate of sale: days × average ÷ cost of goods sold.
fn days_on_hand(days: i64, average: i64, cogs: i64) -> String {
    if cogs <= 0 {
        return String::new();
    }
    mul_div_round(days, average, cogs).map(|d| d.to_string()).unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------
// Invoice Profitability
// ------------------------------------------------------------------------------------------------

/// One invoice's income and cost of goods, from its journal entry.
#[derive(sqlx::FromRow)]
struct InvoiceProfit {
    journal_entry_id: i64,
    customer: String,
    number: i64,
    date: String,
    income: i64,
    cogs: i64,
}

async fn invoice_profits(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<Vec<InvoiceProfit>, String> {
    sqlx::query_as(
        "SELECT e.id AS journal_entry_id, c.name AS customer, i.invoice_number AS number, e.entry_date AS date,
                COALESCE((SELECT SUM(l.credit_cents - l.debit_cents) FROM journal_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id
                          WHERE l.journal_entry_id = e.id AND a.classification = 'INCOME'), 0) AS income,
                COALESCE((SELECT SUM(l.debit_cents - l.credit_cents) FROM journal_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id
                          WHERE l.journal_entry_id = e.id AND a.account_type = 'COST_OF_GOODS_SOLD'), 0) AS cogs
         FROM invoices i JOIN journal_entries e ON e.id = i.journal_entry_id JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 AND e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2
         ORDER BY c.name COLLATE NOCASE, e.entry_date, i.invoice_number",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

const PROFIT_NOTE: &str =
    "Income is what each invoice credited to income accounts; cost is the cost of goods its tracked stock was sold at. Products and services whose stock isn't tracked carry no cost.";

/// Per customer (jobs as Customer:Job): invoices, income, cost of goods, gross profit and margin.
pub(super) async fn invoice_profitability_by_customer(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let invoices = invoice_profits(pool, start, end).await?;
    let columns = vec![
        head("Customer", ColumnKind::Text),
        head("Invoices", ColumnKind::Number),
        head("Income", ColumnKind::Money),
        head("Cost of Goods", ColumnKind::Money),
        head("Gross Profit", ColumnKind::Money),
        head("Margin", ColumnKind::Text),
    ];
    let mut rows = Vec::new();
    let mut total = (0usize, 0i64, 0i64);
    let mut i = 0;
    while i < invoices.len() {
        let customer = invoices[i].customer.clone();
        let (mut count, mut income, mut cogs) = (0usize, 0i64, 0i64);
        while i < invoices.len() && invoices[i].customer == customer {
            count += 1;
            income = add(income, invoices[i].income)?;
            cogs = add(cogs, invoices[i].cogs)?;
            i += 1;
        }
        let profit = income - cogs;
        rows.push(row(RowKind::Detail, 0, vec![json!(customer), json!(count), json!(income), json!(cogs), json!(profit), json!(percent(profit, income))]));
        total = (total.0 + count, add(total.1, income)?, add(total.2, cogs)?);
    }
    let customers = rows.len();
    let profit = total.1 - total.2;
    rows.push(row(RowKind::Total, 0, vec![json!("TOTAL"), json!(total.0), json!(total.1), json!(total.2), json!(profit), json!(percent(profit, total.1))]));
    Ok(done(Some(start), end, columns, rows, customers, vec![PROFIT_NOTE.to_string()]))
}

/// Every invoice in the range under its customer, with its income, cost, profit and margin.
pub(super) async fn invoice_profitability_detail(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let invoices = invoice_profits(pool, start, end).await?;
    let columns = vec![
        head("Date", ColumnKind::Date),
        head("Invoice", ColumnKind::Text),
        head("Income", ColumnKind::Money),
        head("Cost of Goods", ColumnKind::Money),
        head("Gross Profit", ColumnKind::Money),
        head("Margin", ColumnKind::Text),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = (0i64, 0i64);
    let mut i = 0;
    while i < invoices.len() {
        let customer = invoices[i].customer.clone();
        rows.push(section(0, w, &customer));
        let (mut income, mut cogs) = (0i64, 0i64);
        while i < invoices.len() && invoices[i].customer == customer {
            let inv = &invoices[i];
            let profit = inv.income - inv.cogs;
            let mut r = row(RowKind::Detail, 1, vec![json!(inv.date), json!(inv.number.to_string()), json!(inv.income), json!(inv.cogs), json!(profit), json!(percent(profit, inv.income))]);
            r.journal_entry_id = Some(inv.journal_entry_id);
            rows.push(r);
            income = add(income, inv.income)?;
            cogs = add(cogs, inv.cogs)?;
            i += 1;
        }
        let profit = income - cogs;
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {customer}"), &[json!(income), json!(cogs), json!(profit), json!(percent(profit, income))]));
        total = (add(total.0, income)?, add(total.1, cogs)?);
    }
    if !invoices.is_empty() {
        let profit = total.0 - total.1;
        rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total.0), json!(total.1), json!(profit), json!(percent(profit, total.0))]));
    }
    Ok(done(Some(start), end, columns, rows, invoices.len(), vec![PROFIT_NOTE.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_ratio_round_half_away_from_zero() {
        assert_eq!(percent(382, 1_000), "38.2%");
        assert_eq!(percent(1, 3), "33.3%");
        assert_eq!(percent(-50, 1_000), "-5.0%");
        assert_eq!(percent(5, 0), "");
        assert_eq!(ratio(421, 100), "4.21");
        assert_eq!(ratio(2, 3), "0.67");
        assert_eq!(ratio(1, 0), "");
        // Too large to show is blank, never a wrong 0.
        assert_eq!(percent(i64::MAX, 1), "");
        assert_eq!(ratio(i64::MAX, 1), "");
        assert_eq!(days_on_hand(365, 1_000, 4_000), "91");
        assert_eq!(days_on_hand(365, 1_000, 0), "");
    }
}
