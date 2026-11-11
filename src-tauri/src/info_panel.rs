//! The information panel beside the customer, vendor and item lists and on forms (T56): the
//! summary figures for one name or item. Every figure comes from the same function the matching
//! list or report uses, so the panel can't disagree with them (tests check it):
//!
//! - open balance: the customer or vendor list (`sales::list_customers`, `expenses::list_vendors`),
//!   which A/R and A/P aging and the balance reports add up the same way;
//! - active estimates: `estimates::list_estimates` (pending or accepted, not yet invoiced);
//! - unbilled costs: `billable_costs::list_unbilled` (Sales › Bill Costs); unbilled mileage trips:
//!   `mileage::list_unbilled_trips`;
//! - open purchase orders: `workflows::list_purchase_orders` (status OPEN), as Open Purchase Orders;
//! - items: Inventory Valuation (`report_center::inventory_valuation`) for on hand, average cost and
//!   value, and Stock Status (`report_center::stock_status`) for on order and the next delivery.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartySummary {
    pub open_balance_cents: i64,
    /// Customers: estimates pending or accepted and not yet turned into an invoice.
    pub active_estimate_count: i64,
    pub active_estimate_cents: i64,
    /// Customers: billable expense and bill lines not yet on an invoice.
    pub unbilled_cost_count: i64,
    pub unbilled_cost_cents: i64,
    /// Customers: billable mileage trips not yet on an invoice.
    pub unbilled_trip_count: i64,
    /// Vendors: purchase orders still open.
    pub open_po_count: i64,
    pub open_po_cents: i64,
}

/// The summary for customer (`kind` = `customer`) or vendor (`vendor`) `id`.
pub async fn party_summary(pool: &SqlitePool, kind: &str, id: i64) -> Result<PartySummary, String> {
    match kind {
        "customer" => {
            let customers = crate::sales::list_customers(pool).await?;
            let customer = customers.iter().find(|c| c.id == id).ok_or_else(|| format!("PARTY_NOT_FOUND: customer {id} does not exist"))?;
            let estimates: Vec<_> = crate::estimates::list_estimates(pool)
                .await?
                .into_iter()
                .filter(|e| e.customer_name == customer.name && matches!(e.status.as_str(), "PENDING" | "ACCEPTED") && e.converted_to_invoice_id.is_none())
                .collect();
            let costs = crate::billable_costs::list_unbilled(pool, Some(id)).await?;
            let trips = crate::mileage::list_unbilled_trips(pool, Some(id)).await?;
            Ok(PartySummary {
                open_balance_cents: customer.open_balance_cents,
                active_estimate_count: estimates.len() as i64,
                active_estimate_cents: sum(estimates.iter().map(|e| e.total_cents))?,
                unbilled_cost_count: costs.len() as i64,
                unbilled_cost_cents: sum(costs.iter().map(|c| c.amount_cents))?,
                unbilled_trip_count: trips.len() as i64,
                ..PartySummary::default()
            })
        }
        "vendor" => {
            let vendors = crate::expenses::list_vendors(pool).await?;
            let vendor = vendors.iter().find(|v| v.id == id).ok_or_else(|| format!("PARTY_NOT_FOUND: vendor {id} does not exist"))?;
            let pos: Vec<_> = crate::workflows::list_purchase_orders(pool).await?.into_iter().filter(|o| o.vendor_id == id && o.status == "OPEN").collect();
            Ok(PartySummary {
                open_balance_cents: vendor.open_balance_cents,
                open_po_count: pos.len() as i64,
                open_po_cents: sum(pos.iter().map(|o| o.total_cents))?,
                ..PartySummary::default()
            })
        }
        other => Err(format!("PARTY_KIND: unknown kind {other}")),
    }
}

fn sum(mut values: impl Iterator<Item = i64>) -> Result<i64, String> {
    values.try_fold(0i64, |acc, v| acc.checked_add(v)).ok_or_else(|| "INFO_OVERFLOW: amount out of range".to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    /// False for a product or service whose stock isn't tracked: the stock figures are then empty.
    pub tracked: bool,
    pub qty_on_hand_e4: i64,
    pub qty_on_order_e4: i64,
    pub next_delivery: Option<NaiveDate>,
    /// Left to invoice on open sales orders, and on hand less that.
    pub qty_on_sales_order_e4: i64,
    pub qty_available_e4: i64,
    pub average_cost_cents: Option<i64>,
    pub asset_value_cents: i64,
    pub sales_price_cents: i64,
    /// The quantity at which to order more (× 10,000), when one is set.
    pub reorder_point_e4: Option<i64>,
    /// On hand is at or below the reorder point.
    pub at_reorder_point: bool,
    pub preferred_vendor_id: Option<i64>,
    pub preferred_vendor_name: Option<String>,
}

/// The summary for product or service `product_id`, as of `today`.
pub async fn item_summary(pool: &SqlitePool, product_id: i64, today: NaiveDate) -> Result<ItemSummary, String> {
    let item: Option<i64> = sqlx::query_scalar("SELECT id FROM inventory_items WHERE product_id = ?1").bind(product_id).fetch_optional(pool).await.map_err(db_err)?;
    type RecordRow = (i64, Option<i64>, Option<i64>, Option<String>);
    let record: Option<RecordRow> = sqlx::query_as(
        "SELECT p.rate_cents, p.reorder_point_e4, p.preferred_vendor_id, v.name
         FROM products_services p LEFT JOIN vendors v ON v.id = p.preferred_vendor_id WHERE p.id = ?1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (price, reorder_point_e4, preferred_vendor_id, preferred_vendor_name) =
        record.ok_or_else(|| format!("ITEM_NOT_FOUND: product or service {product_id} does not exist"))?;
    let base = ItemSummary { sales_price_cents: price, reorder_point_e4, preferred_vendor_id, preferred_vendor_name, ..ItemSummary::default() };
    let Some(item_id) = item else {
        return Ok(base);
    };
    let valuation = crate::report_center::inventory_valuation(pool, today).await?;
    let status = crate::report_center::stock_status(pool, today, today).await?;
    let value = valuation.rows.iter().find(|r| r.item_id == item_id);
    let stock = status.rows.iter().find(|r| r.item_id == item_id);
    let qty_on_hand_e4 = value.map_or(0, |r| r.qty_on_hand_e4);
    Ok(ItemSummary {
        tracked: true,
        qty_on_hand_e4,
        qty_on_order_e4: stock.map_or(0, |r| r.qty_on_order_e4),
        next_delivery: stock.and_then(|r| r.next_delivery),
        qty_on_sales_order_e4: stock.map_or(0, |r| r.qty_on_sales_order_e4),
        qty_available_e4: stock.map_or(qty_on_hand_e4, |r| r.qty_available_e4),
        average_cost_cents: value.and_then(|r| r.average_cost_cents),
        asset_value_cents: value.map_or(0, |r| r.asset_value_cents),
        at_reorder_point: reorder_point_e4.is_some_and(|point| qty_on_hand_e4 <= point),
        ..base
    })
}

/// Sets an item's reorder point (× 10,000; tracked items only) and preferred vendor, from its
/// record. `None` clears either.
pub async fn set_item_purchasing(pool: &SqlitePool, product_id: i64, reorder_point_e4: Option<i64>, preferred_vendor_id: Option<i64>) -> Result<(), String> {
    if reorder_point_e4.is_some_and(|q| q < 0) {
        return Err("ITEM_REORDER_NEGATIVE: a reorder point can't be below zero".into());
    }
    if reorder_point_e4.is_some() {
        let tracked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM inventory_items WHERE product_id = ?1)").bind(product_id).fetch_one(pool).await.map_err(db_err)?;
        if !tracked {
            return Err("ITEM_REORDER_UNTRACKED: track this item's stock first; a reorder point compares with the quantity on hand".into());
        }
    }
    if let Some(vendor) = preferred_vendor_id {
        let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM vendors WHERE id = ?1").bind(vendor).fetch_optional(pool).await.map_err(db_err)?;
        match active {
            None => return Err(format!("PARTY_NOT_FOUND: vendor {vendor} does not exist")),
            Some(false) => return Err("ITEM_VENDOR_INACTIVE: choose an active vendor".into()),
            Some(true) => {}
        }
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = sqlx::query("UPDATE products_services SET reorder_point_e4 = ?1, preferred_vendor_id = ?2 WHERE id = ?3")
        .bind(reorder_point_e4)
        .bind(preferred_vendor_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err);
    match result {
        Ok(done) if done.rows_affected() == 1 => crate::ledger::commit_write(tx).await.map_err(db_err),
        Ok(_) => Err(crate::ledger::rollback(tx, format!("ITEM_NOT_FOUND: product or service {product_id} does not exist")).await),
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

/// The longest note a customer or vendor keeps.
const NOTES_MAX: usize = 10_000;

/// Saves the notes of customer or vendor `id` from the information panel (the same field as the
/// record's Notes). Blank clears them. A list change, so the audit triggers record it.
pub async fn set_notes(pool: &SqlitePool, kind: &str, id: i64, notes: &str) -> Result<(), String> {
    let table = match kind {
        "customer" => "customers",
        "vendor" => "vendors",
        other => return Err(format!("PARTY_KIND: unknown kind {other}")),
    };
    let notes = notes.trim();
    if notes.chars().count() > NOTES_MAX {
        return Err(format!("PARTY_NOTES_LONG: keep notes to {NOTES_MAX} characters"));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = sqlx::query(&format!("UPDATE {table} SET notes = ?1 WHERE id = ?2"))
        .bind((!notes.is_empty()).then_some(notes))
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err);
    match result {
        Ok(done) if done.rows_affected() == 1 => crate::ledger::commit_write(tx).await.map_err(db_err),
        Ok(_) => Err(crate::ledger::rollback(tx, format!("PARTY_NOT_FOUND: {kind} {id} does not exist")).await),
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

#[cfg(test)]
mod tests;
