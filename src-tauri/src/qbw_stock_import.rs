//! Item quantities in a full import of a desktop company file (gap R3c).
//!
//! The reader ([`crate::qbw_stock`]) gives the items the file tracks in stock, with the quantity
//! and value it stores for each, and every change to their stock with the transaction that made
//! it. [`stage`] keeps them with the batch. After a full import has posted the transactions,
//! [`post`] starts tracking each item and records each change as a stock movement on the entry its
//! transaction became, so the stock ledger and the Inventory Asset account agree the way they do
//! for stock entered in LocalBooks, and voiding an imported sale or purchase puts its stock back.
//!
//! Every item ends up compared with what the file stores ([`summary`]); one that differs, or that
//! could not be tracked, is listed with the reason rather than left out.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::db_err;
use crate::qbw_stock::SourceStock;

/// Stages a ledger's stock with its batch.
pub(crate) async fn stage(tx: &mut Transaction<'_, Sqlite>, batch_id: i64, stock: &SourceStock) -> Result<(), String> {
    for item in &stock.items {
        sqlx::query(
            "INSERT OR IGNORE INTO ledger_import_stock_items
                (batch_id, source_item_id, asset_source_account_id, cogs_source_account_id, stored_qty_e4, stored_value_cents, read_note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(batch_id)
        .bind(item.item_id)
        .bind(item.asset_account_id)
        .bind(item.cogs_account_id)
        .bind(item.stored_qty_e4)
        .bind(item.stored_value_cents)
        .bind(stock.unread_sales.get(&item.item_id).map(|&n| {
            format!("the cost of {} of it could not be read, so the stock {} took out is missing", count(n as i64, "sale", "sales"), if n == 1 { "it" } else { "they" })
        }))
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    for (seq, m) in stock.moves.iter().enumerate() {
        sqlx::query(
            "INSERT INTO ledger_import_stock_moves (batch_id, seq, source_txn_id, source_item_id, qty_change_e4, value_change_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(batch_id)
        .bind(seq as i64)
        .bind(m.transaction_id)
        .bind(m.item_id)
        .bind(m.qty_e4)
        .bind(m.value_cents)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// What a full import did with the file's item quantities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockResult {
    /// Items the file tracks in stock.
    pub items: i64,
    /// Items LocalBooks now tracks with the import's stock movements.
    pub tracked: i64,
    /// Tracked items whose quantity and value match what the file stores.
    pub matching: i64,
    pub movements: i64,
}

/// The stock movement kind for a change made by a transaction of `kind` (the reader's kind name).
fn movement_kind(kind: &str) -> &'static str {
    match kind {
        "Bill" | "Check" | "Credit card charge" => "PURCHASE",
        "Invoice" | "Sales receipt" | "Credit memo" => "SALE",
        _ => "ADJUSTMENT",
    }
}

/// Quantity and value may only stand together with the same sign (see `inventory`).
fn holds(qty_e4: i64, value_cents: i64) -> bool {
    (qty_e4 > 0 && value_cents >= 0) || (qty_e4 == 0 && value_cents == 0) || (qty_e4 < 0 && value_cents <= 0)
}

/// Why an item is short when transactions that change its stock could not be posted.
fn unposted_note(n: i64) -> String {
    if n == 1 {
        "1 transaction that changes its stock could not be posted; it is listed as not read completely".into()
    } else {
        format!("{n} transactions that change its stock could not be posted; they are listed as not read completely")
    }
}

fn count(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One change to apply: (item, quantity, value, date, transaction kind, entry).
type Change = (i64, i64, i64, NaiveDate, String, Option<i64>);

/// Quantity and value still carried at the end, by item.
type Carried = HashMap<i64, (i64, i64)>;

/// The changes in the order they are recorded, and how many fall on transactions that were not
/// posted, by item. Each change is added to what came before it; one that would leave quantity
/// and value of different signs (the file's own order of a sale and a purchase on one day, or
/// rounding left on an item sold out) is carried into the next change of its item instead, so
/// that change records both. What is still carried at the end is returned by item.
pub(crate) fn plan(changes: &[Change]) -> (Vec<Change>, HashMap<i64, i64>, Carried) {
    let mut state: HashMap<i64, (i64, i64)> = HashMap::new();
    let mut carry: HashMap<i64, (i64, i64)> = HashMap::new();
    let mut unposted: HashMap<i64, i64> = HashMap::new();
    let mut out = Vec::new();
    for (item, qty, value, date, kind, entry) in changes {
        let Some(entry) = entry else {
            *unposted.entry(*item).or_default() += 1;
            continue;
        };
        let held = carry.remove(item).unwrap_or((0, 0));
        let (Some(q), Some(v)) = (qty.checked_add(held.0), value.checked_add(held.1)) else {
            carry.insert(*item, held);
            continue;
        };
        let now = state.get(item).copied().unwrap_or((0, 0));
        let (Some(after_q), Some(after_v)) = (now.0.checked_add(q), now.1.checked_add(v)) else {
            carry.insert(*item, held);
            continue;
        };
        if !holds(after_q, after_v) {
            carry.insert(*item, (q, v));
            continue;
        }
        if q != 0 || v != 0 {
            out.push((*item, q, v, *date, kind.clone(), Some(*entry)));
        }
        state.insert(*item, (after_q, after_v));
    }
    carry.retain(|_, c| *c != (0, 0));
    (out, unposted, carry)
}

/// After a full import has posted, starts tracking each item the file tracks and records its
/// stock movements on the entries their transactions became. Runs in the import's own
/// transaction, so a refusal leaves nothing behind.
pub(crate) async fn post(tx: &mut Transaction<'_, Sqlite>, batch_id: i64) -> Result<Option<StockResult>, String> {
    #[derive(sqlx::FromRow)]
    struct Item {
        source_item_id: i64,
        name: Option<String>,
        product_id: Option<i64>,
        item_type: Option<String>,
        already_tracked: bool,
        asset_id: Option<i64>,
        asset_type: Option<String>,
        asset_system: Option<bool>,
        cogs_id: Option<i64>,
        cogs_type: Option<String>,
        stored_qty_e4: Option<i64>,
        stored_value_cents: Option<i64>,
    }
    let items: Vec<Item> = sqlx::query_as(
        "SELECT s.source_item_id, i.name, i.product_id, p.item_type,
                EXISTS (SELECT 1 FROM inventory_items x WHERE x.product_id = i.product_id) AS already_tracked,
                aa.account_id AS asset_id, ac.account_type AS asset_type, ac.is_system AS asset_system,
                ca.account_id AS cogs_id, cc.account_type AS cogs_type, s.stored_qty_e4, s.stored_value_cents
         FROM ledger_import_stock_items s
         LEFT JOIN ledger_import_items i ON i.batch_id = s.batch_id AND i.source_id = s.source_item_id
         LEFT JOIN products_services p ON p.id = i.product_id
         LEFT JOIN ledger_import_accounts aa ON aa.batch_id = s.batch_id AND aa.source_id = s.asset_source_account_id
         LEFT JOIN chart_of_accounts ac ON ac.id = aa.account_id
         LEFT JOIN ledger_import_accounts ca ON ca.batch_id = s.batch_id AND ca.source_id = s.cogs_source_account_id
         LEFT JOIN chart_of_accounts cc ON cc.id = ca.account_id
         WHERE s.batch_id = ?1 ORDER BY s.source_item_id",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if items.is_empty() {
        return Ok(None);
    }
    let mut result = StockResult { items: items.len() as i64, ..Default::default() };
    let mut tracked: HashMap<i64, i64> = HashMap::new();
    let mut notes: HashMap<i64, String> = HashMap::new();
    for item in &items {
        let refused = match (item.product_id, item.item_type.as_deref(), item.asset_id) {
            (None, _, _) => Some("it was not brought in as an item".to_string()),
            (Some(_), Some(t), _) if t != "ITEM" => Some("it was brought in as a kind of item LocalBooks doesn't track in stock".to_string()),
            (Some(_), _, _) if item.already_tracked => Some("LocalBooks already tracked this item before the import, so its stock was left as it was".to_string()),
            (Some(_), _, None) => Some("its inventory account isn't matched to a LocalBooks account".to_string()),
            (Some(_), _, Some(_)) if item.asset_type.as_deref() != Some("OTHER_CURRENT_ASSET") || item.asset_system == Some(true) => {
                Some("its inventory account isn't an Other Current Asset account in LocalBooks".to_string())
            }
            _ => None,
        };
        if let Some(note) = refused {
            notes.insert(item.source_item_id, note);
            continue;
        }
        let (Some(product), Some(asset)) = (item.product_id, item.asset_id) else { continue };
        let cogs = match (item.cogs_id, item.cogs_type.as_deref()) {
            (Some(c), Some("COST_OF_GOODS_SOLD")) => c,
            // The file may post an item's cost to an expense account; LocalBooks keeps it in a
            // Cost of Goods Sold account, its own when the file's isn't one.
            _ => crate::company_setup::ensure_standard_account(tx, crate::company_setup::COST_OF_GOODS_SOLD).await?,
        };
        let id = crate::inventory::track_imported(tx, product, asset, cogs).await.map_err(|e| format!("{e} (item {})", item.name.as_deref().unwrap_or("?")))?;
        tracked.insert(item.source_item_id, id);
    }
    result.tracked = tracked.len() as i64;

    type Row = (i64, i64, i64, String, String, Option<i64>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT m.source_item_id, m.qty_change_e4, m.value_change_cents, t.date, t.kind,
                CASE WHEN e.is_voided = 0 THEN t.journal_entry_id END
         FROM ledger_import_stock_moves m
         JOIN ledger_import_transactions t ON t.batch_id = m.batch_id AND t.source_id = m.source_txn_id
         LEFT JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE m.batch_id = ?1 ORDER BY t.date, t.source_id, m.seq",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let mut changes: Vec<Change> = Vec::with_capacity(rows.len());
    for (item, qty, value, date, kind, entry) in rows {
        if tracked.contains_key(&item) {
            let date: NaiveDate = date.parse().map_err(|_| format!("QBW_BAD_DATE: staged date '{date}' is not a date"))?;
            changes.push((item, qty, value, date, kind, entry));
        }
    }
    let (planned, unposted, carried) = plan(&changes);
    for (item, qty, value, date, kind, entry) in &planned {
        let Some(entry) = entry else { continue };
        crate::inventory::record_imported(tx, tracked[item], *date, movement_kind(kind), *qty, *value, *entry).await?;
        result.movements += 1;
    }
    for item in &items {
        let Some(&id) = tracked.get(&item.source_item_id) else { continue };
        let mut why = Vec::new();
        if let Some(&n) = unposted.get(&item.source_item_id) {
            why.push(unposted_note(n));
        }
        if let Some(&(q, v)) = carried.get(&item.source_item_id) {
            why.push(format!(
                "its last changes ({} units, {}) would leave a quantity and value of different signs, which LocalBooks can't hold, so they were left out",
                crate::inventory::format_qty(q),
                crate::ledger::format_cents(v)
            ));
        }
        let (qty, value): (i64, i64) =
            sqlx::query_as("SELECT qty_on_hand_e4, value_cents FROM inventory_items WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)?;
        if (qty, value) == (item.stored_qty_e4.unwrap_or(0), item.stored_value_cents.unwrap_or(0)) {
            result.matching += 1;
        }
        if !why.is_empty() {
            notes.insert(item.source_item_id, why.join("; "));
        }
    }
    for item in &items {
        sqlx::query("UPDATE ledger_import_stock_items SET inventory_item_id = ?3, note = ?4 WHERE batch_id = ?1 AND source_item_id = ?2")
            .bind(batch_id)
            .bind(item.source_item_id)
            .bind(tracked.get(&item.source_item_id))
            .bind(notes.get(&item.source_item_id))
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(Some(result))
}

/// One item whose quantity or value as imported differs from what the file stores, or that the
/// import could not track. Quantities are × 10,000.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockDifference {
    pub name: String,
    /// What the file stores; `None` when it could not be read.
    pub expected_qty_e4: Option<i64>,
    pub expected_value_cents: Option<i64>,
    /// After a full import, what LocalBooks holds; before, what the import would bring in.
    pub imported_qty_e4: i64,
    pub imported_value_cents: i64,
    /// Why it differs, when the import knows.
    pub note: Option<String>,
}

/// The file's items in stock, compared with what the import brings in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockSummary {
    /// Items the file tracks in stock.
    pub items: i64,
    /// Items whose quantity and value as imported match the file.
    pub matching: i64,
    /// The others, by name.
    pub differences: Vec<StockDifference>,
}

/// Each item the file tracks, compared with the file: after a full import, with the stock
/// LocalBooks holds for it; before, with the changes the transactions that balance would bring in.
pub async fn summary(pool: &SqlitePool, batch_id: i64) -> Result<StockSummary, String> {
    let rows: Vec<StockDifference> = sqlx::query_as(
        "SELECT COALESCE(i.name, 'Item ' || s.source_item_id) AS name, s.stored_qty_e4 AS expected_qty_e4, s.stored_value_cents AS expected_value_cents,
                COALESCE(x.qty_on_hand_e4, (SELECT SUM(m.qty_change_e4) FROM ledger_import_stock_moves m
                     JOIN ledger_import_transactions t ON t.batch_id = m.batch_id AND t.source_id = m.source_txn_id
                     WHERE m.batch_id = s.batch_id AND m.source_item_id = s.source_item_id AND t.imbalance_cents = 0), 0) AS imported_qty_e4,
                COALESCE(x.value_cents, (SELECT SUM(m.value_change_cents) FROM ledger_import_stock_moves m
                     JOIN ledger_import_transactions t ON t.batch_id = m.batch_id AND t.source_id = m.source_txn_id
                     WHERE m.batch_id = s.batch_id AND m.source_item_id = s.source_item_id AND t.imbalance_cents = 0), 0) AS imported_value_cents,
                COALESCE(s.note,
                    CASE WHEN s.stored_qty_e4 IS NULL OR s.stored_value_cents IS NULL
                         THEN 'the file''s own quantity and value for this item could not be read, so they can''t be compared' END,
                    CASE WHEN s.inventory_item_id IS NULL THEN
                    (SELECT CASE COUNT(*) WHEN 1 THEN '1 transaction that changes its stock can''t be posted; it is listed as not read completely'
                            ELSE COUNT(*) || ' transactions that change its stock can''t be posted; they are listed as not read completely' END
                     FROM ledger_import_stock_moves m JOIN ledger_import_transactions t ON t.batch_id = m.batch_id AND t.source_id = m.source_txn_id
                     WHERE m.batch_id = s.batch_id AND m.source_item_id = s.source_item_id AND t.imbalance_cents <> 0
                     HAVING COUNT(*) > 0) END,
                    s.read_note) AS note
         FROM ledger_import_stock_items s
         LEFT JOIN ledger_import_items i ON i.batch_id = s.batch_id AND i.source_id = s.source_item_id
         LEFT JOIN inventory_items x ON x.id = s.inventory_item_id
         WHERE s.batch_id = ?1 ORDER BY name COLLATE NOCASE",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out = StockSummary { items: rows.len() as i64, ..Default::default() };
    for row in rows {
        let same = row.imported_qty_e4 == row.expected_qty_e4.unwrap_or(0) && row.imported_value_cents == row.expected_value_cents.unwrap_or(0);
        if same && row.note.is_none() {
            out.matching += 1;
        } else {
            out.differences.push(row);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
