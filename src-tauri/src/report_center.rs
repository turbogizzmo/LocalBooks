//! The rest of the standard report set: General Ledger, A/R and A/P aging (summary and detail,
//! which also serve as Open Invoices and Unpaid Bills), Sales by Customer, Sales by Product/Service,
//! Expenses by Vendor, Inventory Valuation (summary and detail), Inventory Stock Status and Missing
//! Checks. The financial statements, trial balance and cash flow live in `statements`.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{db_err, mul_div_round, QUANTITY_SCALE};
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

// ------------------------------------------------------------------------------------------------
// Inventory
// ------------------------------------------------------------------------------------------------

/// Average cost per unit of `value_cents` over `qty_e4` units, rounded to the cent; `None` unless
/// there are units on hand.
fn unit_cost(value_cents: i64, qty_e4: i64) -> Option<i64> {
    if qty_e4 > 0 { mul_div_round(value_cents, QUANTITY_SCALE, qty_e4) } else { None }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryValuationRow {
    pub item_id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub description: Option<String>,
    pub qty_on_hand_e4: i64,
    pub average_cost_cents: Option<i64>,
    pub asset_value_cents: i64,
    pub sales_price_cents: i64,
    /// On hand at the sales price.
    pub retail_value_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryValuation {
    pub as_of: NaiveDate,
    pub rows: Vec<InventoryValuationRow>,
    pub total_asset_value_cents: i64,
    pub total_retail_value_cents: i64,
}

/// Quantity and value (moving average cost) of every tracked item as of a date: the stock movements
/// dated on or before it, so the total agrees with the inventory asset accounts on that date.
/// Inactive items are left out unless they still have stock or value.
pub async fn inventory_valuation(pool: &SqlitePool, as_of: NaiveDate) -> Result<InventoryValuation, String> {
    let items: Vec<(i64, String, Option<String>, Option<String>, i64, bool, i64, i64)> = sqlx::query_as(
        "SELECT i.id, p.name, i.sku, p.description, p.rate_cents, p.is_active,
                COALESCE(SUM(s.qty_change_e4), 0), COALESCE(SUM(s.value_change_cents), 0)
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id
         LEFT JOIN stock_ledger_entries s ON s.item_id = i.id AND s.movement_date <= ?1
         GROUP BY i.id ORDER BY p.name COLLATE NOCASE",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut rows = Vec::with_capacity(items.len());
    for (item_id, name, sku, description, sales_price_cents, is_active, qty, value) in items {
        if !is_active && qty == 0 && value == 0 {
            continue;
        }
        let retail_value_cents = mul_div_round(sales_price_cents, qty, QUANTITY_SCALE).ok_or("RPT_OVERFLOW: amount out of range")?;
        rows.push(InventoryValuationRow {
            item_id,
            name,
            sku,
            description,
            qty_on_hand_e4: qty,
            average_cost_cents: unit_cost(value, qty),
            asset_value_cents: value,
            sales_price_cents,
            retail_value_cents,
        });
    }
    Ok(InventoryValuation {
        as_of,
        total_asset_value_cents: rows.iter().map(|r| r.asset_value_cents).sum(),
        total_retail_value_cents: rows.iter().map(|r| r.retail_value_cents).sum(),
        rows,
    })
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InventoryMovementRow {
    pub movement_date: NaiveDate,
    /// `PURCHASE`, `SALE`, `ADJUSTMENT`, `COST_TRUE_UP` or `VOID`.
    pub kind: String,
    pub journal_entry_id: i64,
    pub entry_number: i64,
    /// The transaction that carried the movement (`BILL`, `INVOICE`, `JOURNAL`, …).
    pub source_type: String,
    pub party_name: Option<String>,
    pub qty_change_e4: i64,
    pub value_change_cents: i64,
    #[sqlx(skip)]
    pub qty_on_hand_e4: i64,
    #[sqlx(skip)]
    pub asset_value_cents: i64,
    #[sqlx(skip)]
    pub average_cost_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItemDetail {
    pub item_id: i64,
    pub name: String,
    pub opening_qty_e4: i64,
    pub opening_value_cents: i64,
    pub rows: Vec<InventoryMovementRow>,
    pub closing_qty_e4: i64,
    pub closing_value_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryValuationDetail {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub items: Vec<InventoryItemDetail>,
    pub total_value_cents: i64,
}

/// Every stock movement in the period per tracked item, from its quantity and value on the start
/// date, with the running quantity, value and average cost after each. Items with no stock and no
/// movement in the period are left out.
pub async fn inventory_valuation_detail(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<InventoryValuationDetail, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let items: Vec<(i64, String, i64, i64)> = sqlx::query_as(
        "SELECT i.id, p.name, COALESCE(SUM(s.qty_change_e4), 0), COALESCE(SUM(s.value_change_cents), 0)
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id
         LEFT JOIN stock_ledger_entries s ON s.item_id = i.id AND s.movement_date < ?1
         GROUP BY i.id ORDER BY p.name COLLATE NOCASE",
    )
    .bind(start_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = Vec::new();
    for (item_id, name, opening_qty, opening_value) in items {
        let mut rows: Vec<InventoryMovementRow> = sqlx::query_as(
            "SELECT s.movement_date, s.kind, e.id AS journal_entry_id, e.entry_number, e.source_type, p.party_name,
                    s.qty_change_e4, s.value_change_cents
             FROM stock_ledger_entries s
             JOIN journal_entries e ON e.id = s.journal_entry_id
             LEFT JOIN v_entry_party p ON p.journal_entry_id = COALESCE(e.reverses_entry_id, e.id)
             WHERE s.item_id = ?1 AND s.movement_date BETWEEN ?2 AND ?3
             ORDER BY s.movement_date, s.id",
        )
        .bind(item_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        if rows.is_empty() && opening_qty == 0 && opening_value == 0 {
            continue;
        }
        let (mut qty, mut value) = (opening_qty, opening_value);
        for r in &mut rows {
            qty += r.qty_change_e4;
            value += r.value_change_cents;
            r.qty_on_hand_e4 = qty;
            r.asset_value_cents = value;
            r.average_cost_cents = unit_cost(value, qty);
        }
        out.push(InventoryItemDetail { item_id, name, opening_qty_e4: opening_qty, opening_value_cents: opening_value, rows, closing_qty_e4: qty, closing_value_cents: value });
    }
    Ok(InventoryValuationDetail { start_date, end_date, total_value_cents: out.iter().map(|i| i.closing_value_cents).sum(), items: out })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockStatusRow {
    pub item_id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub description: Option<String>,
    pub qty_on_hand_e4: i64,
    /// Ordered on purchase orders still open.
    pub qty_on_order_e4: i64,
    /// The earliest expected date among those purchase orders.
    pub next_delivery: Option<NaiveDate>,
    /// Units sold in the period (sales less voided sales).
    pub qty_sold_e4: i64,
    pub sales_per_week_e4: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockStatus {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rows: Vec<StockStatusRow>,
}

/// Per tracked item: on hand at the end date, on order from open purchase orders, and the average
/// units sold per week over the period.
pub async fn stock_status(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<StockStatus, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let items: Vec<(i64, String, Option<String>, Option<String>, bool, i64, i64, Option<NaiveDate>, i64)> = sqlx::query_as(
        "SELECT i.id, p.name, i.sku, p.description, p.is_active,
                COALESCE((SELECT SUM(s.qty_change_e4) FROM stock_ledger_entries s WHERE s.item_id = i.id AND s.movement_date <= ?2), 0),
                COALESCE((SELECT SUM(l.quantity_e4) FROM purchase_order_lines l JOIN purchase_orders o ON o.id = l.purchase_order_id
                          WHERE o.status = 'OPEN' AND l.product_service_id = i.product_id), 0),
                (SELECT MIN(o.expected_date) FROM purchase_order_lines l JOIN purchase_orders o ON o.id = l.purchase_order_id
                 WHERE o.status = 'OPEN' AND l.product_service_id = i.product_id),
                -COALESCE((SELECT SUM(s.qty_change_e4) FROM stock_ledger_entries s LEFT JOIN stock_ledger_entries r ON r.id = s.reverses_id
                           WHERE s.item_id = i.id AND s.movement_date BETWEEN ?1 AND ?2
                             AND (s.kind = 'SALE' OR (s.kind = 'VOID' AND r.kind = 'SALE'))), 0)
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id
         ORDER BY p.name COLLATE NOCASE",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let days = (end_date - start_date).num_days() + 1;
    let mut rows = Vec::with_capacity(items.len());
    for (item_id, name, sku, description, is_active, on_hand, on_order, next_delivery, sold) in items {
        if !is_active && on_hand == 0 && on_order == 0 && sold == 0 {
            continue;
        }
        let sales_per_week_e4 = mul_div_round(sold, 7, days).ok_or("RPT_OVERFLOW: quantity out of range")?;
        rows.push(StockStatusRow { item_id, name, sku, description, qty_on_hand_e4: on_hand, qty_on_order_e4: on_order, next_delivery, qty_sold_e4: sold, sales_per_week_e4 });
    }
    Ok(StockStatus { start_date, end_date, rows })
}

// ------------------------------------------------------------------------------------------------
// Missing Checks
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CheckRow {
    pub number: String,
    pub date: NaiveDate,
    /// `Check`, `Bill Payment`, `Sales Tax Payment` or `Refund`.
    pub kind: String,
    pub party_name: Option<String>,
    pub memo: Option<String>,
    pub amount_cents: i64,
    pub is_voided: bool,
    pub journal_entry_id: Option<i64>,
    /// Another check from the account has the same number.
    #[sqlx(skip)]
    pub is_duplicate: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckGap {
    /// The gap comes right before `rows[before]`.
    pub before: usize,
    pub first: u64,
    pub last: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingChecks {
    pub account_id: i64,
    pub account_name: String,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    /// Numbered checks in number order (then by date).
    pub rows: Vec<CheckRow>,
    /// Runs of numbers between the first and last check that no check used.
    pub gaps: Vec<CheckGap>,
    pub missing_count: u64,
    pub duplicate_count: usize,
    /// Payments in the period with a reference that is not a check number (left out of the list).
    pub unnumbered_count: usize,
}

fn check_number(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() || s.len() > 18 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The checks written from an account, in number order, with the numbers skipped between them and
/// numbers used more than once. Checks are checks and the check-numbered payments: bill payments
/// (one check paying several bills of one vendor counts once), sales tax payments and customer
/// refunds. Voided checks keep their number, so they are not missing; a check replaced by an edit
/// is the same check and is listed once.
pub async fn missing_checks(pool: &SqlitePool, account_id: i64, start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> Result<MissingChecks, String> {
    if let (Some(s), Some(e)) = (start_date, end_date) {
        if e < s {
            return Err("RPT_BAD_RANGE: the end date is before the start date".into());
        }
    }
    let account_name: String = sqlx::query_scalar("SELECT full_name FROM v_chart_of_accounts WHERE id = ?1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or("ACCOUNT_NOT_FOUND: that account does not exist")?;
    let all: Vec<CheckRow> = sqlx::query_as(
        "SELECT * FROM (
            SELECT trim(x.check_number) AS number, x.expense_date AS date, 'Check' AS kind, v.name AS party_name, x.memo,
                   x.total_cents AS amount_cents, x.is_voided, x.journal_entry_id
            FROM expenses x LEFT JOIN vendors v ON v.id = x.vendor_id
            WHERE x.kind = 'CHECK' AND x.payment_account_id = ?1 AND x.journal_entry_id IS NOT NULL
              AND x.journal_entry_id NOT IN (SELECT old_entry_id FROM transaction_edits)
            UNION ALL
            SELECT trim(p.reference), p.payment_date, 'Bill Payment', v.name, NULL, SUM(p.amount_cents), p.is_voided, MIN(p.journal_entry_id)
            FROM bill_payments p JOIN bills b ON b.id = p.bill_id JOIN vendors v ON v.id = b.vendor_id
            WHERE p.payment_account_id = ?1 AND p.journal_entry_id IS NOT NULL
              AND p.journal_entry_id NOT IN (SELECT old_entry_id FROM transaction_edits)
            GROUP BY trim(p.reference), p.payment_date, b.vendor_id, p.is_voided
            UNION ALL
            SELECT trim(t.reference), t.payment_date, 'Sales Tax Payment', NULL, NULL, t.amount_cents, e.is_voided, t.journal_entry_id
            FROM sales_tax_payments t JOIN journal_entries e ON e.id = t.journal_entry_id
            WHERE t.account_id = ?1
            UNION ALL
            SELECT trim(r.reference), r.refund_date, 'Refund', c.name, NULL, r.amount_cents, e.is_voided, r.journal_entry_id
            FROM credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
            JOIN credit_memos m ON m.id = r.credit_memo_id JOIN customers c ON c.id = m.customer_id
            WHERE r.account_id = ?1
         ) WHERE number IS NOT NULL AND number <> '' AND (?2 IS NULL OR date >= ?2) AND (?3 IS NULL OR date <= ?3)",
    )
    .bind(account_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let (numbered, other): (Vec<CheckRow>, Vec<CheckRow>) = all.into_iter().partition(|r| check_number(&r.number).is_some());
    let mut rows: Vec<(u64, CheckRow)> = numbered.into_iter().map(|r| (check_number(&r.number).unwrap(), r)).collect();
    rows.sort_by(|(a, x), (b, y)| a.cmp(b).then(x.date.cmp(&y.date)).then(x.journal_entry_id.cmp(&y.journal_entry_id)));
    let mut gaps = Vec::new();
    let mut missing_count = 0;
    for i in 1..rows.len() {
        let (prev, this) = (rows[i - 1].0, rows[i].0);
        if this == prev {
            rows[i - 1].1.is_duplicate = true;
            rows[i].1.is_duplicate = true;
        } else if this > prev + 1 {
            gaps.push(CheckGap { before: i, first: prev + 1, last: this - 1 });
            missing_count += this - prev - 1;
        }
    }
    let rows: Vec<CheckRow> = rows.into_iter().map(|(_, r)| r).collect();
    Ok(MissingChecks {
        account_id,
        account_name,
        start_date,
        end_date,
        duplicate_count: rows.iter().filter(|r| r.is_duplicate).count(),
        rows,
        gaps,
        missing_count,
        unnumbered_count: other.len(),
    })
}

#[cfg(test)]
mod tests;
