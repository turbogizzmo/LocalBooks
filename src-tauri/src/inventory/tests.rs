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
        crate::sales::ProductInput { name: "Widget".into(), description: None, rate_cents: 2_500, income_account_id: sales, is_taxable: true },
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
async fn cost_of_goods_sold_is_split_between_the_classes_on_a_sale() {
    let s = shop().await;
    let class = |name: &str| crate::classes::create_class(&s.pool, crate::classes::ClassInput { name: name.into(), parent_id: None });
    let (east, west) = (class("East").await.unwrap().id, class("West").await.unwrap().id);
    bill(&s, 3, 1_000, s.inventory).await.unwrap(); // 3 units at $10.00

    let input: crate::invoices::InvoiceInput = serde_json::from_value(json!({
        "customerName": "Customer", "issueDate": "2026-04-10", "dueDate": "2026-05-10",
        "lines": [
            { "description": "Widget", "quantityE4": 10_000, "rateCents": 2_500, "incomeAccountId": s.sales, "productServiceId": s.product, "classId": east },
            { "description": "Widget", "quantityE4": 20_000, "rateCents": 2_500, "incomeAccountId": s.sales, "productServiceId": s.product, "classId": west },
            { "description": "Widget", "quantityE4": 10_000, "rateCents": 2_500, "incomeAccountId": s.sales, "productServiceId": s.product, "classId": east }
        ]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(&s.pool, input).await.unwrap();

    let by_class = |account: i64, class: i64| {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1 AND class_id = ?2")
            .bind(account)
            .bind(class)
            .fetch_one(&s.pool)
    };
    // 4 units sold against 3 on hand: $30.00 actual plus one more at the last cost, $40.00 in all.
    assert_eq!((by_class(s.sales, east).await.unwrap(), by_class(s.sales, west).await.unwrap()), (-5_000, -5_000));
    assert_eq!((by_class(s.cogs, east).await.unwrap(), by_class(s.cogs, west).await.unwrap()), (2_000, 2_000));
    assert_eq!(gl(&s.pool, s.cogs).await, 4_000);
    assert_reconciled(&s).await;
}

#[test]
fn quantity_split_never_goes_negative_and_adds_up() {
    let classes: Vec<(Option<i64>, i64)> = (0..7).map(|c| (Some(c), 1)).collect();
    for cost in [1, 2, 3, 5, 100, 101] {
        let shares = split_by_quantity(cost, 7, &classes).unwrap();
        assert!(shares.iter().all(|(_, s)| *s >= 0), "{shares:?}");
        assert_eq!(shares.iter().map(|(_, s)| s).sum::<i64>(), cost);
    }
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
    bill(&s, 10, 700, s.inventory).await.unwrap();
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

    // A write-off can't remove stock that isn't there.
    let shrink = account(&s.pool, "5000").await;
    let err = adjust(&s.pool, &AdjustInput { item_id: s.item, date: "2026-04-30".parse().unwrap(), qty_change_e4: -1_000_000, total_cost_cents: None, offset_account_id: shrink, memo: None })
        .await
        .unwrap_err();
    assert!(err.starts_with("STOCK_SHORT"), "{err}");
}

#[tokio::test]
async fn selling_before_the_bill_costs_at_the_last_purchase_cost_and_trues_up_on_arrival() {
    let s = shop().await;
    // Never bought, nothing on hand: the sale goes through at a provisional cost of zero.
    invoice(&s, 20_000, None).await.unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents, w.average_cost_cents), (-20_000, 0, None));
    assert_eq!(gl(&s.pool, s.cogs).await, 0);

    // The bill arrives: 5 at $12.00. The 2 already sold really cost $24.00, moved to COGS in the
    // bill's own entry; the 3 left are worth $36.00.
    let b = bill(&s, 5, 1_200, s.inventory).await.unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents), (30_000, 3_600));
    assert_eq!(gl(&s.pool, s.cogs).await, 2_400);
    assert_reconciled(&s).await;
    let bill_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bills WHERE id = ?1").bind(b.id).fetch_one(&s.pool).await.unwrap();
    let entry_lines: Vec<(i64, i64, i64)> = sqlx::query_as("SELECT account_id, debit_cents, credit_cents FROM journal_lines WHERE journal_entry_id = ?1").bind(bill_entry).fetch_all(&s.pool).await.unwrap();
    assert!(entry_lines.contains(&(s.cogs, 2_400, 0)) && entry_lines.contains(&(s.inventory, 0, 2_400)), "{entry_lines:?}");

    // Selling past zero again: 3 covered by stock ($36.00), 2 more at the last cost of $12.00.
    receipt(&s, 50_000).await.unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents), (-20_000, -2_400));
    assert_eq!(gl(&s.pool, s.cogs).await, 2_400 + 3_600 + 2_400);
    assert_reconciled(&s).await;

    // A smaller bill covers only part of the shortfall: 1 at $13.00, $1.00 more than its
    // provisional cost, so COGS rises by $1.00 and 1 unit is still owed at $12.00.
    bill(&s, 1, 1_300, s.inventory).await.unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents), (-10_000, -1_200));
    assert_eq!(gl(&s.pool, s.cogs).await, 8_400 + 100);

    // Voiding that bill puts everything back exactly, true-up included.
    let last_bill: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bills ORDER BY id DESC LIMIT 1").fetch_one(&s.pool).await.unwrap();
    ledger::void_transaction(&s.pool, last_bill).await.unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents), (-20_000, -2_400));
    assert_eq!(gl(&s.pool, s.cogs).await, 8_400);
    assert_reconciled(&s).await;

    // Opening stock added by adjustment while below zero trues up too: 4 for $40.00.
    let obe: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Opening Balance Equity'").fetch_one(&s.pool).await.unwrap();
    adjust(&s.pool, &AdjustInput { item_id: s.item, date: "2026-04-20".parse().unwrap(), qty_change_e4: 40_000, total_cost_cents: Some(4_000), offset_account_id: obe, memo: None })
        .await
        .unwrap();
    let w = item(&s.pool, s.item).await;
    assert_eq!((w.qty_on_hand_e4, w.value_cents), (20_000, 2_000), "2 left at $10.00 each");
    assert_eq!(gl(&s.pool, s.cogs).await, 8_400 - 400, "the 2 sold at $12.00 provisional really cost $10.00");
    assert_reconciled(&s).await;
}

/// Random sequences of sales, purchases and voids never fail on arithmetic, never leave quantity
/// and value disagreeing in sign, and always keep the stock value equal to the inventory account.
#[tokio::test]
async fn random_sequences_never_divide_by_zero_and_stay_reconciled() {
    let s = shop().await;
    let mut seed: u64 = 0x5eed;
    let mut next = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut sales: Vec<i64> = Vec::new();
    for _ in 0..120 {
        match next(4) {
            0 | 1 => {
                let qty = (next(6) as i64 + 1) * 5_000; // 0.5 to 3 units
                let inv = invoice(&s, qty, None).await.expect("selling never fails on arithmetic");
                sales.push(inv.journal_entry_id.unwrap());
            }
            2 => {
                bill(&s, next(4) as i64 + 1, 500 + next(1_000) as i64, s.inventory).await.expect("buying never fails");
            }
            _ => {
                if let Some(entry) = sales.pop() {
                    // A void may be refused if later movements depend on it; it must never break the books.
                    let _ = ledger::void_transaction(&s.pool, entry).await;
                }
            }
        }
        let w = item(&s.pool, s.item).await;
        let consistent = (w.qty_on_hand_e4 > 0 && w.value_cents >= 0) || (w.qty_on_hand_e4 == 0 && w.value_cents == 0) || (w.qty_on_hand_e4 < 0 && w.value_cents <= 0);
        assert!(consistent, "{} on hand valued at {}", w.qty_on_hand_e4, w.value_cents);
        assert_reconciled(&s).await;
    }
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
