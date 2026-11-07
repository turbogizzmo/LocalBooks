use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn a_vendors_last_check_or_bill_is_recalled_with_its_lines() {
    let pool = test_pool().await;
    let (checking, rent, supplies) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "6250").await);
    assert_eq!(last_for_vendor(&pool, "Harbor Hardware").await.unwrap(), None);

    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Harbor Hardware", "billNumber": "H-1", "billDate": "2026-03-01", "dueDate": "2026-03-31", "memo": null, "totalCents": 7_000,
        "lines": [{ "accountId": supplies, "amountCents": 7_000, "memo": "Screws" }]
    }))
    .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    let check: crate::expenses::ExpenseInput = serde_json::from_value(serde_json::json!({
        "kind": "CHECK", "payeeName": "Harbor Hardware", "paymentAccountId": checking, "expenseDate": "2026-04-01", "checkNumber": "101",
        "paymentMethod": null, "memo": null,
        "lines": [{ "accountId": rent, "amountCents": 5_000, "memo": "April" }, { "accountId": supplies, "amountCents": 1_250, "memo": null }]
    }))
    .unwrap();
    crate::expenses::post_expense(&pool, check).await.unwrap();

    let last = last_for_vendor(&pool, "harbor hardware").await.unwrap().unwrap();
    assert_eq!((last.kind.as_str(), last.date.as_str()), ("EXPENSE", "2026-04-01"));
    assert_eq!(last.lines.iter().map(|l| (l.account_id, l.amount_cents)).collect::<Vec<_>>(), [(rent, 5_000), (supplies, 1_250)]);
    assert_eq!(last.lines[0].memo.as_deref(), Some("April"));
}
