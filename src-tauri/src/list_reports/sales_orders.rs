//! The sales order reports (gaps row T69): Open Sales Orders by Customer and by Item, and Pending
//! Sales. All read `sales_orders::open_lines`, so they agree with the Sales Orders screen, Stock
//! Status and each other. Sales orders post nothing, so none of these ties to the ledger: what they
//! add up to is what is left to invoice on open orders, at the orders' rates, before tax.

use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::{done, head, labeled, plural, row, section};
use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{ColumnHead, ColumnKind, RowKind};
use crate::sales_orders::{open_lines, pending_sales, OpenLine};

fn sum(values: impl IntoIterator<Item = i64>) -> Result<i64, String> {
    values.into_iter().try_fold(0i64, |a, v| a.checked_add(v)).ok_or_else(|| "RPT_OVERFLOW: amount out of range".to_string())
}

/// The columns after the group label: the line's own name (item or customer), the order and the
/// quantities.
fn columns(group: &str, other: &str) -> Vec<ColumnHead> {
    vec![
        head(group, ColumnKind::Text),
        head(other, ColumnKind::Text),
        head("Order", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Expected", ColumnKind::Date),
        head("Ordered", ColumnKind::Quantity),
        head("Invoiced", ColumnKind::Quantity),
        head("Left", ColumnKind::Quantity),
        head("Backordered", ColumnKind::Quantity),
        head("Open Amount", ColumnKind::Money),
    ]
}

fn item_of(l: &OpenLine) -> String {
    l.item_name.clone().unwrap_or_else(|| l.description.clone())
}

fn line_row(l: &OpenLine, other: String) -> crate::custom_reports::ReportRow {
    row(
        RowKind::Detail,
        1,
        vec![
            Value::Null,
            json!(other),
            json!(format!("#{}", l.order_number)),
            json!(l.order_date),
            json!(l.expected_date),
            json!(l.quantity_e4),
            json!(l.invoiced_qty_e4),
            json!(l.left_qty_e4),
            json!(l.backordered_qty_e4),
            json!(l.open_cents),
        ],
    )
}

fn orders_in(lines: &[OpenLine]) -> usize {
    let mut ids: Vec<i64> = lines.iter().map(|l| l.order_id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

/// Open Sales Orders by Customer: each open line under its customer, oldest order first.
pub(super) async fn open_by_customer(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let open = open_lines(pool).await?;
    let columns = columns("Customer", "Item");
    let w = columns.len();
    let mut groups: Vec<(String, i64, Vec<&OpenLine>)> = Vec::new();
    for l in &open.lines {
        match groups.iter_mut().find(|g| g.1 == l.customer_id) {
            Some(g) => g.2.push(l),
            None => groups.push((l.customer_name.clone(), l.customer_id, vec![l])),
        }
    }
    groups.sort_by_key(|g| (g.0.to_lowercase(), g.1));
    let mut rows = Vec::new();
    for (name, _, lines) in &groups {
        rows.push(section(0, w, name));
        rows.extend(lines.iter().map(|l| line_row(l, item_of(l))));
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {name}"), &[json!(sum(lines.iter().map(|l| l.open_cents))?)]));
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(open.total_open_cents)]));
    let notes = vec![format!(
        "{} with something left to invoice, as of {today}. Left is ordered less invoiced; Backordered is the part of it that stock on hand can't cover, oldest orders first. Sales orders don't post, so the open amount is not in your books.",
        plural(orders_in(&open.lines), "open sales order", "open sales orders")
    )];
    Ok(done(None, today, columns, rows, open.lines.len(), notes))
}

/// Open Sales Orders by Item: each open line under its item, with on hand, on sales orders and
/// available for a tracked item.
pub(super) async fn open_by_item(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let open = open_lines(pool).await?;
    let columns = columns("Item", "Customer");
    let w = columns.len();
    let mut groups: Vec<(String, Option<i64>, Vec<&OpenLine>)> = Vec::new();
    for l in &open.lines {
        let name = item_of(l);
        match groups.iter_mut().find(|g| g.1 == l.product_service_id && (g.1.is_some() || g.0 == name)) {
            Some(g) => g.2.push(l),
            None => groups.push((name, l.product_service_id, vec![l])),
        }
    }
    groups.sort_by_key(|g| (g.0.to_lowercase(), g.1));
    let mut rows = Vec::new();
    for (name, product, lines) in &groups {
        let stock = product.and_then(|p| open.items.iter().find(|i| i.product_service_id == p));
        let heading = match stock {
            Some(s) if s.on_hand_e4.is_some() => format!(
                "{name} · on hand {} · on sales orders {} · available {}",
                crate::forms::format_quantity(s.on_hand_e4.unwrap_or(0)),
                crate::forms::format_quantity(s.on_sales_order_e4),
                crate::forms::format_quantity(s.available_e4.unwrap_or(0))
            ),
            _ => name.clone(),
        };
        rows.push(section(0, w, &heading));
        rows.extend(lines.iter().map(|l| line_row(l, l.customer_name.clone())));
        let left = sum(lines.iter().map(|l| l.left_qty_e4))?;
        let backordered = sum(lines.iter().map(|l| l.backordered_qty_e4))?;
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {name}"), &[json!(left), json!(backordered), json!(sum(lines.iter().map(|l| l.open_cents))?)]));
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(open.total_open_cents)]));
    let notes = vec![format!(
        "{} on open sales orders, as of {today}. Available is on hand less what is left on open orders; below zero, that much is backordered. Stock leaves only when an order is invoiced.",
        plural(groups.len(), "item", "items")
    )];
    Ok(done(None, today, columns, rows, open.lines.len(), notes))
}

/// Pending Sales: by customer, what is ordered and not yet invoiced.
pub(super) async fn pending(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let pending = pending_sales(pool).await?;
    let columns = vec![head("Customer", ColumnKind::Text), head("Open Orders", ColumnKind::Number), head("Not Yet Invoiced", ColumnKind::Money)];
    let w = columns.len();
    let mut rows: Vec<_> = pending.rows.iter().map(|r| row(RowKind::Detail, 0, vec![json!(r.customer_name), json!(r.orders), json!(r.open_cents)])).collect();
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(pending.total_cents)]));
    let notes = vec![format!(
        "Sales agreed on sales orders and not yet invoiced, as of {today}, before tax: {}. Nothing here has posted; each sale posts when its invoice does.",
        plural(pending.rows.len(), "customer", "customers")
    )];
    Ok(done(None, today, columns, rows, pending.rows.len(), notes))
}
