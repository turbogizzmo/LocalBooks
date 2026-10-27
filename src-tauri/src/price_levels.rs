//! Price levels: named sets of prices, QuickBooks-style, that a customer can be given by default.
//!
//! - **Fixed %** levels raise or lower every item's standard price by one percentage, then round.
//! - **Per item** levels list items with their own price: a fixed price, or the standard price or
//!   the unit cost raised or lowered by a percentage (then rounded). Items not listed sell at their
//!   standard price.
//!
//! All arithmetic is in integer cents, with Change Item Prices' formula and rounding
//! ([`crate::item_prices::new_price`]): `base + round_half_up(base × percent_milli / 100,000)`,
//! where `percent_milli` is thousandths of a percent (−10% → −10,000), then the level's rounding
//! (to a step, nearest/up/down, or up to .99). Percentages run from −100% to +1,000%, so a price is
//! never negative. Fixed per-item prices are used as entered and never rounded.
//!
//! A price that can't be worked out falls back to the standard price: a cost-based price for an
//! item with no known cost (stock not tracked or never bought), or a level that is inactive.
//!
//! On sales forms the line rate fills in from the customer's default level, and each line can pick
//! another level; a typed rate always wins. Documents keep the rate they were written with.
//!
//! Every change to a level (created, changed, made inactive, deleted, imported) writes a
//! PRICE_CHANGE event in the ledger audit trail with the level before and after, in the same
//! transaction as the change.

use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::item_prices::{self, Adjustment, PriceItem, Rounding};
use crate::ledger::{db_err, rollback, MAX_LINE_CENTS};

/// −100% (free) to +1,000%, in thousandths of a percent.
pub const MIN_PERCENT_MILLI: i64 = -100_000;
pub const MAX_PERCENT_MILLI: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LevelKind {
    FixedPercent,
    PerItem,
}

impl LevelKind {
    fn code(self) -> &'static str {
        match self {
            LevelKind::FixedPercent => "FIXED_PERCENT",
            LevelKind::PerItem => "PER_ITEM",
        }
    }
    fn from_code(code: &str) -> Self {
        if code == "PER_ITEM" { LevelKind::PerItem } else { LevelKind::FixedPercent }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemBasis {
    /// `price_cents` as entered.
    Fixed,
    /// The item's standard (sales) price, changed by `percent_milli`.
    Standard,
    /// The item's unit cost, changed by `percent_milli`.
    Cost,
}

impl ItemBasis {
    fn code(self) -> &'static str {
        match self {
            ItemBasis::Fixed => "FIXED",
            ItemBasis::Standard => "STANDARD",
            ItemBasis::Cost => "COST",
        }
    }
    fn from_code(code: &str) -> Self {
        match code {
            "STANDARD" => ItemBasis::Standard,
            "COST" => ItemBasis::Cost,
            _ => ItemBasis::Fixed,
        }
    }
}

/// One item's price on a per-item level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelItem {
    pub product_id: i64,
    pub basis: ItemBasis,
    #[serde(default)]
    pub price_cents: Option<i64>,
    #[serde(default)]
    pub percent_milli: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceLevelInput {
    pub name: String,
    pub kind: LevelKind,
    /// Fixed % levels: the change, in thousandths of a percent.
    #[serde(default)]
    pub percent_milli: i64,
    pub rounding: Rounding,
    /// Per item levels: the items and their prices.
    #[serde(default)]
    pub items: Vec<LevelItem>,
    #[serde(default = "yes")]
    pub is_active: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceLevel {
    pub id: i64,
    pub name: String,
    pub kind: LevelKind,
    pub percent_milli: i64,
    pub rounding: Rounding,
    pub is_active: bool,
    pub items: Vec<LevelItem>,
    /// Customers who have this level as their default.
    pub customer_count: i64,
}

impl PriceLevel {
    fn input(&self) -> PriceLevelInput {
        PriceLevelInput {
            name: self.name.clone(),
            kind: self.kind,
            percent_milli: self.percent_milli,
            rounding: self.rounding,
            items: self.items.clone(),
            is_active: self.is_active,
        }
    }
}

/// Where a resolved price came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceSource {
    /// A fixed % level's percentage of the standard price.
    LevelPercent,
    /// A per-item fixed price.
    Fixed,
    /// A per-item percentage of the standard price.
    StandardPercent,
    /// A per-item percentage of the unit cost.
    CostPercent,
    /// The standard price: the item isn't on the per-item level.
    Standard,
    /// The standard price, because the item has no known cost.
    NoCost,
}

fn change(base: i64, percent_milli: i64, rounding: Rounding) -> Option<i64> {
    item_prices::new_price(base, Adjustment::Percent { percent_milli }, rounding)
}

/// The price of `item` at `level` (whether or not the level is active), in cents.
pub fn resolve(level: &PriceLevel, item: &PriceItem) -> (i64, PriceSource) {
    let standard = item.rate_cents;
    match level.kind {
        LevelKind::FixedPercent => match change(standard, level.percent_milli, level.rounding) {
            Some(p) => (p, PriceSource::LevelPercent),
            None => (standard, PriceSource::Standard),
        },
        LevelKind::PerItem => {
            let Some(row) = level.items.iter().find(|i| i.product_id == item.id) else {
                return (standard, PriceSource::Standard);
            };
            let pct = row.percent_milli.unwrap_or(0);
            match row.basis {
                ItemBasis::Fixed => (row.price_cents.unwrap_or(standard), PriceSource::Fixed),
                ItemBasis::Standard => change(standard, pct, level.rounding).map_or((standard, PriceSource::Standard), |p| (p, PriceSource::StandardPercent)),
                ItemBasis::Cost => match item.unit_cost_cents.and_then(|c| change(c, pct, level.rounding)) {
                    Some(p) => (p, PriceSource::CostPercent),
                    None => (standard, PriceSource::NoCost),
                },
            }
        }
    }
}

/// A percentage as written in a file ("-10%", "12.5", "−7.25 %") in thousandths of a percent,
/// rounded half away from zero; `None` if it isn't one.
pub fn parse_percent_milli(text: &str) -> Option<i64> {
    let t = text.trim().trim_end_matches('%').trim().replace('−', "-");
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if (whole.is_empty() && frac.is_empty()) || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) || whole.len() > 7 {
        return None;
    }
    let frac: String = frac.chars().chain(std::iter::repeat('0')).take(4).collect();
    let ten_thousandths = whole.parse::<i64>().unwrap_or(0) * 10_000 + frac.parse::<i64>().ok()?;
    let milli = (ten_thousandths + 5) / 10;
    Some(if negative { -milli } else { milli })
}

/// Checks a level's own values (not whether its items exist).
pub fn validate(input: &PriceLevelInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("PRICE_LEVEL_NO_NAME: enter a name for the price level".into());
    }
    if input.name.trim().chars().count() > 100 {
        return Err("PRICE_LEVEL_LONG_NAME: a price level name is at most 100 characters".into());
    }
    let pct_ok = |p: i64| (MIN_PERCENT_MILLI..=MAX_PERCENT_MILLI).contains(&p);
    if !pct_ok(input.percent_milli) {
        return Err("PRICE_LEVEL_BAD_PERCENT: a percentage must be between -100% and 1,000%".into());
    }
    item_prices::validate(Adjustment::Percent { percent_milli: input.percent_milli }, input.rounding).map_err(|e| e.replacen("PRICE_", "PRICE_LEVEL_", 1))?;
    if input.kind == LevelKind::FixedPercent && !input.items.is_empty() {
        return Err("PRICE_LEVEL_FIXED_HAS_ITEMS: a fixed % level applies to every item and lists none".into());
    }
    for (n, item) in input.items.iter().enumerate() {
        if input.items[..n].iter().any(|i| i.product_id == item.product_id) {
            return Err(format!("PRICE_LEVEL_DUPLICATE_ITEM: product {} is listed twice", item.product_id));
        }
        match item.basis {
            ItemBasis::Fixed => match item.price_cents {
                Some(p) if (0..=MAX_LINE_CENTS).contains(&p) && item.percent_milli.is_none() => {}
                _ => return Err("PRICE_LEVEL_BAD_ITEM: a fixed item price must be zero or more".into()),
            },
            ItemBasis::Standard | ItemBasis::Cost => match item.percent_milli {
                Some(p) if pct_ok(p) && item.price_cents.is_none() => {}
                _ => return Err("PRICE_LEVEL_BAD_ITEM: an item's percentage must be between -100% and 1,000%".into()),
            },
        }
    }
    Ok(())
}

type LevelRow = (i64, String, String, i64, String, bool, i64);

async fn read_levels(conn: &mut SqliteConnection, id: Option<i64>) -> Result<Vec<PriceLevel>, String> {
    let rows: Vec<LevelRow> = sqlx::query_as(
        "SELECT l.id, l.name, l.kind, l.percent_milli, l.rounding, l.is_active,
                (SELECT COUNT(*) FROM customers c WHERE c.price_level_id = l.id)
         FROM price_levels l WHERE ?1 IS NULL OR l.id = ?1 ORDER BY l.name COLLATE NOCASE",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let items: Vec<(i64, i64, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT i.price_level_id, i.product_id, i.basis, i.price_cents, i.percent_milli
         FROM price_level_items i JOIN products_services p ON p.id = i.product_id
         WHERE ?1 IS NULL OR i.price_level_id = ?1 ORDER BY p.name COLLATE NOCASE",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    rows.into_iter()
        .map(|(id, name, kind, percent_milli, rounding, is_active, customer_count)| {
            Ok(PriceLevel {
                id,
                name,
                kind: LevelKind::from_code(&kind),
                percent_milli,
                rounding: serde_json::from_str(&rounding).map_err(|e| format!("PRICE_LEVEL_BAD_ROUNDING: {e}"))?,
                is_active,
                items: items
                    .iter()
                    .filter(|i| i.0 == id)
                    .map(|i| LevelItem { product_id: i.1, basis: ItemBasis::from_code(&i.2), price_cents: i.3, percent_milli: i.4 })
                    .collect(),
                customer_count,
            })
        })
        .collect()
}

/// Every price level with its items, by name.
pub async fn list(pool: &SqlitePool) -> Result<Vec<PriceLevel>, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    read_levels(&mut conn, None).await
}

async fn one(conn: &mut SqliteConnection, id: i64) -> Result<PriceLevel, String> {
    read_levels(conn, Some(id)).await?.pop().ok_or_else(|| format!("PRICE_LEVEL_NOT_FOUND: price level {id} does not exist"))
}

/// The level as recorded in the audit trail: its settings and items, with item names.
async fn snapshot(conn: &mut SqliteConnection, level: &PriceLevel) -> Result<serde_json::Value, String> {
    let mut items = Vec::with_capacity(level.items.len());
    for i in &level.items {
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM products_services WHERE id = ?1").bind(i.product_id).fetch_optional(&mut *conn).await.map_err(db_err)?;
        items.push(serde_json::json!({
            "productId": i.product_id, "name": name, "basis": i.basis, "priceCents": i.price_cents, "percentMilli": i.percent_milli,
        }));
    }
    Ok(serde_json::json!({
        "name": level.name, "kind": level.kind, "percentMilli": level.percent_milli, "rounding": level.rounding,
        "isActive": level.is_active, "items": items,
    }))
}

async fn audit(conn: &mut SqliteConnection, level_id: i64, event: &str, before: Option<serde_json::Value>, after: Option<serde_json::Value>, extra: serde_json::Value) -> Result<i64, String> {
    let mut payload = serde_json::json!({ "event": event, "priceLevelId": level_id, "before": before, "after": after });
    if let (Some(p), serde_json::Value::Object(more)) = (payload.as_object_mut(), extra) {
        p.extend(more);
    }
    sqlx::query_scalar("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('PRICE_CHANGE', ?1, ?2) RETURNING id")
        .bind(level_id)
        .bind(payload.to_string())
        .fetch_one(&mut *conn)
        .await
        .map_err(db_err)
}

fn duplicate(name: &str) -> impl Fn(sqlx::Error) -> String + '_ {
    move |e| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("PRICE_LEVEL_DUPLICATE: there is already a price level named '{name}'") } else { msg }
    }
}

async fn write_items(conn: &mut SqliteConnection, id: i64, items: &[LevelItem]) -> Result<(), String> {
    sqlx::query("DELETE FROM price_level_items WHERE price_level_id = ?1").bind(id).execute(&mut *conn).await.map_err(db_err)?;
    for i in items {
        let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM products_services WHERE id = ?1").bind(i.product_id).fetch_optional(&mut *conn).await.map_err(db_err)?;
        if exists.is_none() {
            return Err(format!("PRICE_LEVEL_UNKNOWN_ITEM: product {} does not exist", i.product_id));
        }
        sqlx::query("INSERT INTO price_level_items (price_level_id, product_id, basis, price_cents, percent_milli) VALUES (?1, ?2, ?3, ?4, ?5)")
            .bind(id)
            .bind(i.product_id)
            .bind(i.basis.code())
            .bind(i.price_cents)
            .bind(i.percent_milli)
            .execute(&mut *conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

/// Creates a level and records it, inside the caller's transaction (also used by imports, which
/// pass `event` = "PRICE_LEVEL_IMPORTED").
pub async fn create_in(conn: &mut SqliteConnection, input: &PriceLevelInput, event: &str) -> Result<i64, String> {
    validate(input)?;
    let name = input.name.trim();
    let rounding = serde_json::to_string(&input.rounding).map_err(|e| e.to_string())?;
    let id: i64 = sqlx::query_scalar("INSERT INTO price_levels (name, kind, percent_milli, rounding, is_active) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id")
        .bind(name)
        .bind(input.kind.code())
        .bind(input.percent_milli)
        .bind(rounding)
        .bind(input.is_active)
        .fetch_one(&mut *conn)
        .await
        .map_err(duplicate(name))?;
    write_items(conn, id, &input.items).await?;
    let after = one(conn, id).await?;
    let after = snapshot(conn, &after).await?;
    audit(conn, id, event, None, Some(after), serde_json::json!({})).await?;
    Ok(id)
}

async fn update_in(conn: &mut SqliteConnection, id: i64, input: &PriceLevelInput) -> Result<(), String> {
    validate(input)?;
    let before = one(conn, id).await?;
    // Unchanged (items in any order): nothing to write or record.
    let by_product = |mut level: PriceLevelInput| {
        level.items.sort_by_key(|i| i.product_id);
        level
    };
    if by_product(before.input()) == by_product(PriceLevelInput { name: input.name.trim().to_string(), ..input.clone() }) {
        return Ok(());
    }
    let name = input.name.trim();
    let rounding = serde_json::to_string(&input.rounding).map_err(|e| e.to_string())?;
    sqlx::query("UPDATE price_levels SET name = ?1, kind = ?2, percent_milli = ?3, rounding = ?4, is_active = ?5 WHERE id = ?6")
        .bind(name)
        .bind(input.kind.code())
        .bind(input.percent_milli)
        .bind(rounding)
        .bind(input.is_active)
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(duplicate(name))?;
    write_items(conn, id, &input.items).await?;
    let after = one(conn, id).await?;
    let (before, after) = (snapshot(conn, &before).await?, snapshot(conn, &after).await?);
    audit(conn, id, "PRICE_LEVEL_CHANGED", Some(before), Some(after), serde_json::json!({})).await?;
    Ok(())
}

async fn in_transaction<T, F>(pool: &SqlitePool, work: F) -> Result<T, String>
where
    F: for<'c> FnOnce(&'c mut SqliteConnection) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + 'c>>,
{
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(v) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(v)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Creates a level (`id` = `None`) or changes one, and records it in the audit trail.
pub async fn save(pool: &SqlitePool, id: Option<i64>, input: PriceLevelInput) -> Result<PriceLevel, String> {
    in_transaction(pool, move |conn| {
        Box::pin(async move {
            let id = match id {
                Some(id) => {
                    update_in(conn, id, &input).await?;
                    id
                }
                None => create_in(conn, &input, "PRICE_LEVEL_CREATED").await?,
            };
            one(conn, id).await
        })
    })
    .await
}

/// Makes a level active or inactive. An inactive level is hidden from sales forms and fills in no
/// prices; customers keep it as their default, so making it active again restores their prices.
pub async fn set_active(pool: &SqlitePool, id: i64, active: bool) -> Result<PriceLevel, String> {
    in_transaction(pool, move |conn| {
        Box::pin(async move {
            let level = one(conn, id).await?;
            update_in(conn, id, &PriceLevelInput { is_active: active, ..level.input() }).await?;
            one(conn, id).await
        })
    })
    .await
}

/// Deletes a level; customers who had it as their default go back to standard prices.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    in_transaction(pool, move |conn| {
        Box::pin(async move {
            let level = one(conn, id).await?;
            let before = snapshot(conn, &level).await?;
            sqlx::query("DELETE FROM price_levels WHERE id = ?1").bind(id).execute(&mut *conn).await.map_err(db_err)?;
            audit(conn, id, "PRICE_LEVEL_DELETED", Some(before), None, serde_json::json!({ "customersCleared": level.customer_count })).await?;
            Ok(())
        })
    })
    .await
}

/// A level as the price list and the sales forms show it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelSummary {
    pub id: i64,
    pub name: String,
    pub kind: LevelKind,
    pub percent_milli: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelPrice {
    pub price_level_id: i64,
    pub price_cents: i64,
    pub source: PriceSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceListRow {
    #[serde(flatten)]
    pub item: PriceItem,
    /// One per level, in the order of [`PriceList::levels`].
    pub prices: Vec<LevelPrice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerLevel {
    pub customer_id: i64,
    pub price_level_id: i64,
}

/// The Item Price List: every item's standard price and its price at each level; and each
/// customer's default level, so sales forms can fill in rates.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceList {
    pub levels: Vec<LevelSummary>,
    pub items: Vec<PriceListRow>,
    pub customer_levels: Vec<CustomerLevel>,
}

pub async fn price_list(pool: &SqlitePool) -> Result<PriceList, String> {
    let levels = list(pool).await?;
    let items = item_prices::list_items(pool).await?;
    let customer_levels: Vec<(i64, i64)> = sqlx::query_as("SELECT id, price_level_id FROM customers WHERE price_level_id IS NOT NULL")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(PriceList {
        items: items
            .into_iter()
            .map(|item| PriceListRow {
                prices: levels
                    .iter()
                    .map(|l| {
                        let (price_cents, source) = resolve(l, &item);
                        LevelPrice { price_level_id: l.id, price_cents, source }
                    })
                    .collect(),
                item,
            })
            .collect(),
        levels: levels
            .into_iter()
            .map(|l| LevelSummary { id: l.id, name: l.name, kind: l.kind, percent_milli: l.percent_milli, is_active: l.is_active })
            .collect(),
        customer_levels: customer_levels.into_iter().map(|(customer_id, price_level_id)| CustomerLevel { customer_id, price_level_id }).collect(),
    })
}

/// What `input` would price each item at, before it is saved.
pub async fn preview(pool: &SqlitePool, input: &PriceLevelInput) -> Result<Vec<ItemPreview>, String> {
    validate(input)?;
    let level = PriceLevel {
        id: 0,
        name: input.name.clone(),
        kind: input.kind,
        percent_milli: input.percent_milli,
        rounding: input.rounding,
        is_active: true,
        items: input.items.clone(),
        customer_count: 0,
    };
    Ok(item_prices::list_items(pool)
        .await?
        .iter()
        .map(|item| {
            let (price_cents, source) = resolve(&level, item);
            ItemPreview { product_id: item.id, price_cents, source }
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPreview {
    pub product_id: i64,
    pub price_cents: i64,
    pub source: PriceSource,
}

/// Recent price level changes from the audit trail, newest first.
pub async fn history(pool: &SqlitePool, limit: i64) -> Result<Vec<item_prices::PriceChangeRecord>, String> {
    let rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, timestamp, actor, payload_snapshot FROM ledger_audit_log
         WHERE action_type = 'PRICE_CHANGE' AND json_extract(payload_snapshot, '$.event') LIKE 'PRICE_LEVEL_%'
         ORDER BY id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(audit_record_id, timestamp, actor, payload)| item_prices::PriceChangeRecord {
            audit_record_id,
            timestamp,
            actor,
            details: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
        })
        .collect())
}

#[cfg(test)]
mod tests;
