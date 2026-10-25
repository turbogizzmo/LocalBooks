//! Inventory at moving average cost.
//!
//! A tracked product (`inventory_items`) holds a quantity and a total value. Stock comes in on bill
//! lines (debiting the item's Inventory Asset account) or by adjustment, and goes out on invoices and
//! sales receipts, which add Cost of Goods Sold lines to the sale's own journal entry:
//! debit COGS, credit Inventory Asset, at round(value × qty sold / qty on hand). Every movement is
//! appended to `stock_ledger_entries` with the journal entry that carries its money side; voiding that
//! entry reverses the movement, so stock and the Inventory Asset account always agree.
//!
//! Stock may be sold before its purchase is recorded. Units sold beyond what is on hand are costed
//! at the item's last purchase cost per unit, leaving a negative quantity with a negative value.
//! When stock arrives while below zero, the difference between the actual and provisional cost of
//! the units already sold is moved to COGS in the purchase's own entry (a COST_TRUE_UP movement).
//! No cost is ever worked out by dividing by a quantity of zero or less.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{self, db_err, mul_div_round, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};

/// A tracked product as stored.
struct Item {
    id: i64,
    name: String,
    asset_account_id: i64,
    cogs_account_id: i64,
    qty_e4: i64,
    value_cents: i64,
    last_unit_cost_cents: Option<i64>,
}

async fn item_for_product(tx: &mut Transaction<'_, Sqlite>, product_id: i64) -> Result<Option<Item>, String> {
    let row: Option<(i64, String, i64, i64, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT i.id, p.name, i.asset_account_id, i.cogs_account_id, i.qty_on_hand_e4, i.value_cents, i.last_unit_cost_cents
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE i.product_id = ?1",
    )
    .bind(product_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(row.map(|(id, name, asset_account_id, cogs_account_id, qty_e4, value_cents, last_unit_cost_cents)| Item {
        id,
        name,
        asset_account_id,
        cogs_account_id,
        qty_e4,
        value_cents,
        last_unit_cost_cents,
    }))
}

/// 15_000 → "1.5", -20_000 → "-2".
fn format_qty(qty_e4: i64) -> String {
    let sign = if qty_e4 < 0 { "-" } else { "" };
    let abs = qty_e4.unsigned_abs();
    let frac = format!("{:04}", abs % 10_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { format!("{sign}{}", abs / 10_000) } else { format!("{sign}{}.{frac}", abs / 10_000) }
}

/// Appends a movement continuing from the item's current quantity and value.
async fn record_movement(
    tx: &mut Transaction<'_, Sqlite>,
    item_id: i64,
    date: NaiveDate,
    kind: &str,
    qty_change_e4: i64,
    value_change_cents: i64,
    journal_entry_id: i64,
    reverses_id: Option<i64>,
) -> Result<i64, String> {
    let (name, qty, value): (String, i64, i64) = sqlx::query_as(
        "SELECT p.name, i.qty_on_hand_e4, i.value_cents FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE i.id = ?1",
    )
    .bind(item_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    let qty_after = qty.checked_add(qty_change_e4).ok_or("STOCK_OVERFLOW: quantity out of range")?;
    let value_after = value.checked_add(value_change_cents).ok_or("STOCK_OVERFLOW: value out of range")?;
    // Quantity and value must agree in sign (also enforced by the table's CHECK constraint).
    let consistent = (qty_after > 0 && value_after >= 0) || (qty_after == 0 && value_after == 0) || (qty_after < 0 && value_after <= 0);
    if !consistent {
        return Err(format!(
            "STOCK_VALUE: this would leave {name} with {} on hand valued at {}; reverse later sales or adjustments first",
            format_qty(qty_after),
            crate::ledger::format_cents(value_after)
        ));
    }
    sqlx::query_scalar(
        "INSERT INTO stock_ledger_entries
            (item_id, movement_date, kind, qty_change_e4, value_change_cents, qty_after_e4, value_after_cents, journal_entry_id, reverses_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
    )
    .bind(item_id)
    .bind(date)
    .bind(kind)
    .bind(qty_change_e4)
    .bind(value_change_cents)
    .bind(qty_after)
    .bind(value_after)
    .bind(journal_entry_id)
    .bind(reverses_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)
}

/// Cost of `qty_e4` units taken out of what is on hand, at moving average: the whole remaining
/// value when they are the last units. Refuses more than is on hand (used for write-offs).
fn average_cost(item: &Item, qty_e4: i64) -> Result<i64, String> {
    if qty_e4 <= 0 || qty_e4 > item.qty_e4 {
        return Err(format!("STOCK_SHORT: only {} of {} on hand", format_qty(item.qty_e4.max(0)), item.name));
    }
    if qty_e4 == item.qty_e4 {
        return Ok(item.value_cents);
    }
    // Here 0 < qty_e4 < item.qty_e4, so the divisor is positive.
    mul_div_round(item.value_cents, qty_e4, item.qty_e4).ok_or_else(|| "STOCK_OVERFLOW: cost out of range".to_string())
}

/// Cost of selling `qty_e4` units. Units covered by stock on hand cost moving average; any units
/// beyond that (stock goes below zero) cost the last purchase cost per unit, or nothing if the item
/// has never been bought, to be trued up when stock arrives.
fn sale_cost(item: &Item, qty_e4: i64) -> Result<i64, String> {
    if qty_e4 <= item.qty_e4 {
        return average_cost(item, qty_e4);
    }
    // Guard: no division by a quantity of zero or less. Units on hand (if any) take their whole value.
    let covered = item.qty_e4.max(0);
    let covered_cost = if covered > 0 { item.value_cents } else { 0 };
    let short = qty_e4 - covered;
    let unit = item.last_unit_cost_cents.unwrap_or(0);
    let short_cost = mul_div_round(unit, short, crate::ledger::QUANTITY_SCALE).ok_or("STOCK_OVERFLOW: cost out of range")?;
    covered_cost.checked_add(short_cost).ok_or_else(|| "STOCK_OVERFLOW: cost out of range".to_string())
}

/// When stock arrives while below zero: the actual cost of the units already sold minus the
/// provisional cost they were sold at. Positive means COGS was understated. Zero when stock is not
/// below zero. Divides only by the positive purchase quantity and positive shortfall.
fn true_up(qty_before: i64, value_before: i64, qty_e4: i64, cost_cents: i64) -> Result<i64, String> {
    if qty_before >= 0 || qty_e4 <= 0 {
        return Ok(0);
    }
    let shortfall = -qty_before;
    let covered = shortfall.min(qty_e4);
    let actual = if covered == qty_e4 { Some(cost_cents) } else { mul_div_round(cost_cents, covered, qty_e4) };
    let provisional = if covered == shortfall { Some(-value_before) } else { mul_div_round(-value_before, covered, shortfall) };
    match (actual, provisional) {
        (Some(a), Some(p)) => Ok(a - p),
        _ => Err("STOCK_OVERFLOW: cost out of range".into()),
    }
}

/// Journal lines moving `variance` between Cost of Goods Sold and Inventory Asset.
fn true_up_lines(item: &Item, variance: i64) -> Vec<JournalLine> {
    match variance.signum() {
        1 => vec![JournalLine::debit(item.cogs_account_id, variance), JournalLine::credit(item.asset_account_id, variance)],
        -1 => vec![JournalLine::debit(item.asset_account_id, -variance), JournalLine::credit(item.cogs_account_id, -variance)],
        _ => Vec::new(),
    }
}

/// Cost of goods sold for a sale, worked out before its journal entry is posted.
pub(crate) struct SaleCost {
    /// Extra journal lines: debit each COGS account, credit each Inventory Asset account.
    pub lines: Vec<JournalLine>,
    /// (item, quantity, cost) to record once the journal entry exists.
    movements: Vec<(i64, i64, i64)>,
}

/// Works out the cost of the tracked products on a sale's lines (`(product_id, quantity_e4, class_id)`).
/// Several lines for the same product are costed together, in one movement; when they carry
/// different classes, the Cost of Goods Sold is split between those classes by quantity.
pub(crate) async fn cost_of_sale(tx: &mut Transaction<'_, Sqlite>, lines: &[(Option<i64>, i64, Option<i64>)]) -> Result<SaleCost, String> {
    // Per product: total quantity, and quantity per class in first-appearance order.
    let mut per_product: BTreeMap<i64, (i64, Vec<(Option<i64>, i64)>)> = BTreeMap::new();
    for (product, qty, class) in lines {
        if let Some(p) = product {
            let (total, by_class) = per_product.entry(*p).or_default();
            *total += qty;
            match by_class.iter_mut().find(|(c, _)| c == class) {
                Some((_, q)) => *q += qty,
                None => by_class.push((*class, *qty)),
            }
        }
    }
    let mut debits: BTreeMap<(i64, Option<i64>), i64> = BTreeMap::new();
    let mut credits: BTreeMap<i64, i64> = BTreeMap::new();
    let mut movements = Vec::new();
    for (product, (qty, by_class)) in per_product {
        let Some(item) = item_for_product(tx, product).await? else { continue };
        let cost = sale_cost(&item, qty)?;
        if cost > 0 {
            for (class, share) in split_by_quantity(cost, qty, &by_class)? {
                *debits.entry((item.cogs_account_id, class)).or_insert(0) += share;
            }
            *credits.entry(item.asset_account_id).or_insert(0) += cost;
        }
        movements.push((item.id, qty, cost));
    }
    let mut out: Vec<JournalLine> =
        debits.into_iter().filter(|(_, c)| *c != 0).map(|((a, class), c)| JournalLine::debit(a, c).with_class(class)).collect();
    out.extend(credits.into_iter().map(|(a, c)| JournalLine::credit(a, c)));
    Ok(SaleCost { lines: out, movements })
}

/// Splits `cost` across classes in proportion to their quantities. Each share is the difference of
/// rounded running totals, so the shares never go negative and always add up to `cost` exactly.
fn split_by_quantity(cost: i64, total_qty: i64, by_class: &[(Option<i64>, i64)]) -> Result<Vec<(Option<i64>, i64)>, String> {
    let mut shares = Vec::with_capacity(by_class.len());
    let (mut running_qty, mut assigned) = (0, 0);
    for (class, qty) in by_class {
        running_qty += qty;
        let upto = mul_div_round(cost, running_qty, total_qty).ok_or("STOCK_OVERFLOW: cost out of range")?;
        shares.push((*class, upto - assigned));
        assigned = upto;
    }
    Ok(shares)
}

impl SaleCost {
    pub fn is_empty(&self) -> bool {
        self.movements.is_empty()
    }

    /// Records the stock leaving, against the sale's posted journal entry.
    pub async fn record(&self, tx: &mut Transaction<'_, Sqlite>, date: NaiveDate, journal_entry_id: i64) -> Result<(), String> {
        for (item, qty, cost) in &self.movements {
            record_movement(tx, *item, date, "SALE", -qty, -cost, journal_entry_id, None).await?;
        }
        Ok(())
    }
}

/// A purchase line for a tracked product: the line must post to that item's Inventory Asset account.
/// Returns the item id to record once the bill's journal entry exists.
pub(crate) async fn check_purchase_line(tx: &mut Transaction<'_, Sqlite>, product_id: i64, account_id: i64, quantity_e4: Option<i64>, n: usize) -> Result<Option<i64>, String> {
    let Some(item) = item_for_product(tx, product_id).await? else { return Ok(None) };
    if account_id != item.asset_account_id {
        return Err(format!("STOCK_PURCHASE_ACCOUNT: line {n} buys {}, so it must post to its inventory account", item.name));
    }
    if quantity_e4.is_none() {
        return Err(format!("STOCK_PURCHASE_QTY: line {n} buys {}; enter a quantity and rate", item.name));
    }
    Ok(Some(item.id))
}

async fn item_by_id(tx: &mut Transaction<'_, Sqlite>, item_id: i64) -> Result<Item, String> {
    let product: i64 = sqlx::query_scalar("SELECT product_id FROM inventory_items WHERE id = ?1").bind(item_id).fetch_one(&mut **tx).await.map_err(db_err)?;
    item_for_product(tx, product).await?.ok_or_else(|| "STOCK_NOT_FOUND: that item is not tracked".into())
}

/// Stock arriving on a bill, worked out before the bill's journal entry is posted.
pub(crate) struct Receipt {
    /// True-up lines for stock that arrives while below zero (added to the bill's entry).
    pub lines: Vec<JournalLine>,
    /// (item, quantity, cost, true-up) per bill line, in order.
    plan: Vec<(i64, i64, i64, i64)>,
}

/// Plans stock received on a bill's lines (`(item, quantity, cost)`), applying each in turn.
pub(crate) async fn plan_receipt(tx: &mut Transaction<'_, Sqlite>, received: &[(i64, i64, i64)]) -> Result<Receipt, String> {
    let mut state: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    let mut lines = Vec::new();
    let mut plan = Vec::new();
    for &(item_id, qty, cost) in received {
        let item = item_by_id(tx, item_id).await?;
        let (q, v) = *state.entry(item_id).or_insert((item.qty_e4, item.value_cents));
        let variance = true_up(q, v, qty, cost)?;
        lines.extend(true_up_lines(&item, variance));
        state.insert(item_id, (q + qty, v + cost - variance));
        plan.push((item_id, qty, cost, variance));
    }
    Ok(Receipt { lines, plan })
}

impl Receipt {
    /// Records the stock and any true-up against the bill's posted entry, and the last unit cost.
    pub async fn record(&self, tx: &mut Transaction<'_, Sqlite>, date: NaiveDate, journal_entry_id: i64) -> Result<(), String> {
        for &(item, qty, cost, variance) in &self.plan {
            // The true-up comes first: recorded alone, a purchase into negative stock could leave a
            // moment where quantity is still below zero but value is above it.
            if variance != 0 {
                record_movement(tx, item, date, "COST_TRUE_UP", 0, -variance, journal_entry_id, None).await?;
            }
            record_movement(tx, item, date, "PURCHASE", qty, cost, journal_entry_id, None).await?;
            remember_unit_cost(tx, item, qty, cost).await?;
        }
        Ok(())
    }
}

/// Keeps the latest cost per unit for costing sales beyond what is on hand.
async fn remember_unit_cost(tx: &mut Transaction<'_, Sqlite>, item_id: i64, qty_e4: i64, cost_cents: i64) -> Result<(), String> {
    let unit = mul_div_round(cost_cents, crate::ledger::QUANTITY_SCALE, qty_e4).ok_or("STOCK_OVERFLOW: cost out of range")?;
    sqlx::query("UPDATE inventory_items SET last_unit_cost_cents = ?1 WHERE id = ?2")
        .bind(unit)
        .bind(item_id)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// Reverses every stock movement carried by a journal entry being voided.
pub(crate) async fn reverse_for_entry(tx: &mut Transaction<'_, Sqlite>, journal_entry_id: i64, reversal_entry_id: i64, date: NaiveDate) -> Result<(), String> {
    let movements: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, item_id, qty_change_e4, value_change_cents FROM stock_ledger_entries
         WHERE journal_entry_id = ?1 AND kind <> 'VOID' ORDER BY id DESC",
    )
    .bind(journal_entry_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    for (id, item, qty, value) in movements {
        record_movement(tx, item, date, "VOID", -qty, -value, reversal_entry_id, Some(id)).await?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Items and adjustments
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItemView {
    pub id: i64,
    pub product_id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub sales_price_cents: i64,
    pub asset_account_id: i64,
    pub cogs_account_id: i64,
    pub qty_on_hand_e4: i64,
    pub value_cents: i64,
    /// Value per unit, rounded to the cent (for display; costing uses the exact total value).
    pub average_cost_cents: Option<i64>,
}

pub async fn list_items(pool: &SqlitePool) -> Result<Vec<InventoryItemView>, String> {
    sqlx::query_as(
        "SELECT i.id, i.product_id, p.name, i.sku, p.rate_cents AS sales_price_cents, i.asset_account_id, i.cogs_account_id,
                i.qty_on_hand_e4, i.value_cents,
                CASE WHEN i.qty_on_hand_e4 > 0 THEN (i.value_cents * 10000 * 2 + i.qty_on_hand_e4) / (i.qty_on_hand_e4 * 2) END AS average_cost_cents
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id
         ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInput {
    pub product_id: i64,
    #[serde(default)]
    pub sku: Option<String>,
    /// Defaults to "Inventory Asset" and "Cost of Goods Sold".
    #[serde(default)]
    pub asset_account_id: Option<i64>,
    #[serde(default)]
    pub cogs_account_id: Option<i64>,
}

/// Starts tracking quantity and cost for an existing product (starting at zero on hand).
pub async fn track_product(pool: &SqlitePool, input: &TrackInput) -> Result<InventoryItemView, String> {
    let default = |sql: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sql).fetch_optional(pool).await.map_err(db_err)
    };
    let asset = match input.asset_account_id {
        Some(a) => a,
        None => default("SELECT id FROM chart_of_accounts WHERE name = 'Inventory Asset' AND account_type = 'OTHER_CURRENT_ASSET' AND is_active = 1 ORDER BY id LIMIT 1")
            .await?
            .ok_or("STOCK_NO_ASSET_ACCOUNT: create an Other Current Asset account for inventory first")?,
    };
    let cogs = match input.cogs_account_id {
        Some(a) => a,
        None => default("SELECT id FROM chart_of_accounts WHERE account_type = 'COST_OF_GOODS_SOLD' AND is_active = 1 ORDER BY COALESCE(account_number, ''), id LIMIT 1")
            .await?
            .ok_or("STOCK_NO_COGS_ACCOUNT: create a Cost of Goods Sold account first")?,
    };
    let sku = input.sku.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO inventory_items (product_id, sku, asset_account_id, cogs_account_id) VALUES (?1, ?2, ?3, ?4) RETURNING id",
    )
    .bind(input.product_id)
    .bind(sku)
    .bind(asset)
    .bind(cogs)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") && msg.contains("product_id") {
            "STOCK_ALREADY_TRACKED: that product is already tracked".into()
        } else if msg.contains("UNIQUE") {
            "STOCK_SKU_TAKEN: another item already uses that SKU".into()
        } else {
            msg
        }
    })?;
    list_items(pool).await?.into_iter().find(|i| i.id == id).ok_or_else(|| "STOCK_NOT_FOUND".into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustInput {
    pub item_id: i64,
    pub date: NaiveDate,
    /// Signed quantity change (scaled by 10,000).
    pub qty_change_e4: i64,
    /// For an increase: the total cost of the units added. Ignored for a decrease, which is valued
    /// at moving average cost.
    #[serde(default)]
    pub total_cost_cents: Option<i64>,
    /// The other side: e.g. Opening Balance Equity for opening stock, or a shrinkage/COGS account
    /// for units lost or damaged.
    pub offset_account_id: i64,
    #[serde(default)]
    pub memo: Option<String>,
}

/// Opening stock, a stock count or a write-off: a journal entry between the item's Inventory Asset
/// account and `offset_account_id`, and the matching stock movement.
pub async fn adjust(pool: &SqlitePool, input: &AdjustInput) -> Result<InventoryItemView, String> {
    if input.qty_change_e4 == 0 {
        return Err("STOCK_ADJUST_ZERO: enter the quantity to add or remove".into());
    }
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let product: i64 = sqlx::query_scalar("SELECT product_id FROM inventory_items WHERE id = ?1")
            .bind(input.item_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?
            .ok_or("STOCK_NOT_FOUND: that item is not tracked")?;
        let item = item_for_product(&mut tx, product).await?.expect("tracked");
        if input.offset_account_id == item.asset_account_id {
            return Err("STOCK_ADJUST_OFFSET: choose an account other than the inventory account".to_string());
        }
        let value = if input.qty_change_e4 > 0 {
            let cost = input.total_cost_cents.ok_or("STOCK_ADJUST_COST: enter the total cost of the units added")?;
            if !(0..=MAX_LINE_CENTS).contains(&cost) {
                return Err("STOCK_ADJUST_COST: the cost must be zero or more".to_string());
            }
            cost
        } else {
            -average_cost(&item, -input.qty_change_e4)?
        };
        let memo = input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()).unwrap_or("Inventory adjustment");
        let description = format!("{memo}: {} {} {}", item.name, if input.qty_change_e4 > 0 { "+" } else { "-" }, format_qty(input.qty_change_e4.abs()));
        // Stock added while below zero trues up the cost of units already sold.
        let variance = if value > 0 { true_up(item.qty_e4, item.value_cents, input.qty_change_e4, value)? } else { 0 };
        let entry_id = if value == 0 {
            None
        } else {
            let mut lines = if value > 0 {
                vec![JournalLine::debit(item.asset_account_id, value), JournalLine::credit(input.offset_account_id, value)]
            } else {
                vec![JournalLine::debit(input.offset_account_id, -value), JournalLine::credit(item.asset_account_id, -value)]
            };
            lines.extend(true_up_lines(&item, variance));
            let entry = JournalEntry { id: None, date: input.date, description: description.clone(), lines, currency: None };
            Some(ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?)
        };
        let Some(entry_id) = entry_id else {
            return Err("STOCK_ADJUST_FREE: adding stock at no cost needs a cost of at least 0.01, so the books record it".to_string());
        };
        if variance != 0 {
            record_movement(&mut tx, item.id, input.date, "COST_TRUE_UP", 0, -variance, entry_id, None).await?;
        }
        record_movement(&mut tx, item.id, input.date, "ADJUSTMENT", input.qty_change_e4, value, entry_id, None).await?;
        if value > 0 {
            remember_unit_cost(&mut tx, item.id, input.qty_change_e4, value).await?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Err(err) => return Err(ledger::rollback(tx, err).await),
    }
    list_items(pool).await?.into_iter().find(|i| i.id == input.item_id).ok_or_else(|| "STOCK_NOT_FOUND".into())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementView {
    pub id: i64,
    pub movement_date: String,
    pub kind: String,
    pub qty_change_e4: i64,
    pub value_change_cents: i64,
    pub qty_after_e4: i64,
    pub value_after_cents: i64,
    pub entry_number: i64,
    pub memo: Option<String>,
}

pub async fn movements(pool: &SqlitePool, item_id: i64) -> Result<Vec<StockMovementView>, String> {
    sqlx::query_as(
        "SELECT s.id, s.movement_date, s.kind, s.qty_change_e4, s.value_change_cents, s.qty_after_e4, s.value_after_cents,
                e.entry_number, e.memo
         FROM stock_ledger_entries s JOIN journal_entries e ON e.id = s.journal_entry_id
         WHERE s.item_id = ?1 ORDER BY s.id DESC",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
