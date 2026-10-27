//! Change Item Prices: set new sales prices for many products and services at once.
//!
//! A new price starts from each item's current sales price or its unit cost, moves by a fixed amount
//! or a percentage up or down, and is then rounded. All in integer cents:
//!
//! - percentage: `base + round_half_up(base × percent_milli / 100,000)`, where `percent_milli` is the
//!   change in thousandths of a percent (+12.5% → 12,500; −10% → −10,000);
//! - amount: `base + amount_cents`;
//! - rounding to a step (5¢, 10¢, 25¢, $1, …): to the nearest step (halves go up), always up, or
//!   always down; or up to the next price ending in .99.
//!
//! An item's unit cost is its moving average cost while stock is on hand, otherwise its last purchase
//! cost; products whose stock isn't tracked have no cost and are left out of a change based on cost.
//! A price that would fall below zero is left unchanged.
//!
//! Applying writes the new prices and one PRICE_CHANGE event in the ledger audit trail listing every
//! item changed with its old and new price, in one transaction. Invoices, estimates and other
//! documents already written keep the prices they were written with.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, mul_div_round, rollback, MAX_LINE_CENTS};

const PERCENT_SCALE: i64 = 100_000;
/// Largest change accepted: ±1,000% or ±1,000,000.00.
pub const MAX_PERCENT_MILLI: i64 = 1_000_000;
pub const MAX_AMOUNT_CENTS: i64 = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Basis {
    CurrentPrice,
    Cost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Adjustment {
    /// Thousandths of a percent, negative to lower prices.
    #[serde(rename_all = "camelCase")]
    Percent { percent_milli: i64 },
    /// Cents, negative to lower prices.
    #[serde(rename_all = "camelCase")]
    Amount { amount_cents: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Rounding {
    /// To the cent only.
    None,
    #[serde(rename_all = "camelCase")]
    Nearest { step_cents: i64 },
    #[serde(rename_all = "camelCase")]
    Up { step_cents: i64 },
    #[serde(rename_all = "camelCase")]
    Down { step_cents: i64 },
    /// Up to the next price ending in .99 (12.40 → 12.99, 13.00 → 13.99, 12.99 stays).
    EndIn99,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceChangeInput {
    pub product_ids: Vec<i64>,
    pub basis: Basis,
    pub adjustment: Adjustment,
    pub rounding: Rounding,
}

/// Validates the change itself (not the items).
pub fn validate(adjustment: Adjustment, rounding: Rounding) -> Result<(), String> {
    match adjustment {
        Adjustment::Percent { percent_milli } if percent_milli.abs() > MAX_PERCENT_MILLI => {
            return Err("PRICE_BAD_CHANGE: a percentage change must be between -1,000% and 1,000%".into())
        }
        Adjustment::Amount { amount_cents } if amount_cents.abs() > MAX_AMOUNT_CENTS => {
            return Err("PRICE_BAD_CHANGE: an amount change must be under 1,000,000.00".into())
        }
        _ => {}
    }
    match rounding {
        Rounding::Nearest { step_cents } | Rounding::Up { step_cents } | Rounding::Down { step_cents } if !(1..=100_000).contains(&step_cents) => {
            Err("PRICE_BAD_ROUNDING: round to a step between 0.01 and 1,000.00".into())
        }
        _ => Ok(()),
    }
}

/// Rounds `cents` (zero or more) as `rounding` says.
pub fn round_price(cents: i64, rounding: Rounding) -> i64 {
    match rounding {
        Rounding::None => cents,
        Rounding::Nearest { step_cents: s } => (cents + s / 2) / s * s,
        Rounding::Up { step_cents: s } => (cents + s - 1) / s * s,
        Rounding::Down { step_cents: s } => cents / s * s,
        Rounding::EndIn99 => cents / 100 * 100 + 99,
    }
}

/// The new price from `base`, or `None` if it would fall below zero or out of range.
pub fn new_price(base: i64, adjustment: Adjustment, rounding: Rounding) -> Option<i64> {
    let changed = match adjustment {
        Adjustment::Percent { percent_milli } => base.checked_add(mul_div_round(base, percent_milli, PERCENT_SCALE)?)?,
        Adjustment::Amount { amount_cents } => base.checked_add(amount_cents)?,
    };
    if changed < 0 {
        return None;
    }
    Some(round_price(changed, rounding)).filter(|p| *p <= MAX_LINE_CENTS)
}

/// A product or service as Change Item Prices lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PriceItem {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    /// Stock is tracked (an inventory product); otherwise a service or non-inventory product.
    pub is_inventory: bool,
    pub rate_cents: i64,
    /// Moving average cost while stock is on hand, else the last purchase cost; `None` if unknown.
    pub unit_cost_cents: Option<i64>,
}

/// Unit cost: the average (value ÷ quantity, rounded half up) while stock is on hand.
const ITEMS_SELECT: &str = "
    SELECT p.id, p.name, p.description, p.is_active, i.id IS NOT NULL AS is_inventory, p.rate_cents,
           CASE WHEN i.qty_on_hand_e4 > 0 THEN (i.value_cents * 10000 * 2 + i.qty_on_hand_e4) / (i.qty_on_hand_e4 * 2)
                ELSE i.last_unit_cost_cents END AS unit_cost_cents
    FROM products_services p LEFT JOIN inventory_items i ON i.product_id = p.id
    ORDER BY p.name COLLATE NOCASE";

pub async fn list_items(pool: &SqlitePool) -> Result<Vec<PriceItem>, String> {
    sqlx::query_as(ITEMS_SELECT)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// One item's proposed price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceChange {
    pub product_id: i64,
    pub name: String,
    pub current_cents: i64,
    /// `None` when the item is left as it is; `reason` then says why.
    pub new_cents: Option<i64>,
    pub reason: Option<String>,
}

fn plan(items: &[PriceItem], input: &PriceChangeInput) -> Result<Vec<PriceChange>, String> {
    validate(input.adjustment, input.rounding)?;
    let mut changes = Vec::with_capacity(input.product_ids.len());
    for id in &input.product_ids {
        let item = items.iter().find(|i| i.id == *id).ok_or_else(|| format!("PRICE_UNKNOWN_ITEM: product {id} does not exist"))?;
        if changes.iter().any(|c: &PriceChange| c.product_id == *id) {
            continue;
        }
        let base = match input.basis {
            Basis::CurrentPrice => Some(item.rate_cents),
            Basis::Cost => item.unit_cost_cents,
        };
        let (new_cents, reason) = match base.map(|b| new_price(b, input.adjustment, input.rounding)) {
            None => (None, Some("No cost: stock isn't tracked or hasn't been bought".to_string())),
            Some(None) => (None, Some("The new price would be below zero".to_string())),
            Some(Some(p)) if p == item.rate_cents => (None, Some("No change".to_string())),
            Some(Some(p)) => (Some(p), None),
        };
        changes.push(PriceChange { product_id: item.id, name: item.name.clone(), current_cents: item.rate_cents, new_cents, reason });
    }
    Ok(changes)
}

/// What applying `input` would do, item by item.
pub async fn preview(pool: &SqlitePool, input: &PriceChangeInput) -> Result<Vec<PriceChange>, String> {
    plan(&list_items(pool).await?, input)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPrices {
    pub changed: Vec<PriceChange>,
    /// The PRICE_CHANGE record in the ledger audit trail.
    pub audit_record_id: i64,
}

/// Sets the new prices and records them in the audit trail, all or nothing.
pub async fn apply(pool: &SqlitePool, input: &PriceChangeInput) -> Result<AppliedPrices, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let items: Vec<PriceItem> = sqlx::query_as(ITEMS_SELECT).fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let changed: Vec<PriceChange> = plan(&items, input)?.into_iter().filter(|c| c.new_cents.is_some()).collect();
        if changed.is_empty() {
            return Err("PRICE_NOTHING_TO_CHANGE: none of the selected prices would change".to_string());
        }
        for c in &changed {
            sqlx::query("UPDATE products_services SET rate_cents = ?1 WHERE id = ?2")
                .bind(c.new_cents)
                .bind(c.product_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        let payload = serde_json::json!({
            "event": "CHANGE_ITEM_PRICES",
            "basis": input.basis,
            "adjustment": input.adjustment,
            "rounding": input.rounding,
            "items": changed.iter().map(|c| serde_json::json!({
                "productId": c.product_id, "name": c.name, "beforeCents": c.current_cents, "afterCents": c.new_cents,
            })).collect::<Vec<_>>(),
        });
        let audit_record_id: i64 = sqlx::query_scalar(
            "INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('PRICE_CHANGE', ?1, ?2) RETURNING id",
        )
        .bind(changed.len() as i64)
        .bind(payload.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        Ok(AppliedPrices { changed, audit_record_id })
    }
    .await;
    match result {
        Ok(applied) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(applied)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// An earlier use of Change Item Prices, from the audit trail.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceChangeRecord {
    pub audit_record_id: i64,
    pub timestamp: String,
    pub actor: Option<String>,
    /// The recorded event: basis, adjustment, rounding and items (old and new prices).
    pub details: serde_json::Value,
}

pub async fn history(pool: &SqlitePool, limit: i64) -> Result<Vec<PriceChangeRecord>, String> {
    let rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        // Price level changes are PRICE_CHANGE events too (`price_levels.rs`); they have their own history.
        "SELECT id, timestamp, actor, payload_snapshot FROM ledger_audit_log
         WHERE action_type = 'PRICE_CHANGE' AND COALESCE(json_extract(payload_snapshot, '$.event'), '') NOT LIKE 'PRICE_LEVEL_%'
         ORDER BY id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(audit_record_id, timestamp, actor, payload)| PriceChangeRecord {
            audit_record_id,
            timestamp,
            actor,
            details: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
        })
        .collect())
}

#[cfg(test)]
mod tests;
