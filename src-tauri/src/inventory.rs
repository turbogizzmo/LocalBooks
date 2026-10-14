//! Inventory at moving average cost.
//!
//! A tracked product (`inventory_items`) holds a quantity and a total value. Stock comes in on bill
//! lines (debiting the item's Inventory Asset account) or by adjustment, and goes out on invoices and
//! sales receipts, which add Cost of Goods Sold lines to the sale's own journal entry:
//! debit COGS, credit Inventory Asset, at round(value × qty sold / qty on hand). Every movement is
//! appended to `stock_ledger_entries` with the journal entry that carries its money side; voiding that
//! entry reverses the movement, so stock and the Inventory Asset account always agree.

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
}

async fn item_for_product(tx: &mut Transaction<'_, Sqlite>, product_id: i64) -> Result<Option<Item>, String> {
    let row: Option<(i64, String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT i.id, p.name, i.asset_account_id, i.cogs_account_id, i.qty_on_hand_e4, i.value_cents
         FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE i.product_id = ?1",
    )
    .bind(product_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(row.map(|(id, name, asset_account_id, cogs_account_id, qty_e4, value_cents)| Item { id, name, asset_account_id, cogs_account_id, qty_e4, value_cents }))
}

fn format_qty(qty_e4: i64) -> String {
    crate::ledger::format_cents(qty_e4 / 100).trim_end_matches(".00").to_string()
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
    if qty_after < 0 {
        return Err(format!("STOCK_SHORT: only {} of {name} on hand", format_qty(qty)));
    }
    if value_after < 0 || (qty_after == 0 && value_after != 0) {
        return Err(format!(
            "STOCK_VALUE: this would leave {name} with {} units valued at {}; reverse later sales or adjustments first",
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

/// Cost of `qty_e4` units at moving average: the whole remaining value when they are the last units.
fn average_cost(item: &Item, qty_e4: i64) -> Result<i64, String> {
    if qty_e4 > item.qty_e4 {
        return Err(format!("STOCK_SHORT: only {} of {} on hand; receive stock or adjust the quantity first", format_qty(item.qty_e4), item.name));
    }
    if qty_e4 == item.qty_e4 {
        return Ok(item.value_cents);
    }
    mul_div_round(item.value_cents, qty_e4, item.qty_e4).ok_or_else(|| "STOCK_OVERFLOW: cost out of range".to_string())
}

/// Cost of goods sold for a sale, worked out before its journal entry is posted.
pub(crate) struct SaleCost {
    /// Extra journal lines: debit each COGS account, credit each Inventory Asset account.
    pub lines: Vec<JournalLine>,
    /// (item, quantity, cost) to record once the journal entry exists.
    movements: Vec<(i64, i64, i64)>,
}

/// Works out the cost of the tracked products on a sale's lines (`(product_id, quantity_e4)`).
/// Several lines for the same product are costed together, in one movement.
pub(crate) async fn cost_of_sale(tx: &mut Transaction<'_, Sqlite>, lines: &[(Option<i64>, i64)]) -> Result<SaleCost, String> {
    let mut per_product: BTreeMap<i64, i64> = BTreeMap::new();
    for (product, qty) in lines {
        if let Some(p) = product {
            *per_product.entry(*p).or_insert(0) += qty;
        }
    }
    let mut debits: BTreeMap<i64, i64> = BTreeMap::new();
    let mut credits: BTreeMap<i64, i64> = BTreeMap::new();
    let mut movements = Vec::new();
    for (product, qty) in per_product {
        let Some(item) = item_for_product(tx, product).await? else { continue };
        let cost = average_cost(&item, qty)?;
        if cost > 0 {
            *debits.entry(item.cogs_account_id).or_insert(0) += cost;
            *credits.entry(item.asset_account_id).or_insert(0) += cost;
        }
        movements.push((item.id, qty, cost));
    }
    let mut out: Vec<JournalLine> = debits.into_iter().map(|(a, c)| JournalLine::debit(a, c)).collect();
    out.extend(credits.into_iter().map(|(a, c)| JournalLine::credit(a, c)));
    Ok(SaleCost { lines: out, movements })
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

pub(crate) async fn record_purchase(tx: &mut Transaction<'_, Sqlite>, item_id: i64, date: NaiveDate, qty_e4: i64, cost_cents: i64, journal_entry_id: i64) -> Result<(), String> {
    record_movement(tx, item_id, date, "PURCHASE", qty_e4, cost_cents, journal_entry_id, None).await.map(|_| ())
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
    let mut tx = pool.begin().await.map_err(db_err)?;
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
        let entry_id = if value == 0 {
            None
        } else {
            let lines = if value > 0 {
                vec![JournalLine::debit(item.asset_account_id, value), JournalLine::credit(input.offset_account_id, value)]
            } else {
                vec![JournalLine::debit(input.offset_account_id, -value), JournalLine::credit(item.asset_account_id, -value)]
            };
            let entry = JournalEntry { id: None, date: input.date, description: description.clone(), lines, currency: None };
            Some(ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?)
        };
        let Some(entry_id) = entry_id else {
            return Err("STOCK_ADJUST_FREE: adding stock at no cost needs a cost of at least 0.01, so the books record it".to_string());
        };
        record_movement(&mut tx, item.id, input.date, "ADJUSTMENT", input.qty_change_e4, value, entry_id, None).await?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => tx.commit().await.map_err(db_err)?,
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
mod tests {
    use super::*;
    use crate::db::test_pool;
    use serde_json::json;

    async fn account(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
    }

    async fn gl(pool: &SqlitePool, account: i64) -> i64 {
        sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
    }

    async fn item(pool: &SqlitePool, id: i64) -> InventoryItemView {
        list_items(pool).await.unwrap().into_iter().find(|i| i.id == id).unwrap()
    }

    struct Shop {
        pool: SqlitePool,
        product: i64,
        item: i64,
        inventory: i64,
        cogs: i64,
        sales: i64,
        checking: i64,
    }

    async fn shop() -> Shop {
        let pool = test_pool().await;
        let sales = account(&pool, "4000").await;
        let product = crate::sales::create_product(
            &pool,
            crate::sales::ProductInput { name: "Widget".into(), description: None, rate_cents: 2_500, income_account_id: sales },
        )
        .await
        .unwrap()
        .id;
        let tracked = track_product(&pool, &TrackInput { product_id: product, sku: Some("W-1".into()), asset_account_id: None, cogs_account_id: None }).await.unwrap();
        Shop { inventory: tracked.asset_account_id, cogs: tracked.cogs_account_id, item: tracked.id, product, sales, checking: account(&pool, "1010").await, pool }
    }

    async fn bill(s: &Shop, qty: i64, rate: i64, account: i64) -> Result<crate::expenses::BillSummary, String> {
        let input: crate::expenses::BillInput = serde_json::from_value(json!({
            "vendorName": "Supplier", "billNumber": null, "billDate": "2026-04-02", "dueDate": "2026-05-02", "memo": null,
            "totalCents": qty * rate,
            "lines": [{ "accountId": account, "amountCents": qty * rate, "memo": null, "quantityE4": qty * 10_000, "rateCents": rate, "productServiceId": s.product }]
        }))
        .unwrap();
        crate::expenses::post_vendor_bill(&s.pool, input).await
    }

    async fn invoice(s: &Shop, qty_e4: i64, currency: Option<serde_json::Value>) -> Result<crate::invoices::InvoiceSummary, String> {
        let input: crate::invoices::InvoiceInput = serde_json::from_value(json!({
            "customerName": "Customer", "issueDate": "2026-04-10", "dueDate": "2026-05-10", "currency": currency,
            "lines": [{ "description": "Widget", "quantityE4": qty_e4, "rateCents": 2_500, "incomeAccountId": s.sales, "productServiceId": s.product }]
        }))
        .unwrap();
        crate::invoices::create_and_post_invoice(&s.pool, input).await
    }

    async fn receipt(s: &Shop, qty_e4: i64) -> Result<crate::sales::SalesReceiptSummary, String> {
        let input: crate::sales::SalesReceiptInput = serde_json::from_value(json!({
            "customerName": null, "receiptDate": "2026-04-12", "depositAccountId": s.checking, "paymentMethod": "CASH", "reference": null, "memo": null,
            "lines": [{ "productServiceId": s.product, "description": "Widget", "quantityE4": qty_e4, "rateCents": 2_500, "incomeAccountId": s.sales }]
        }))
        .unwrap();
        crate::sales::post_sales_receipt(&s.pool, input).await
    }

    /// The stock ledger and the Inventory Asset account must always agree.
    async fn assert_reconciled(s: &Shop) {
        assert_eq!(gl(&s.pool, s.inventory).await, item(&s.pool, s.item).await.value_cents, "stock value matches the Inventory Asset account");
    }

    #[tokio::test]
    async fn moving_average_cost_flows_through_purchases_sales_and_voids() {
        let s = shop().await;
        let obe: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Opening Balance Equity'").fetch_one(&s.pool).await.unwrap();

        // Opening stock: 10 units worth $50.00 in total.
        adjust(&s.pool, &AdjustInput { item_id: s.item, date: "2026-04-01".parse().unwrap(), qty_change_e4: 100_000, total_cost_cents: Some(5_000), offset_account_id: obe, memo: Some("Opening stock".into()) })
            .await
            .unwrap();
        // Bought 10 more at $7.00: 20 on hand worth $120.00, average $6.00.
        let bought = bill(&s, 10, 700, s.inventory).await.unwrap();
        let w = item(&s.pool, s.item).await;
        assert_eq!((w.qty_on_hand_e4, w.value_cents, w.average_cost_cents), (200_000, 12_000, Some(600)));
        assert_reconciled(&s).await;

        // Selling 5 on an invoice costs 5 × $6.00 = $30.00, in the invoice's own journal entry.
        let inv = invoice(&s, 50_000, None).await.unwrap();
        let lines: Vec<(i64, i64, i64)> = sqlx::query_as("SELECT account_id, debit_cents, credit_cents FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number")
            .bind(inv.journal_entry_id.unwrap())
            .fetch_all(&s.pool)
            .await
            .unwrap();
        assert!(lines.contains(&(s.cogs, 3_000, 0)) && lines.contains(&(s.inventory, 0, 3_000)), "{lines:?}");
        assert_eq!(gl(&s.pool, s.cogs).await, 3_000);
        assert_reconciled(&s).await;

        // Selling the last 15 takes exactly the remaining $90.00.
        let sold_out = receipt(&s, 150_000).await.unwrap();
        let w = item(&s.pool, s.item).await;
        assert_eq!((w.qty_on_hand_e4, w.value_cents, w.average_cost_cents), (0, 0, None));
        assert_eq!(gl(&s.pool, s.cogs).await, 12_000);

        // Nothing left: the sale is refused and nothing is posted.
        let invoices_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&s.pool).await.unwrap();
        assert!(invoice(&s, 10_000, None).await.unwrap_err().starts_with("STOCK_SHORT"));
        let invoices_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&s.pool).await.unwrap();
        assert_eq!(invoices_before, invoices_after);

        // The bill's 10 units have been sold, so voiding the bill is refused.
        let bill_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bills WHERE id = ?1").bind(bought.id).fetch_one(&s.pool).await.unwrap();
        assert!(ledger::void_transaction(&s.pool, bill_entry).await.unwrap_err().starts_with("STOCK_SHORT"));

        // Voiding the sales receipt puts its 15 units back at the cost they left at.
        ledger::void_transaction(&s.pool, sold_out.journal_entry_id.unwrap()).await.unwrap();
        let w = item(&s.pool, s.item).await;
        assert_eq!((w.qty_on_hand_e4, w.value_cents), (150_000, 9_000));
        assert_eq!(gl(&s.pool, s.cogs).await, 3_000, "the COGS reversed with the receipt");
        assert_reconciled(&s).await;

        // Counting 1 short writes it off at average cost.
        let shrink = account(&s.pool, "5000").await;
        adjust(&s.pool, &AdjustInput { item_id: s.item, date: "2026-04-30".parse().unwrap(), qty_change_e4: -10_000, total_cost_cents: None, offset_account_id: shrink, memo: Some("Count".into()) })
            .await
            .unwrap();
        assert_eq!(item(&s.pool, s.item).await.value_cents, 8_400);
        assert_reconciled(&s).await;

        let history = movements(&s.pool, s.item).await.unwrap();
        assert_eq!(history.iter().map(|m| m.kind.as_str()).collect::<Vec<_>>(), ["ADJUSTMENT", "VOID", "SALE", "SALE", "PURCHASE", "ADJUSTMENT"]);
    }

    #[tokio::test]
    async fn costs_round_without_drift_and_rules_hold() {
        let s = shop().await;
        bill(&s, 3, 333, s.inventory).await.unwrap(); // 3 units for $9.99
        let mut cogs = Vec::new();
        for _ in 0..3 {
            let before = gl(&s.pool, s.cogs).await;
            invoice(&s, 10_000, None).await.unwrap();
            cogs.push(gl(&s.pool, s.cogs).await - before);
        }
        assert_eq!(cogs, [333, 333, 333]);
        assert_eq!(item(&s.pool, s.item).await.value_cents, 0);

        bill(&s, 3, 334, s.inventory).await.unwrap(); // 3 units for $10.02: 334 each
        bill(&s, 1, 1, s.inventory).await.unwrap(); // and 1 for $0.01: 4 units, $10.03
        let before = gl(&s.pool, s.cogs).await;
        invoice(&s, 10_000, None).await.unwrap();
        assert_eq!(gl(&s.pool, s.cogs).await - before, 251, "1003 × 1/4 = 250.75, rounded");
        invoice(&s, 30_000, None).await.unwrap();
        assert_eq!(gl(&s.pool, s.cogs).await - before, 1_003, "the last units take exactly what is left");
        assert_reconciled(&s).await;

        bill(&s, 2, 500, s.inventory).await.unwrap();
        let eur = json!({ "code": "EUR", "exchangeRateE6": 1_080_000 });
        assert!(invoice(&s, 10_000, Some(eur)).await.unwrap_err().starts_with("INV_FX_INVENTORY"));
        assert!(bill(&s, 1, 500, account(&s.pool, "6000").await).await.unwrap_err().starts_with("STOCK_PURCHASE_ACCOUNT"));
        let err = sqlx::query("UPDATE inventory_items SET qty_on_hand_e4 = 999 WHERE id = ?1").bind(s.item).execute(&s.pool).await.map_err(db_err).unwrap_err();
        assert!(err.starts_with("STOCK_DIRECT_EDIT"), "{err}");
        let err = sqlx::query("DELETE FROM stock_ledger_entries").execute(&s.pool).await.map_err(db_err).unwrap_err();
        assert!(err.starts_with("STOCK_IMMUTABLE"), "{err}");
        assert!(track_product(&s.pool, &TrackInput { product_id: s.product, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap_err().starts_with("STOCK_ALREADY_TRACKED"));
    }
}
