use super::*;
use crate::db::test_pool;
use serde_json::json;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn each_segment_ties_out_to_its_list_or_report() {
    let pool = test_pool().await;
    let (checking, card, rent, services) = (account(&pool, "1010").await, account(&pool, "2100").await, account(&pool, "6300").await, account(&pool, "4100").await);
    let mut tx = pool.begin().await.unwrap();
    let acme = crate::sales::find_or_create_named(&mut tx, crate::sales::Party::Customer, "Acme").await.unwrap();
    tx.commit().await.unwrap();

    // A billable check line (unbilled), a card charge, an estimate pending and one rejected.
    let expense = |kind: &str, from: i64, cents: i64, customer: Option<i64>| {
        serde_json::from_value::<crate::expenses::ExpenseInput>(json!({
            "kind": kind, "payeeName": "Supplier", "paymentAccountId": from, "expenseDate": "2026-04-01", "checkNumber": null, "paymentMethod": null, "memo": null,
            "lines": [{ "accountId": rent, "amountCents": cents, "memo": "Materials", "customerId": customer, "billable": customer.is_some() }]
        }))
        .unwrap()
    };
    crate::expenses::post_expense(&pool, expense("CHECK", checking, 12_000, Some(acme))).await.unwrap();
    crate::expenses::post_expense(&pool, expense("EXPENSE", card, 4_500, None)).await.unwrap();
    let estimate = |status_note: &str| {
        serde_json::from_value::<crate::estimates::EstimateInput>(json!({
            "customerName": "Acme", "estimateDate": "2026-04-02", "expirationDate": null, "memo": status_note,
            "lines": [{ "description": "Deck", "quantityE4": 10_000, "rateCents": 90_000, "incomeAccountId": services }]
        }))
        .unwrap()
    };
    crate::estimates::create_estimate(&pool, estimate("open")).await.unwrap();
    let rejected = crate::estimates::create_estimate(&pool, estimate("no")).await.unwrap();
    crate::estimates::set_estimate_status(&pool, rejected.id, "REJECTED").await.unwrap();

    let sales = sales_segments(&pool).await.unwrap();
    let seg = |s: &[BarSegment], key: &str| s.iter().find(|x| x.key == key).unwrap().clone();
    let unbilled = seg(&sales, "UNBILLED");
    let costs: i64 = crate::billable_costs::list_unbilled(&pool, None).await.unwrap().iter().map(|c| c.amount_cents).sum();
    let time: i64 = crate::billable_costs::list_unbilled_time(&pool, None).await.unwrap().iter().map(|t| t.amount_cents).sum();
    assert_eq!((unbilled.cents, unbilled.count), (costs + time, 1));
    assert_eq!((unbilled.items[0].kind, unbilled.items[0].customer_id, unbilled.items[0].amount_cents), ("COST", Some(acme), 12_000));
    let estimates = seg(&sales, "ESTIMATES");
    assert_eq!((estimates.cents, estimates.count), (90_000, 1), "the rejected estimate isn't counted");

    // Purchase orders: open ones only; card balances: the Balance Sheet's card lines.
    crate::preferences::set(&pool, crate::preferences::tax_time::INVENTORY, "true").await.unwrap();
    let order = |memo: &str| {
        serde_json::from_value::<crate::workflows::PurchaseOrderInput>(json!({
            "vendorName": "Supplier", "poDate": "2026-04-03", "memo": memo, "lines": [{ "accountId": rent, "description": "Paper", "amountCents": 3_000 }]
        }))
        .unwrap()
    };
    crate::workflows::create_purchase_order(&pool, order("keep")).await.unwrap();
    let closed = crate::workflows::create_purchase_order(&pool, order("close")).await.unwrap();
    crate::workflows::set_purchase_order_closed(&pool, closed, true).await.unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let bills = bill_segments(&pool, today).await.unwrap();
    let pos = seg(&bills, "PURCHASE_ORDERS");
    assert_eq!((pos.cents, pos.count, pos.items[0].memo.as_deref()), (3_000, 1, Some("keep")));
    let cards = seg(&bills, "CARDS");
    let bs = crate::statements::balance_sheet(&pool, today).await.unwrap();
    let card_line = bs.lines.iter().find(|l| l.account_id == Some(card)).and_then(|l| l.amount_cents).unwrap();
    assert_eq!((cards.cents, cards.count, cards.items[0].id), (card_line, 1, card));
    assert_eq!(card_line, 4_500);

    // A feature turned off drops its segment, as the screens drop its window.
    crate::preferences::set(&pool, crate::preferences::tax_time::INVENTORY, "false").await.unwrap();
    let keys: Vec<&str> = bill_segments(&pool, today).await.unwrap().iter().map(|s| s.key).collect();
    assert_eq!(keys, ["CARDS"]);
    sqlx::query("INSERT INTO system_preferences (key, value) VALUES (?1, 'false') ON CONFLICT (key) DO UPDATE SET value = 'false'")
        .bind(crate::sales_prefs::ESTIMATES)
        .execute(&pool)
        .await
        .unwrap();
    let keys: Vec<&str> = sales_segments(&pool).await.unwrap().iter().map(|s| s.key).collect();
    assert_eq!(keys, ["UNBILLED"]);
}
