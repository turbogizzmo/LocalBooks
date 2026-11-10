use super::*;
use serde_json::json;

async fn count(pool: &SqlitePool, sql: &str, id: i64) -> usize {
    sqlx::query_scalar::<_, i64>(sql).bind(id).fetch_one(pool).await.unwrap() as usize
}

/// One of each kind, written through the forms' own save paths.
async fn one_of_each(pool: &SqlitePool) -> Vec<(&'static str, i64)> {
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(pool).await.unwrap();
    let expense: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6000'").fetch_one(pool).await.unwrap();
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(pool).await.unwrap();
    let sale = json!([{ "description": "Design work", "quantityE4": 20_000, "rateCents": 7_500, "incomeAccountId": income },
                      { "description": "Travel", "quantityE4": 10_000, "rateCents": 2_000, "incomeAccountId": income }]);
    let input = |v: serde_json::Value| v;
    let receipt: crate::sales::SalesReceiptInput = serde_json::from_value(input(json!({
        "customerName": "Walk-in", "receiptDate": "2025-06-02", "depositAccountId": checking, "paymentMethod": "CHECK", "reference": "1201", "memo": "Counter sale", "lines": sale
    }))).unwrap();
    let estimate: crate::estimates::EstimateInput = serde_json::from_value(json!({
        "customerName": "Walk-in", "estimateDate": "2025-06-02", "expirationDate": null, "memo": "Quote", "lines": sale
    })).unwrap();
    let credit: crate::credits::CreditMemoInput = serde_json::from_value(json!({
        "customerName": "Walk-in", "creditDate": "2025-06-02", "memo": "Returned", "lines": sale
    })).unwrap();
    let vendor_credit: crate::credits::VendorCreditInput = serde_json::from_value(json!({
        "vendorName": "Paper Co", "reference": "CR-9", "creditDate": "2025-06-02", "memo": null,
        "lines": [{ "accountId": expense, "amountCents": 4_000, "memo": "Damaged reams" }]
    })).unwrap();
    crate::preferences::set(pool, crate::preferences::tax_time::INVENTORY, "true").await.unwrap();
    let job: i64 = sqlx::query_scalar("INSERT INTO customers (name) VALUES ('Harbor Office Refit') RETURNING id").fetch_one(pool).await.unwrap();
    let order: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Paper Co", "poDate": "2025-06-02", "memo": "Restock",
        "lines": [{ "accountId": expense, "description": "Paper", "quantityE4": 50_000, "rateCents": 900, "amountCents": 4_500, "customerId": job }]
    })).unwrap();
    vec![
        ("SALES_RECEIPT", crate::sales::post_sales_receipt(pool, receipt).await.unwrap().id),
        ("ESTIMATE", crate::estimates::create_estimate(pool, estimate).await.unwrap().id),
        ("CREDIT_MEMO", crate::credits::create_credit_memo(pool, credit).await.unwrap().id),
        ("VENDOR_CREDIT", crate::credits::create_vendor_credit(pool, vendor_credit).await.unwrap().id),
        ("PURCHASE_ORDER", crate::workflows::create_purchase_order(pool, order).await.unwrap()),
    ]
}

#[tokio::test]
async fn each_kind_copies_its_header_and_lines() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let saved = one_of_each(&pool).await;
    for (kind, id) in &saved {
        let copy = copy_source(&pool, kind, *id).await.unwrap();
        assert_eq!(copy.kind, *kind);
        let expected = if matches!(*kind, "VENDOR_CREDIT" | "PURCHASE_ORDER") { "Paper Co" } else { "Walk-in" };
        assert_eq!(copy.name.as_deref(), Some(expected), "{kind}");
        assert!(copy.number.is_some(), "{kind} names the original");
        assert!(copy.lines.iter().all(|l| l.amount_cents > 0 && l.account_id == l.income_account_id), "{kind}");
    }
    let copy = |kind: &'static str| {
        let pool = pool.clone();
        let id = saved.iter().find(|(k, _)| *k == kind).unwrap().1;
        async move { copy_source(&pool, kind, id).await.unwrap() }
    };
    let receipt = copy("SALES_RECEIPT").await;
    assert_eq!(
        receipt.lines.iter().map(|l| (l.description.as_str(), l.quantity_e4, l.rate_cents, l.amount_cents)).collect::<Vec<_>>(),
        [("Design work", Some(20_000), Some(7_500), 15_000), ("Travel", Some(10_000), Some(2_000), 2_000)]
    );
    assert_eq!((receipt.payment_method.as_deref(), receipt.reference.as_deref(), receipt.memo.as_deref()), (Some("CHECK"), Some("1201"), Some("Counter sale")));
    assert!(receipt.account_id.is_some() && receipt.details.is_some());
    assert_eq!(copy("ESTIMATE").await.lines.len(), 2);
    assert_eq!(copy("CREDIT_MEMO").await.memo.as_deref(), Some("Returned"));
    let vendor_credit = copy("VENDOR_CREDIT").await;
    assert_eq!((vendor_credit.reference.as_deref(), vendor_credit.lines[0].description.as_str(), vendor_credit.lines[0].amount_cents), (Some("CR-9"), "Damaged reams", 4_000));
    let order = copy("PURCHASE_ORDER").await;
    assert_eq!((order.memo.as_deref(), order.lines[0].quantity_e4, order.lines[0].rate_cents), (Some("Restock"), Some(50_000), Some(900)));
    let job: Option<i64> = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Harbor Office Refit'").fetch_optional(&pool).await.unwrap();
    assert_eq!(order.lines[0].customer_id, job, "the order line's job comes with the copy");

    assert!(copy_source(&pool, "INVOICE", 1).await.unwrap_err().starts_with("DUP_BAD_KIND"));
    assert!(copy_source(&pool, "ESTIMATE", 99_999).await.unwrap_err().starts_with("DUP_NOT_FOUND"));
}

#[tokio::test]
async fn a_deposit_copies_only_its_other_lines_and_a_transfer_its_accounts() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let account = |number: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(&pool).await.unwrap() }
    };
    let (checking, savings) = (account("1010").await, account("1020").await);
    let transfer = crate::banking::post_transfer(
        &pool,
        crate::banking::TransferInput { transfer_date: chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(), from_account_id: checking, to_account_id: savings, amount_cents: 50_000, memo: Some("To savings".into()) },
    )
    .await
    .unwrap();
    let copy = copy_source(&pool, "TRANSFER", transfer).await.unwrap();
    assert_eq!((copy.account_id, copy.to_account_id, copy.amount_cents, copy.memo.as_deref()), (Some(checking), Some(savings), Some(50_000), Some("To savings")));

    // Every deposit: its copy holds the lines that aren't a received payment or sales receipt.
    let deposits: Vec<i64> = sqlx::query_scalar("SELECT id FROM deposits").fetch_all(&pool).await.unwrap();
    for id in deposits {
        let copy = copy_source(&pool, "DEPOSIT", id).await.unwrap();
        let other = count(&pool, "SELECT COUNT(*) FROM deposit_lines WHERE deposit_id = ?1 AND is_voided = 0 AND invoice_payment_id IS NULL AND sales_receipt_id IS NULL", id).await;
        assert_eq!(copy.lines.len(), other);
        assert!(copy.account_id.is_some());
    }
}
