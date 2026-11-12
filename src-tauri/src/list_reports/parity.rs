//! The last reports of `docs/plans/desktop-parity.md` that LocalBooks' records can answer (gap T9):
//! the Terms Listing, Unbilled Costs by Job, Inventory Stock Status by Vendor and Closing Date
//! Exceptions. Each reads the same records the ledger and the other reports read, so its totals tie
//! out to them (see the tests).

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::{done, head, labeled, plural, row, section, CUSTOMER_NAMES};
use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{type_label, ColumnKind, RowKind};
use crate::ledger::db_err;

// ------------------------------------------------------------------------------------------------
// Terms Listing
// ------------------------------------------------------------------------------------------------

/// Parts per million as a percent with up to four decimals (a ppm is 0.0001%): 20000 → "2%",
/// 12500 → "1.25%", 1 → "0.0001%".
fn percent(ppm: i64) -> String {
    let whole = ppm / 10_000;
    let frac = ppm % 10_000;
    if frac == 0 {
        format!("{whole}%")
    } else {
        format!("{whole}.{}%", format!("{frac:04}").trim_end_matches('0'))
    }
}

pub(super) async fn terms_listing(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let terms: Vec<(String, i64, i64, Option<i64>, bool)> =
        sqlx::query_as("SELECT name, net_days, discount_ppm, discount_days, is_active FROM terms ORDER BY is_active DESC, net_days, name COLLATE NOCASE")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let columns = vec![
        head("Terms", ColumnKind::Text),
        head("Net Due In (days)", ColumnKind::Number),
        head("Discount", ColumnKind::Text),
        head("Discount If Paid Within (days)", ColumnKind::Number),
        head("Active", ColumnKind::Text),
    ];
    let rows = terms
        .iter()
        .map(|(name, net, ppm, days, active)| {
            row(
                RowKind::Detail,
                0,
                vec![
                    json!(name),
                    json!(net),
                    if *ppm == 0 { Value::Null } else { json!(percent(*ppm)) },
                    days.map_or(Value::Null, |d| json!(d)),
                    json!(if *active { "Yes" } else { "No" }),
                ],
            )
        })
        .collect();
    let notes = vec![format!("{} in Lists › Customer & Vendor Profile Lists › Terms List.", plural(terms.len(), "term", "terms"))];
    Ok(done(None, today, columns, rows, terms.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Unbilled Costs by Job
// ------------------------------------------------------------------------------------------------

pub(super) async fn unbilled_costs_by_job(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let costs = crate::billable_costs::list_unbilled(pool, None).await?;
    let names: BTreeMap<i64, String> = sqlx::query_as::<_, (i64, String)>(&format!("{CUSTOMER_NAMES} SELECT id, full_name FROM cn"))
        .fetch_all(pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .collect();
    // (job's full name, lower-cased for sorting; job id) → its costs, oldest first.
    let mut jobs: BTreeMap<(String, i64), Vec<&crate::billable_costs::UnbilledCost>> = BTreeMap::new();
    for c in &costs {
        let name = names.get(&c.customer_id).cloned().unwrap_or_else(|| c.customer_name.clone());
        jobs.entry((name.to_lowercase(), c.customer_id)).or_default().push(c);
    }
    let columns = vec![
        head("", ColumnKind::Text),
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Source Name", ColumnKind::Text),
        head("Account", ColumnKind::Text),
        head("Memo", ColumnKind::Text),
        head("Amount", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total: i64 = 0;
    for ((_, id), list) in &mut jobs {
        list.sort_by(|a, b| (&a.date, a.line_id).cmp(&(&b.date, b.line_id)));
        let name = names.get(id).cloned().unwrap_or_else(|| list[0].customer_name.clone());
        rows.push(section(0, w, &name));
        let mut sub: i64 = 0;
        for c in list.iter() {
            sub = sub.checked_add(c.amount_cents).ok_or("RPT_OVERFLOW: unbilled costs")?;
            rows.push(row(
                RowKind::Detail,
                1,
                vec![
                    Value::Null,
                    // A bill line, or a line of a check, card charge or other expense.
                    json!(if c.source == "BILL" { "Bill" } else { "Expense" }),
                    json!(c.date),
                    json!(c.vendor_name),
                    json!(c.account_name),
                    json!(c.memo),
                    json!(c.amount_cents),
                ],
            ));
        }
        total = total.checked_add(sub).ok_or("RPT_OVERFLOW: unbilled costs")?;
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {name}"), &[json!(sub)]));
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let notes = vec![format!(
        "{} on {}: billable expense and bill lines not yet on an invoice (Customers › Bill Costs puts them on one).",
        plural(costs.len(), "cost", "costs"),
        plural(jobs.len(), "customer or job", "customers and jobs")
    )];
    Ok(done(None, today, columns, rows, costs.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Inventory Stock Status by Vendor
// ------------------------------------------------------------------------------------------------

pub(super) async fn stock_status_by_vendor(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let valuation = crate::report_center::inventory_valuation(pool, today).await?;
    // inventory item id → (product's preferred vendor, reorder point, quantity on open POs)
    type ExtraRow = (i64, Option<i64>, Option<String>, Option<i64>, i64);
    let extra: Vec<ExtraRow> = sqlx::query_as(
        "SELECT i.id, p.preferred_vendor_id, v.name, p.reorder_point_e4,
                COALESCE((SELECT SUM(COALESCE(l.quantity_e4, 0)) FROM purchase_order_lines l JOIN purchase_orders o ON o.id = l.purchase_order_id
                          WHERE l.product_service_id = p.id AND o.status = 'OPEN' AND o.po_date <= ?1), 0)
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id LEFT JOIN vendors v ON v.id = p.preferred_vendor_id",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    type Extra = (Option<i64>, Option<String>, Option<i64>, i64);
    let extra: BTreeMap<i64, Extra> = extra.into_iter().map(|(i, v, n, r, po)| (i, (v, n, r, po))).collect();
    // (no vendor last, vendor name, vendor id) → rows
    let mut vendors: BTreeMap<(bool, String, i64), Vec<ReportItem<'_>>> = BTreeMap::new();
    for item in &valuation.rows {
        let (vendor, name, reorder, on_po) = extra.get(&item.item_id).cloned().unwrap_or((None, None, None, 0));
        let key = match (vendor, &name) {
            (Some(id), Some(n)) => (false, n.to_lowercase(), id),
            _ => (true, String::new(), 0),
        };
        vendors.entry(key).or_default().push(ReportItem { item, vendor_name: name, reorder, on_po });
    }
    let columns = vec![
        head("", ColumnKind::Text),
        head("Item", ColumnKind::Text),
        head("Description", ColumnKind::Text),
        head("Reorder Pt", ColumnKind::Quantity),
        head("On Hand", ColumnKind::Quantity),
        head("Order", ColumnKind::Text),
        head("On PO", ColumnKind::Quantity),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut to_order = 0;
    for ((none, _, _), items) in &vendors {
        let title = if *none { "No preferred vendor".to_string() } else { items[0].vendor_name.clone().unwrap_or_default() };
        rows.push(section(0, w, &title));
        for r in items {
            // Order: at or below the reorder point, counting what is already on order.
            let expected = r.item.qty_on_hand_e4.checked_add(r.on_po).ok_or("RPT_OVERFLOW: quantity on hand and on order")?;
            let order = r.reorder.is_some_and(|point| expected <= point);
            to_order += usize::from(order);
            rows.push(row(
                RowKind::Detail,
                1,
                vec![
                    Value::Null,
                    json!(r.item.name),
                    json!(r.item.description),
                    r.reorder.map_or(Value::Null, |p| json!(p)),
                    json!(r.item.qty_on_hand_e4),
                    if order { json!("Order") } else { Value::Null },
                    json!(r.on_po),
                ],
            ));
        }
    }
    let notes = vec![format!(
        "{} tracked, grouped by preferred vendor; {} at or below the reorder point counting what is on open purchase orders. On Hand as of {today}.",
        plural(valuation.rows.len(), "item", "items"),
        plural(to_order, "item is", "items are")
    )];
    Ok(done(None, today, columns, rows, valuation.rows.len(), notes))
}

struct ReportItem<'a> {
    item: &'a crate::report_center::InventoryValuationRow,
    vendor_name: Option<String>,
    reorder: Option<i64>,
    on_po: i64,
}

// ------------------------------------------------------------------------------------------------
// Closing Date Exceptions
// ------------------------------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct Exception {
    id: i64,
    entry_number: i64,
    entry_date: NaiveDate,
    source_type: String,
    memo: Option<String>,
    created_at: String,
    /// Who posted it, as the hash-chained audit trail records it (an External Accountant, or the
    /// user who moved the closing date back).
    entered_by: Option<String>,
    closed_at: String,
    amount_cents: i64,
}

/// Entries dated on or before the closing date that were entered after their date was first
/// closed: made while the closing date was moved back, or voids carrying a closed date.
pub(super) async fn closing_date_exceptions(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let lock = crate::closing::status(pool).await?.lock_date;
    let found: Vec<Exception> = match lock {
        None => Vec::new(),
        Some(lock) => sqlx::query_as(
            "SELECT * FROM (
                SELECT e.id, e.entry_number, e.entry_date, e.source_type, e.memo, e.created_at,
                       (SELECT a.actor FROM ledger_audit_log a WHERE a.action_type = 'POST' AND a.target_entity_id = e.id ORDER BY a.id LIMIT 1) AS entered_by,
                       (SELECT MIN(k.set_at) FROM closing_locks k WHERE k.lock_date >= e.entry_date) AS closed_at,
                       -- The first close covering its date, by the entry ids it recorded: exact,
                       -- where two clock times can be equal.
                       (SELECT k.last_entry_id FROM closing_locks k WHERE k.lock_date >= e.entry_date ORDER BY k.id LIMIT 1) AS closed_after_entry,
                       (SELECT COALESCE(SUM(l.debit_cents), 0) FROM journal_lines l WHERE l.journal_entry_id = e.id) AS amount_cents
                FROM journal_entries e
                WHERE e.status = 'POSTED' AND e.entry_date <= ?1 AND e.entry_date BETWEEN ?2 AND ?3
             ) WHERE closed_at IS NOT NULL AND (CASE WHEN closed_after_entry IS NOT NULL THEN id > closed_after_entry ELSE created_at > closed_at END)
             ORDER BY entry_date, entry_number",
        )
        .bind(lock)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await
        .map_err(db_err)?,
    };
    let columns = vec![
        head("Date", ColumnKind::Date),
        head("Type", ColumnKind::Text),
        // The journal entry's number; each row opens its transaction (invoice, check, bill...).
        head("Entry No.", ColumnKind::Number),
        head("Memo", ColumnKind::Text),
        head("Entered", ColumnKind::Text),
        head("Entered By", ColumnKind::Text),
        head("Date Closed Since", ColumnKind::Text),
        head("Amount", ColumnKind::Money),
    ];
    let w = columns.len();
    let shown = |t: &str| t.replace('T', " ").trim_end_matches('Z').chars().take(19).collect::<String>();
    let mut rows = Vec::new();
    let mut total: i64 = 0;
    for x in &found {
        total = total.checked_add(x.amount_cents).ok_or("RPT_OVERFLOW: exceptions")?;
        let mut r = row(
            RowKind::Detail,
            0,
            vec![
                json!(x.entry_date),
                json!(type_label(&x.source_type)),
                json!(x.entry_number),
                json!(x.memo),
                json!(shown(&x.created_at)),
                json!(x.entered_by),
                json!(shown(&x.closed_at)),
                json!(x.amount_cents),
            ],
        );
        r.journal_entry_id = Some(x.id);
        rows.push(r);
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let notes = vec![match lock {
        None => "No closing date is set (Company › Set Closing Date), so nothing is an exception.".to_string(),
        Some(lock) => format!(
            "The books are closed through {lock}. {} dated on or before then {} entered after {} date was closed (times are UTC).",
            plural(found.len(), "entry", "entries"),
            if found.len() == 1 { "was" } else { "were" },
            if found.len() == 1 { "its" } else { "their" }
        ),
    }];
    Ok(done(Some(start), end, columns, rows, found.len(), notes))
}
