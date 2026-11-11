use super::*;
use crate::db::test_pool;
use crate::money::Cents;
use crate::sales::{create_product, list_products, ProductInput};

async fn income(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap()
}

fn row(name: &str, rate: &str, cost: &str) -> PastedItem {
    PastedItem { name: name.into(), rate: Some(rate.into()), cost: Some(cost.into()), ..Default::default() }
}

#[tokio::test]
async fn purchase_details_batch_activity_and_pasted_items() {
    let pool = test_pool().await;
    let account = income(&pool).await;
    let bolt = create_product(&pool, ProductInput { name: "Bolt".into(), description: None, rate_cents: Cents(50), income_account_id: account, is_taxable: true }).await.unwrap().id;

    set_purchase_details(&pool, bolt, Some(" Hex bolt, box of 100 "), Some(1_200)).await.unwrap();
    let view = list_products(&pool).await.unwrap().into_iter().find(|p| p.id == bolt).unwrap();
    assert_eq!((view.purchase_description.as_deref(), view.purchase_cost_cents), (Some("Hex bolt, box of 100"), Some(1_200)));
    assert!(set_purchase_details(&pool, bolt, None, Some(-1)).await.unwrap_err().starts_with("ITEM_BAD_COST"));
    assert!(set_purchase_details(&pool, 9_999, None, None).await.unwrap_err().starts_with("ITEM_NOT_FOUND"));

    // Pasted rows: problems listed, nothing saved; then a clean paste adds and changes.
    let bad = paste_items(&pool, &[row("Nut", "x", ""), row("", "1", ""), row("Nut", "1", ""), PastedItem { name: "Washer".into(), income_account: Some("Nope".into()), ..Default::default() }]).await.unwrap();
    let messages: Vec<(usize, String)> = bad.problems.into_iter().map(|p| (p.row, p.message)).collect();
    assert_eq!(messages, [(1, "The price \"x\" isn't an amount of zero or more".into()), (2, "The name is empty".into()), (3, "Nut is pasted twice".into()), (4, "\"Nope\" isn't an active income account".into())]);
    assert_eq!(list_products(&pool).await.unwrap().len(), 1, "nothing saved");

    let done = paste_items(&pool, &[row("Nut", "0.25", "0.10"), row("bolt", "0.60", ""), PastedItem { name: "Washer".into(), income_account: Some("4100".into()), purchase_description: Some("Zinc washer".into()), ..Default::default() }]).await.unwrap();
    assert_eq!((done.added, done.updated, done.problems.len()), (2, 1, 0));
    let items = list_products(&pool).await.unwrap();
    let get = |n: &str| items.iter().find(|p| p.name == n).unwrap();
    assert_eq!((get("Nut").rate_cents, get("Nut").purchase_cost_cents), (25, Some(10)));
    assert_eq!((get("Bolt").rate_cents, get("Bolt").purchase_cost_cents, get("Bolt").purchase_description.as_deref()), (60, Some(1_200), Some("Hex bolt, box of 100")), "an empty cell leaves the value");
    assert_eq!(get("Washer").purchase_description.as_deref(), Some("Zinc washer"));

    // Several at once made inactive, then active again.
    let ids: Vec<i64> = items.iter().map(|p| p.id).collect();
    assert_eq!(set_items_active(&pool, &ids, false).await.unwrap(), 3);
    assert!(list_products(&pool).await.unwrap().iter().all(|p| !p.is_active));
    assert_eq!(set_items_active(&pool, &ids[..1], true).await.unwrap(), 1);
    assert!(set_items_active(&pool, &[], true).await.unwrap_err().starts_with("ITEM_NONE_TICKED"));
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
