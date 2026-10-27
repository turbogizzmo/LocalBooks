use super::*;
use crate::db::test_pool;
use crate::inventory::{adjust, track_product, AdjustInput, TrackInput};
use crate::sales::{create_product, ProductInput};

fn item(id: i64, rate_cents: i64, unit_cost_cents: Option<i64>) -> PriceItem {
    PriceItem { id, name: format!("Item {id}"), description: None, is_active: true, is_inventory: unit_cost_cents.is_some(), rate_cents, unit_cost_cents }
}

fn fixed(percent_milli: i64, rounding: Rounding) -> PriceLevel {
    PriceLevel { id: 1, name: "Wholesale".into(), kind: LevelKind::FixedPercent, percent_milli, rounding, is_active: true, items: vec![], customer_count: 0 }
}

fn per_item(items: Vec<LevelItem>, rounding: Rounding) -> PriceLevel {
    PriceLevel { kind: LevelKind::PerItem, items, ..fixed(0, rounding) }
}

fn row(product_id: i64, basis: ItemBasis, price_cents: Option<i64>, percent_milli: Option<i64>) -> LevelItem {
    LevelItem { product_id, basis, price_cents, percent_milli }
}

#[test]
fn fixed_percent_levels_change_the_standard_price_in_whole_cents() {
    // −10% of 19.99 = −1.999 → −2.00 (half away from zero), so 17.99.
    assert_eq!(resolve(&fixed(-10_000, Rounding::None), &item(1, 1_999, None)), (1_799, PriceSource::LevelPercent));
    // +12.5% of 10.00 = 11.25.
    assert_eq!(resolve(&fixed(12_500, Rounding::None), &item(1, 1_000, None)), (1_125, PriceSource::LevelPercent));
    // Half a cent goes away from zero: −0.5% of 1.00 → 0.99, +0.5% → 1.01.
    assert_eq!(resolve(&fixed(-500, Rounding::None), &item(1, 100, None)).0, 99);
    assert_eq!(resolve(&fixed(500, Rounding::None), &item(1, 100, None)).0, 101);
    // −100% makes it free, never negative.
    assert_eq!(resolve(&fixed(-100_000, Rounding::None), &item(1, 4_321, None)).0, 0);
    // A free item stays free.
    assert_eq!(resolve(&fixed(25_000, Rounding::None), &item(1, 0, None)).0, 0);
}

#[test]
fn levels_round_as_change_item_prices_does() {
    let at = |r, cents| resolve(&fixed(-15_000, r), &item(1, cents, None)).0;
    // 25.00 − 15% = 21.25.
    assert_eq!(at(Rounding::Nearest { step_cents: 100 }, 2_500), 2_100);
    assert_eq!(at(Rounding::Up { step_cents: 100 }, 2_500), 2_200);
    assert_eq!(at(Rounding::Down { step_cents: 25 }, 2_500), 2_125);
    assert_eq!(at(Rounding::Nearest { step_cents: 10 }, 2_500), 2_130, "21.25 → nearest 0.10 (halves up) = 21.30");
    assert_eq!(at(Rounding::EndIn99, 2_500), 2_199);
    // Per-item percentages round too; fixed per-item prices are used as entered.
    let level = per_item(vec![row(1, ItemBasis::Standard, None, Some(-15_000)), row(2, ItemBasis::Fixed, Some(1_234), None)], Rounding::EndIn99);
    assert_eq!(resolve(&level, &item(1, 2_500, None)), (2_199, PriceSource::StandardPercent));
    assert_eq!(resolve(&level, &item(2, 2_500, None)), (1_234, PriceSource::Fixed));
}

#[test]
fn per_item_levels_price_listed_items_and_leave_the_rest_at_standard() {
    let level = per_item(
        vec![
            row(1, ItemBasis::Fixed, Some(1_500), None),
            row(2, ItemBasis::Standard, None, Some(-20_000)),
            row(3, ItemBasis::Cost, None, Some(40_000)),
            row(4, ItemBasis::Cost, None, Some(40_000)),
        ],
        Rounding::None,
    );
    assert_eq!(resolve(&level, &item(1, 2_000, None)), (1_500, PriceSource::Fixed));
    assert_eq!(resolve(&level, &item(2, 2_000, None)), (1_600, PriceSource::StandardPercent));
    // Cost 12.34 + 40% = 17.276 → 17.28.
    assert_eq!(resolve(&level, &item(3, 2_000, Some(1_234))), (1_728, PriceSource::CostPercent));
    // No known cost: the standard price.
    assert_eq!(resolve(&level, &item(4, 2_000, None)), (2_000, PriceSource::NoCost));
    // Not on the level: the standard price.
    assert_eq!(resolve(&level, &item(5, 2_000, None)), (2_000, PriceSource::Standard));
}

#[test]
fn validation_refuses_bad_levels() {
    let ok = PriceLevelInput { name: "Wholesale".into(), kind: LevelKind::FixedPercent, percent_milli: -10_000, rounding: Rounding::None, items: vec![], is_active: true };
    assert!(validate(&ok).is_ok());
    let err = |input: PriceLevelInput| validate(&input).unwrap_err();
    assert!(err(PriceLevelInput { name: "  ".into(), ..ok.clone() }).starts_with("PRICE_LEVEL_NO_NAME"));
    assert!(err(PriceLevelInput { percent_milli: -100_001, ..ok.clone() }).starts_with("PRICE_LEVEL_BAD_PERCENT"));
    assert!(err(PriceLevelInput { percent_milli: 1_000_001, ..ok.clone() }).starts_with("PRICE_LEVEL_BAD_PERCENT"));
    assert!(err(PriceLevelInput { rounding: Rounding::Up { step_cents: 0 }, ..ok.clone() }).starts_with("PRICE_LEVEL_BAD_ROUNDING"));
    assert!(err(PriceLevelInput { items: vec![row(1, ItemBasis::Fixed, Some(1), None)], ..ok.clone() }).starts_with("PRICE_LEVEL_FIXED_HAS_ITEMS"));
    let per = PriceLevelInput { kind: LevelKind::PerItem, ..ok.clone() };
    let with = |items| PriceLevelInput { items, ..per.clone() };
    assert!(validate(&with(vec![row(1, ItemBasis::Fixed, Some(0), None), row(2, ItemBasis::Cost, None, Some(25_000))])).is_ok());
    assert!(err(with(vec![row(1, ItemBasis::Fixed, Some(-1), None)])).starts_with("PRICE_LEVEL_BAD_ITEM"));
    assert!(err(with(vec![row(1, ItemBasis::Fixed, None, None)])).starts_with("PRICE_LEVEL_BAD_ITEM"));
    assert!(err(with(vec![row(1, ItemBasis::Standard, Some(5), None)])).starts_with("PRICE_LEVEL_BAD_ITEM"));
    assert!(err(with(vec![row(1, ItemBasis::Cost, None, Some(-200_000))])).starts_with("PRICE_LEVEL_BAD_ITEM"));
    assert!(err(with(vec![row(1, ItemBasis::Fixed, Some(1), None), row(1, ItemBasis::Fixed, Some(2), None)])).starts_with("PRICE_LEVEL_DUPLICATE_ITEM"));
}

#[test]
fn levels_read_from_the_screen_as_json() {
    let input: PriceLevelInput = serde_json::from_value(serde_json::json!({
        "name": "Contractors",
        "kind": "PER_ITEM",
        "rounding": { "mode": "NEAREST", "stepCents": 5 },
        "items": [
            { "productId": 3, "basis": "FIXED", "priceCents": 1250 },
            { "productId": 4, "basis": "COST", "percentMilli": 30000 },
        ],
    }))
    .unwrap();
    assert_eq!(input.kind, LevelKind::PerItem);
    assert!(input.is_active, "active unless said otherwise");
    assert_eq!(input.items[1], row(4, ItemBasis::Cost, None, Some(30_000)));
}

struct Books {
    pool: SqlitePool,
    consulting: i64,
    widget: i64,
}

/// Consulting (service, 150.00); Widget (inventory: 10 on hand costing 52.50 → 5.25 each; 25.00).
async fn books() -> Books {
    let pool = test_pool().await;
    let account = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (sales, services, obe) = (account("4000").await, account("4100").await, account("3000").await);
    let product = |name: &str, rate_cents, income_account_id| ProductInput { name: name.into(), description: None, rate_cents, income_account_id, is_taxable: true };
    let consulting = create_product(&pool, product("Consulting", 15_000, services)).await.unwrap().id;
    let widget = create_product(&pool, product("Widget", 2_500, sales)).await.unwrap().id;
    let item = track_product(&pool, &TrackInput { product_id: widget, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap().id;
    adjust(&pool, &AdjustInput { item_id: item, date: "2026-10-01".parse().unwrap(), qty_change_e4: 100_000, total_cost_cents: Some(5_250), offset_account_id: obe, memo: None })
        .await
        .unwrap();
    Books { pool, consulting, widget }
}

fn wholesale() -> PriceLevelInput {
    PriceLevelInput { name: "Wholesale".into(), kind: LevelKind::FixedPercent, percent_milli: -10_000, rounding: Rounding::None, items: vec![], is_active: true }
}

#[tokio::test]
async fn the_price_list_shows_every_item_at_every_level_and_customer_defaults() {
    let b = books().await;
    let w = save(&b.pool, None, wholesale()).await.unwrap();
    let contractor = PriceLevelInput {
        name: "Contractor".into(),
        kind: LevelKind::PerItem,
        percent_milli: 0,
        rounding: Rounding::EndIn99,
        items: vec![row(b.widget, ItemBasis::Cost, None, Some(100_000)), row(b.consulting, ItemBasis::Fixed, Some(12_000), None)],
        is_active: true,
    };
    let c = save(&b.pool, None, contractor).await.unwrap();
    assert_eq!(c.items.len(), 2);

    let acme = crate::sales::create_customer(&b.pool, "Acme", None).await.unwrap();
    sqlx::query("UPDATE customers SET price_level_id = ?1 WHERE id = ?2").bind(w.id).bind(acme.id).execute(&b.pool).await.unwrap();

    let list = price_list(&b.pool).await.unwrap();
    assert_eq!(list.levels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["Contractor", "Wholesale"]);
    let prices = |id| list.items.iter().find(|r| r.item.id == id).unwrap().prices.iter().map(|p| p.price_cents).collect::<Vec<_>>();
    // Consulting: Contractor fixed 120.00; Wholesale 150.00 − 10%.
    assert_eq!(prices(b.consulting), vec![12_000, 13_500]);
    // Widget: cost 5.25 × 2 = 10.50 → 10.99; 25.00 − 10% = 22.50.
    assert_eq!(prices(b.widget), vec![1_099, 2_250]);
    assert_eq!(list.customer_levels.len(), 1);
    assert_eq!((list.customer_levels[0].customer_id, list.customer_levels[0].price_level_id), (acme.id, w.id));
}

#[tokio::test]
async fn changes_are_recorded_in_the_audit_trail() {
    let b = books().await;
    let level = save(&b.pool, None, wholesale()).await.unwrap();
    // Saving it unchanged records nothing.
    save(&b.pool, Some(level.id), wholesale()).await.unwrap();
    save(&b.pool, Some(level.id), PriceLevelInput { percent_milli: -15_000, ..wholesale() }).await.unwrap();
    set_active(&b.pool, level.id, false).await.unwrap();
    let acme = crate::sales::create_customer(&b.pool, "Acme", None).await.unwrap();
    sqlx::query("UPDATE customers SET price_level_id = ?1 WHERE id = ?2").bind(level.id).bind(acme.id).execute(&b.pool).await.unwrap();
    delete(&b.pool, level.id).await.unwrap();

    let events = history(&b.pool, 10).await.unwrap();
    let names: Vec<&str> = events.iter().map(|e| e.details["event"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["PRICE_LEVEL_DELETED", "PRICE_LEVEL_CHANGED", "PRICE_LEVEL_CHANGED", "PRICE_LEVEL_CREATED"]);
    assert_eq!(events[0].details["customersCleared"], 1);
    assert_eq!(events[2].details["before"]["percentMilli"], -10_000);
    assert_eq!(events[2].details["after"]["percentMilli"], -15_000);
    assert_eq!(events[1].details["after"]["isActive"], false);
    assert!(events.iter().all(|e| e.actor.is_some()), "sealed with who made the change");

    // The customer is back on standard prices; Change Item Prices' own history doesn't list these.
    let left: Option<i64> = sqlx::query_scalar("SELECT price_level_id FROM customers WHERE id = ?1").bind(acme.id).fetch_one(&b.pool).await.unwrap();
    assert_eq!(left, None);
    assert!(crate::item_prices::history(&b.pool, 10).await.unwrap().is_empty());
    let chain = crate::audit_chain::verify(&b.pool).await.unwrap();
    assert!(chain.problems.is_empty(), "{:?}", chain.problems);
}

#[tokio::test]
async fn saving_per_item_prices_in_another_order_records_nothing() {
    let b = books().await;
    // Widget sorts after Consulting by name but was created with a larger id: list it first.
    let items = vec![row(b.widget, ItemBasis::Fixed, Some(2_000), None), row(b.consulting, ItemBasis::Fixed, Some(12_000), None)];
    let input = PriceLevelInput { kind: LevelKind::PerItem, items, ..wholesale() };
    let level = save(&b.pool, None, input.clone()).await.unwrap();
    let reversed = PriceLevelInput { items: input.items.iter().rev().cloned().collect(), ..input.clone() };
    save(&b.pool, Some(level.id), input).await.unwrap();
    save(&b.pool, Some(level.id), reversed).await.unwrap();
    assert_eq!(history(&b.pool, 10).await.unwrap().len(), 1, "only the creation is recorded");
}

#[tokio::test]
async fn bad_saves_change_nothing() {
    let b = books().await;
    save(&b.pool, None, wholesale()).await.unwrap();
    assert!(save(&b.pool, None, PriceLevelInput { name: "wholesale".into(), ..wholesale() }).await.unwrap_err().starts_with("PRICE_LEVEL_DUPLICATE"));
    let unknown = PriceLevelInput { kind: LevelKind::PerItem, items: vec![row(9_999, ItemBasis::Fixed, Some(1), None)], name: "X".into(), ..wholesale() };
    assert!(save(&b.pool, None, unknown).await.unwrap_err().starts_with("PRICE_LEVEL_UNKNOWN_ITEM"));
    assert!(delete(&b.pool, 9_999).await.unwrap_err().starts_with("PRICE_LEVEL_NOT_FOUND"));
    assert_eq!(list(&b.pool).await.unwrap().len(), 1);
    assert_eq!(history(&b.pool, 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_customer_default_is_set_on_the_customer() {
    use crate::parties::{get_party, update_party, PartyDetailsInput};
    use crate::tax_ids::Access;
    let b = books().await;
    let level = save(&b.pool, None, wholesale()).await.unwrap();
    let acme = crate::sales::create_customer(&b.pool, "Acme", None).await.unwrap();
    let input = |price_level_id| PartyDetailsInput {
        name: "Acme".into(),
        company_name: None,
        email: None,
        phone: None,
        address: None,
        terms_days: None,
        notes: None,
        tax_id: None,
        keep_tax_id: true,
        is_1099: false,
        tax_rate_id: None,
        is_tax_exempt: false,
        tax_exemption: None,
        form_style_id: None,
        sales_rep_id: None,
        price_level_id,
        custom_fields: vec![],
    };
    let saved = update_party(&b.pool, "customer", acme.id, input(Some(level.id)), Access::OPEN).await.unwrap();
    assert_eq!(saved.price_level_id, Some(level.id));
    assert_eq!(get_party(&b.pool, "customer", acme.id, Access::OPEN).await.unwrap().price_level_id, Some(level.id));
    assert!(update_party(&b.pool, "customer", acme.id, input(Some(9_999)), Access::OPEN).await.unwrap_err().starts_with("PARTY_BAD_PRICE_LEVEL"));
    // An inactive level stays on customers who have it, but can't be newly given.
    set_active(&b.pool, level.id, false).await.unwrap();
    update_party(&b.pool, "customer", acme.id, input(Some(level.id)), Access::OPEN).await.unwrap();
    let globex = crate::sales::create_customer(&b.pool, "Globex", None).await.unwrap();
    let err = update_party(&b.pool, "customer", globex.id, PartyDetailsInput { name: "Globex".into(), ..input(Some(level.id)) }, Access::OPEN).await.unwrap_err();
    assert!(err.starts_with("PARTY_INACTIVE_PRICE_LEVEL"), "{err}");
}

#[tokio::test]
async fn preview_prices_an_unsaved_level_without_saving_it() {
    let b = books().await;
    let input = PriceLevelInput { kind: LevelKind::PerItem, items: vec![row(b.widget, ItemBasis::Standard, None, Some(-50_000))], ..wholesale() };
    let rows = preview(&b.pool, &input).await.unwrap();
    let at = |id| rows.iter().find(|r| r.product_id == id).map(|r| (r.price_cents, r.source)).unwrap();
    assert_eq!(at(b.widget), (1_250, PriceSource::StandardPercent));
    assert_eq!(at(b.consulting), (15_000, PriceSource::Standard));
    assert!(list(&b.pool).await.unwrap().is_empty());
    assert!(preview(&b.pool, &PriceLevelInput { percent_milli: -100_001, ..wholesale() }).await.is_err());
}

#[test]
fn percentages_read_from_files() {
    assert_eq!(parse_percent_milli("-10.0%"), Some(-10_000));
    assert_eq!(parse_percent_milli("12.5"), Some(12_500));
    assert_eq!(parse_percent_milli(" +7.25 % "), Some(7_250));
    assert_eq!(parse_percent_milli("−3"), Some(-3_000));
    // Beyond thousandths, rounded half away from zero.
    assert_eq!(parse_percent_milli("0.0005"), Some(1));
    assert_eq!(parse_percent_milli("-0.0005"), Some(-1));
    assert_eq!(parse_percent_milli(".5"), Some(500));
    assert_eq!(parse_percent_milli(""), None);
    assert_eq!(parse_percent_milli("ten"), None);
    assert_eq!(parse_percent_milli("1.2.3"), None);
}
