use super::*;
use crate::db::test_pool;
use crate::inventory::{adjust, track_product, AdjustInput, TrackInput};
use crate::sales::{create_product, ProductInput};

const PCT: fn(i64) -> Adjustment = |percent_milli| Adjustment::Percent { percent_milli };
const AMT: fn(i64) -> Adjustment = |amount_cents| Adjustment::Amount { amount_cents };

#[test]
fn percentages_and_amounts_work_in_whole_cents() {
    // +10% of 19.99 = 1.999 → 2.00, so 21.99.
    assert_eq!(new_price(1_999, PCT(10_000), Rounding::None), Some(2_199));
    // −12.5% of 10.00 = 8.75.
    assert_eq!(new_price(1_000, PCT(-12_500), Rounding::None), Some(875));
    // Half a cent rounds away from zero: 0.5% of 1.00 = 0.005 → 0.01 either way.
    assert_eq!(new_price(100, PCT(500), Rounding::None), Some(101));
    assert_eq!(new_price(100, PCT(-500), Rounding::None), Some(99));
    assert_eq!(new_price(1_000, AMT(250), Rounding::None), Some(1_250));
    assert_eq!(new_price(1_000, AMT(-1_000), Rounding::None), Some(0));
    // Below zero is refused rather than clamped.
    assert_eq!(new_price(1_000, AMT(-1_001), Rounding::None), None);
    assert_eq!(new_price(1_000, PCT(-100_100), Rounding::None), None);
    // Cost plus 40% markup.
    assert_eq!(new_price(1_234, PCT(40_000), Rounding::None), Some(1_728));
}

#[test]
fn rounding_steps_and_endings() {
    let r = |mode, cents| round_price(cents, mode);
    assert_eq!(r(Rounding::Nearest { step_cents: 5 }, 1_232), 1_230);
    assert_eq!(r(Rounding::Nearest { step_cents: 5 }, 1_233), 1_235);
    assert_eq!(r(Rounding::Nearest { step_cents: 100 }, 1_250), 1_300, "halves go up");
    assert_eq!(r(Rounding::Nearest { step_cents: 100 }, 1_249), 1_200);
    assert_eq!(r(Rounding::Up { step_cents: 25 }, 1_201), 1_225);
    assert_eq!(r(Rounding::Up { step_cents: 25 }, 1_225), 1_225);
    assert_eq!(r(Rounding::Down { step_cents: 100 }, 1_299), 1_200);
    assert_eq!(r(Rounding::EndIn99, 1_240), 1_299);
    assert_eq!(r(Rounding::EndIn99, 1_299), 1_299);
    assert_eq!(r(Rounding::EndIn99, 1_300), 1_399);
    assert_eq!(r(Rounding::EndIn99, 0), 99);
    assert_eq!(new_price(1_999, PCT(10_000), Rounding::Nearest { step_cents: 100 }), Some(2_200));

    assert!(validate(PCT(1_000_001), Rounding::None).is_err());
    assert!(validate(AMT(-100_000_001), Rounding::None).is_err());
    assert!(validate(PCT(5_000), Rounding::Up { step_cents: 0 }).is_err());
    assert!(validate(PCT(5_000), Rounding::Nearest { step_cents: 100_001 }).is_err());
    assert!(validate(PCT(-1_000_000), Rounding::Down { step_cents: 100_000 }).is_ok());
}

#[test]
fn the_change_reads_from_the_screen_as_json() {
    let input: PriceChangeInput = serde_json::from_value(serde_json::json!({
        "productIds": [1, 2],
        "basis": "COST",
        "adjustment": { "kind": "PERCENT", "percentMilli": 25000 },
        "rounding": { "mode": "UP", "stepCents": 25 },
    }))
    .unwrap();
    assert_eq!((input.basis, input.adjustment, input.rounding), (Basis::Cost, PCT(25_000), Rounding::Up { step_cents: 25 }));
    let end: Rounding = serde_json::from_value(serde_json::json!({ "mode": "END_IN99" })).unwrap();
    assert_eq!(end, Rounding::EndIn99);
}

struct Catalog {
    pool: SqlitePool,
    consulting: i64,
    widget: i64,
    gadget: i64,
}

/// Consulting (service, 150.00), Widget (inventory: 10 on hand costing 52.50 → 5.25 each; 25.00),
/// Gadget (inventory, never bought; 9.99).
async fn catalog() -> Catalog {
    let pool = test_pool().await;
    let account = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (sales, services, obe) = (account("4000").await, account("4100").await, account("3000").await);
    let product = |name: &str, rate_cents, income_account_id| ProductInput { name: name.into(), description: None, rate_cents, income_account_id, is_taxable: true };
    let consulting = create_product(&pool, product("Consulting", 15_000, services)).await.unwrap().id;
    let widget = create_product(&pool, product("Widget", 2_500, sales)).await.unwrap().id;
    let gadget = create_product(&pool, product("Gadget", 999, sales)).await.unwrap().id;
    let track = |product_id| TrackInput { product_id, sku: None, asset_account_id: None, cogs_account_id: None };
    let item = track_product(&pool, &track(widget)).await.unwrap().id;
    track_product(&pool, &track(gadget)).await.unwrap();
    adjust(&pool, &AdjustInput { item_id: item, date: "2026-10-01".parse().unwrap(), qty_change_e4: 100_000, total_cost_cents: Some(5_250), offset_account_id: obe, memo: None })
        .await
        .unwrap();
    Catalog { pool, consulting, widget, gadget }
}

#[tokio::test]
async fn items_list_their_type_price_and_unit_cost() {
    let c = catalog().await;
    let items = list_items(&c.pool).await.unwrap();
    let find = |id| items.iter().find(|i| i.id == id).unwrap();
    assert_eq!((find(c.consulting).is_inventory, find(c.consulting).unit_cost_cents), (false, None));
    assert_eq!((find(c.widget).is_inventory, find(c.widget).rate_cents, find(c.widget).unit_cost_cents), (true, 2_500, Some(525)));
    assert_eq!((find(c.gadget).is_inventory, find(c.gadget).unit_cost_cents), (true, None));
}

#[tokio::test]
async fn preview_explains_the_items_it_leaves_alone() {
    let c = catalog().await;
    let input = PriceChangeInput {
        product_ids: vec![c.consulting, c.widget, c.gadget],
        basis: Basis::Cost,
        adjustment: PCT(100_000),
        rounding: Rounding::EndIn99,
    };
    let preview = preview(&c.pool, &input).await.unwrap();
    // Widget: 5.25 × 2 = 10.50 → 10.99. The others have no cost.
    assert_eq!(preview[1].new_cents, Some(1_099));
    assert_eq!(preview[0].new_cents, None);
    assert!(preview[0].reason.as_deref().unwrap().starts_with("No cost"));
    assert_eq!(preview[2].new_cents, None);

    let down = PriceChangeInput { product_ids: vec![c.consulting, c.gadget], basis: Basis::CurrentPrice, adjustment: AMT(-1_000), rounding: Rounding::None };
    let preview = super::preview(&c.pool, &down).await.unwrap();
    assert_eq!(preview[0].new_cents, Some(14_000));
    assert_eq!((preview[1].new_cents, preview[1].reason.as_deref()), (None, Some("The new price would be below zero")));

    let unknown = PriceChangeInput { product_ids: vec![9_999], ..down };
    assert!(super::preview(&c.pool, &unknown).await.unwrap_err().starts_with("PRICE_UNKNOWN_ITEM"));
}

#[tokio::test]
async fn applying_sets_prices_records_the_audit_trail_and_leaves_documents_alone() {
    let c = catalog().await;
    // An invoice already written at Consulting's old price.
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&c.pool).await.unwrap();
    let invoice = crate::invoices::create_and_post_invoice(
        &c.pool,
        crate::invoices::InvoiceInput {
            customer_name: "Acme".into(),
            issue_date: "2026-10-02".parse().unwrap(),
            due_date: "2026-11-01".parse().unwrap(),
            lines: vec![crate::invoices::InvoiceLineInput {
                description: "Consulting".into(),
                quantity_e4: 10_000,
                rate_cents: 15_000,
                income_account_id: services,
                product_service_id: Some(c.consulting),
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .unwrap();

    let input = PriceChangeInput {
        product_ids: vec![c.consulting, c.widget],
        basis: Basis::CurrentPrice,
        adjustment: PCT(10_000),
        rounding: Rounding::Nearest { step_cents: 100 },
    };
    let applied = apply(&c.pool, &input).await.unwrap();
    assert_eq!(applied.changed.iter().map(|p| (p.product_id, p.current_cents, p.new_cents)).collect::<Vec<_>>(), vec![
        (c.consulting, 15_000, Some(16_500)),
        (c.widget, 2_500, Some(2_800)), // 27.50 rounds up to 28.00
    ]);
    let prices: Vec<(i64, i64)> = sqlx::query_as("SELECT id, rate_cents FROM products_services ORDER BY id").fetch_all(&c.pool).await.unwrap();
    assert_eq!(prices, vec![(c.consulting, 16_500), (c.widget, 2_800), (c.gadget, 999)]);

    // The invoice keeps the price it was written with.
    let detail = crate::invoices::invoice_detail(&c.pool, invoice.id).await.unwrap();
    assert_eq!((detail.items[0].rate_cents, detail.invoice.total_cents), (15_000, 15_000));

    // One sealed PRICE_CHANGE record, listing old and new prices, and the chain still verifies.
    let history = history(&c.pool, 10).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].audit_record_id, applied.audit_record_id);
    assert!(history[0].actor.is_some(), "sealed with who made the change");
    let d = &history[0].details;
    assert_eq!(d["event"], "CHANGE_ITEM_PRICES");
    assert_eq!(d["basis"], "CURRENT_PRICE");
    assert_eq!(d["adjustment"], serde_json::json!({ "kind": "PERCENT", "percentMilli": 10_000 }));
    assert_eq!(d["items"][0], serde_json::json!({ "productId": c.consulting, "name": "Consulting", "beforeCents": 15_000, "afterCents": 16_500 }));
    let chain = crate::audit_chain::verify(&c.pool).await.unwrap();
    assert!(chain.problems.is_empty(), "{:?}", chain.problems);
    assert_eq!(chain.records_pending, 0);
}

#[tokio::test]
async fn applying_nothing_changes_nothing() {
    let c = catalog().await;
    let same = PriceChangeInput { product_ids: vec![c.consulting], basis: Basis::CurrentPrice, adjustment: AMT(0), rounding: Rounding::None };
    assert!(apply(&c.pool, &same).await.unwrap_err().starts_with("PRICE_NOTHING_TO_CHANGE"));
    let no_cost = PriceChangeInput { product_ids: vec![c.consulting, c.gadget], basis: Basis::Cost, adjustment: PCT(50_000), rounding: Rounding::None };
    assert!(apply(&c.pool, &no_cost).await.unwrap_err().starts_with("PRICE_NOTHING_TO_CHANGE"));
    let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'PRICE_CHANGE'").fetch_one(&c.pool).await.unwrap();
    assert_eq!(records, 0);
    let bad = PriceChangeInput { product_ids: vec![c.consulting], basis: Basis::CurrentPrice, adjustment: PCT(5_000), rounding: Rounding::Up { step_cents: 0 } };
    assert!(apply(&c.pool, &bad).await.unwrap_err().starts_with("PRICE_BAD_ROUNDING"));
}
