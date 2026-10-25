//! The rest of the standard report set: General Ledger, A/R and A/P aging (summary and detail,
//! which also serve as Open Invoices and Unpaid Bills), Sales by Customer, Sales by Product/Service
//! and Expenses by Vendor. The financial statements, trial balance and cash flow live in
//! `statements`.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::preferences::{fiscal_start_month, fiscal_year_bounds};
use crate::registers::{account_register, RegisterRow};

// ------------------------------------------------------------------------------------------------
// General Ledger
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerAccount {
    pub account_id: i64,
    pub account_name: String,
    pub account_type_name: String,
    pub opening_balance_cents: i64,
    pub rows: Vec<RegisterRow>,
    pub total_debit_cents: i64,
    pub total_credit_cents: i64,
    pub ending_balance_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralLedger {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub accounts: Vec<LedgerAccount>,
}

/// Every account with a balance or activity, with its transactions in the period. Balance sheet
/// accounts start from their balance on the start date; income and expense accounts from their
/// fiscal-year-to-date amount, as they reset each year.
pub async fn general_ledger(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<GeneralLedger, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let (fiscal_start, _) = fiscal_year_bounds(start_date, fiscal_start_month(pool).await?);
    let accounts: Vec<(i64, bool, bool)> = sqlx::query_as(
        "SELECT c.id, c.is_balance_sheet,
                EXISTS (SELECT 1 FROM v_posted_lines l WHERE l.account_id = c.id AND l.entry_date <= ?1)
         FROM v_chart_of_accounts c ORDER BY c.type_sort_order, COALESCE(c.account_number, ''), c.full_name",
    )
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::new();
    for (id, is_balance_sheet, any) in accounts {
        if !any {
            continue;
        }
        let mut reg = account_register(pool, id, Some(start_date), Some(end_date), false).await?;
        if !is_balance_sheet {
            // Income and expense restart at the fiscal year: drop earlier years from the opening.
            let since: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1 AND entry_date >= ?2 AND entry_date < ?3",
            )
            .bind(id)
            .bind(fiscal_start)
            .bind(start_date)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
            let ytd_opening = if reg.is_debit_normal { since } else { -since };
            let shift = reg.opening_balance_cents - ytd_opening;
            reg.opening_balance_cents = ytd_opening;
            for row in &mut reg.rows {
                row.balance_cents -= shift;
            }
            reg.ending_balance_cents -= shift;
        }
        if reg.rows.is_empty() && reg.opening_balance_cents == 0 {
            continue;
        }
        out.push(LedgerAccount {
            account_id: id,
            total_debit_cents: reg.rows.iter().map(|r| r.debit_cents).sum(),
            total_credit_cents: reg.rows.iter().map(|r| r.credit_cents).sum(),
            account_name: reg.account_name,
            account_type_name: reg.account_type_name,
            opening_balance_cents: reg.opening_balance_cents,
            ending_balance_cents: reg.ending_balance_cents,
            rows: reg.rows,
        });
    }
    Ok(GeneralLedger { start_date, end_date, accounts: out })
}

// ------------------------------------------------------------------------------------------------
// Aging
// ------------------------------------------------------------------------------------------------

pub const AGING_LABELS: [&str; 5] = ["Current", "1 - 30", "31 - 60", "61 - 90", "91 and over"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingRow {
    pub party_name: String,
    /// Current, 1–30, 31–60, 61–90 and 91+ days past due.
    pub buckets_cents: [i64; 5],
    pub total_cents: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgingItem {
    pub party_name: String,
    /// `Invoice`, `Credit Memo`, `Bill` or `Vendor Credit`.
    pub kind: String,
    pub number: Option<String>,
    pub date: NaiveDate,
    pub due_date: NaiveDate,
    #[sqlx(skip)]
    pub days_past_due: i64,
    /// Still open as of the report date; negative for credit not yet used.
    pub open_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingReport {
    /// `RECEIVABLE` or `PAYABLE`.
    pub kind: String,
    pub as_of: NaiveDate,
    pub bucket_labels: [&'static str; 5],
    pub rows: Vec<AgingRow>,
    pub totals_cents: [i64; 5],
    pub total_cents: i64,
    /// Every open item, oldest due first within each name.
    pub items: Vec<AgingItem>,
}

fn bucket(days_past_due: i64) -> usize {
    match days_past_due {
        d if d <= 0 => 0,
        1..=30 => 1,
        31..=60 => 2,
        61..=90 => 3,
        _ => 4,
    }
}

/// What customers owed (`receivable`) or what was owed to vendors (`payable`) as of a date: each
/// document's balance after the payments and credits dated on or before it, aged by due date.
/// Credit not yet used shows as a negative amount aged from its date.
pub async fn aging(pool: &SqlitePool, payable: bool, as_of: NaiveDate) -> Result<AgingReport, String> {
    let sql = if payable {
        "SELECT * FROM (
            SELECT v.name AS party_name, 'Bill' AS kind, b.bill_number AS number, b.bill_date AS date, b.due_date,
                   b.total_cents
                   - COALESCE((SELECT SUM(p.amount_cents) FROM bill_payments p WHERE p.bill_id = b.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
                   - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.bill_id = b.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                   AS open_cents
            FROM bills b JOIN vendors v ON v.id = b.vendor_id
            WHERE b.is_voided = 0 AND b.journal_entry_id IS NOT NULL AND b.bill_date <= ?1
            UNION ALL
            SELECT v.name, 'Vendor Credit', c.reference, c.credit_date, c.credit_date,
                   -(c.total_cents
                     - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.vendor_credit_id = c.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                     - COALESCE((SELECT SUM(r.amount_cents) FROM vendor_credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                                 WHERE r.vendor_credit_id = c.id AND e.is_voided = 0 AND r.refund_date <= ?1), 0))
            FROM vendor_credits c JOIN vendors v ON v.id = c.vendor_id
            WHERE c.is_voided = 0 AND c.credit_date <= ?1
         ) WHERE open_cents <> 0 ORDER BY party_name COLLATE NOCASE, due_date"
    } else {
        "SELECT * FROM (
            SELECT c.name AS party_name, 'Invoice' AS kind, CAST(i.invoice_number AS TEXT) AS number, i.issue_date AS date, i.due_date,
                   COALESCE(i.home_total_cents, i.total_cents)
                   - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents)) FROM invoice_payments p
                               WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
                   - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                   AS open_cents
            FROM invoices i JOIN customers c ON c.id = i.customer_id
            WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.issue_date <= ?1
            UNION ALL
            SELECT c.name, 'Credit Memo', CAST(m.credit_number AS TEXT), m.credit_date, m.credit_date,
                   -(m.total_cents
                     - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.credit_memo_id = m.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                     - COALESCE((SELECT SUM(r.amount_cents) FROM credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                                 WHERE r.credit_memo_id = m.id AND e.is_voided = 0 AND r.refund_date <= ?1), 0))
            FROM credit_memos m JOIN customers c ON c.id = m.customer_id
            WHERE m.is_voided = 0 AND m.refund_account_id IS NULL AND m.credit_date <= ?1
         ) WHERE open_cents <> 0 ORDER BY party_name COLLATE NOCASE, due_date"
    };
    let mut items: Vec<AgingItem> = sqlx::query_as(sql).bind(as_of).fetch_all(pool).await.map_err(db_err)?;
    let mut rows: Vec<AgingRow> = Vec::new();
    let mut totals = [0i64; 5];
    for item in &mut items {
        item.days_past_due = (as_of - item.due_date).num_days();
        let b = bucket(item.days_past_due);
        if rows.last().map(|r| r.party_name.as_str()) != Some(item.party_name.as_str()) {
            rows.push(AgingRow { party_name: item.party_name.clone(), buckets_cents: [0; 5], total_cents: 0 });
        }
        let row = rows.last_mut().unwrap();
        row.buckets_cents[b] += item.open_cents;
        row.total_cents += item.open_cents;
        totals[b] += item.open_cents;
    }
    Ok(AgingReport {
        kind: if payable { "PAYABLE" } else { "RECEIVABLE" }.into(),
        as_of,
        bucket_labels: AGING_LABELS,
        total_cents: totals.iter().sum(),
        rows,
        totals_cents: totals,
        items,
    })
}

// ------------------------------------------------------------------------------------------------
// Sales and expense summaries
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRow {
    pub label: String,
    /// Units sold, scaled by 10,000 (product summaries only).
    pub quantity_e4: Option<i64>,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryReport {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rows: Vec<SummaryRow>,
    pub total_cents: i64,
}

fn summary(start_date: NaiveDate, end_date: NaiveDate, rows: Vec<SummaryRow>) -> SummaryReport {
    SummaryReport { start_date, end_date, total_cents: rows.iter().map(|r| r.amount_cents).sum(), rows }
}

/// Income (net of credits and voids) by customer, or expenses by vendor, for the period.
pub async fn by_party(pool: &SqlitePool, expenses: bool, start_date: NaiveDate, end_date: NaiveDate) -> Result<SummaryReport, String> {
    let (classification, sign) = if expenses { ("EXPENSE", "l.debit_cents - l.credit_cents") } else { ("INCOME", "l.credit_cents - l.debit_cents") };
    let rows: Vec<(Option<String>, i64)> = sqlx::query_as(&format!(
        "SELECT p.party_name, SUM({sign})
         FROM v_posted_lines l
         JOIN v_chart_of_accounts a ON a.id = l.account_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = l.journal_entry_id
         WHERE a.classification = ?1 AND l.entry_date BETWEEN ?2 AND ?3
         GROUP BY p.party_name COLLATE NOCASE
         HAVING SUM({sign}) <> 0
         ORDER BY p.party_name IS NULL, p.party_name COLLATE NOCASE"
    ))
    .bind(classification)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let none = if expenses { "No vendor" } else { "No customer" };
    Ok(summary(
        start_date,
        end_date,
        rows.into_iter().map(|(name, cents)| SummaryRow { label: name.unwrap_or_else(|| none.into()), quantity_e4: None, amount_cents: cents }).collect(),
    ))
}

/// Quantity and amount sold per product or service, from invoices and sales receipts less credit
/// memos, by document date. Voided documents are left out.
pub async fn sales_by_product(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<SummaryReport, String> {
    let rows: Vec<(Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT p.name, SUM(x.qty), SUM(x.amount) FROM (
            SELECT it.product_service_id AS product, it.quantity_e4 AS qty,
                   it.amount_cents * COALESCE(i.home_total_cents, i.total_cents) / i.total_cents AS amount
            FROM invoice_items it JOIN invoices i ON i.id = it.invoice_id
            WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.issue_date BETWEEN ?1 AND ?2
            UNION ALL
            SELECT sl.product_service_id, sl.quantity_e4, sl.amount_cents
            FROM sales_receipt_lines sl JOIN sales_receipts r ON r.id = sl.sales_receipt_id
            WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL AND r.receipt_date BETWEEN ?1 AND ?2
            UNION ALL
            SELECT cl.product_service_id, -cl.quantity_e4, -cl.amount_cents
            FROM credit_memo_lines cl JOIN credit_memos m ON m.id = cl.credit_memo_id
            WHERE m.is_voided = 0 AND m.journal_entry_id IS NOT NULL AND m.credit_date BETWEEN ?1 AND ?2
         ) x LEFT JOIN products_services p ON p.id = x.product
         GROUP BY x.product
         HAVING SUM(x.amount) <> 0 OR SUM(x.qty) <> 0
         ORDER BY p.name IS NULL, p.name COLLATE NOCASE",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(summary(
        start_date,
        end_date,
        rows.into_iter()
            .map(|(name, qty, cents)| SummaryRow { label: name.unwrap_or_else(|| "No product/service".into()), quantity_e4: Some(qty), amount_cents: cents })
            .collect(),
    ))
}

#[cfg(test)]
mod tests;
