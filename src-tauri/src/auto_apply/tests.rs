use super::*;
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn invoice(pool: &SqlitePool, customer: &str, issue: &str, due: &str, cents: i64) -> i64 {
    let services = account(pool, "4100").await;
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": customer, "issueDate": issue, "dueDate": due,
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
}

#[test]
fn amounts_spread_in_order_up_to_each_balance() {
    let open = [(1, 3_000), (2, 5_000), (3, 1_000)];
    assert_eq!(in_order(&open, 6_000), vec![Allocation { document_id: 1, amount_cents: 3_000 }, Allocation { document_id: 2, amount_cents: 3_000 }]);
    assert_eq!(in_order(&open, 20_000).iter().map(|a| a.amount_cents).sum::<i64>(), 9_000, "an overpayment is not allocated");
    assert!(in_order(&open, 0).is_empty());
    assert!(in_order(&open, -5).is_empty());
}

#[tokio::test]
async fn customer_payments_apply_to_the_oldest_due_invoices_unless_turned_off() {
    let pool = test_pool().await;
    let late = invoice(&pool, "Acme", "2026-03-01", "2026-03-31", 20_000).await;
    let early = invoice(&pool, "Acme", "2026-01-01", "2026-01-31", 10_000).await;
    invoice(&pool, "Globex", "2025-12-01", "2025-12-31", 50_000).await;

    let applied = customer_payment(&pool, "Acme", None, 15_000, false).await.unwrap();
    assert_eq!(applied, vec![Allocation { document_id: early, amount_cents: 10_000 }, Allocation { document_id: late, amount_cents: 5_000 }]);
    assert!(customer_payment(&pool, "Acme", Some("EUR"), 15_000, false).await.unwrap().is_empty(), "only invoices in the payment's currency");

    crate::preferences::set(&pool, AUTO_APPLY_PAYMENTS, "false").await.unwrap();
    assert!(customer_payment(&pool, "Acme", None, 15_000, false).await.unwrap().is_empty());
    assert_eq!(customer_payment(&pool, "Acme", None, 15_000, true).await.unwrap().len(), 2, "Auto Apply on request still applies");
}

#[tokio::test]
async fn pay_bills_uses_open_vendor_credits_first_when_set() {
    let pool = test_pool().await;
    let (checking, supplies) = (account(&pool, "1010").await, account(&pool, "6250").await);
    let bill = |number: &str, cents: i64| {
        let pool = pool.clone();
        let input: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
            "vendorName": "Staples", "billNumber": number, "billDate": "2026-03-01", "dueDate": "2026-03-31", "memo": null, "totalCents": cents,
            "lines": [{ "accountId": supplies, "amountCents": cents, "memo": null }]
        }))
        .unwrap();
        async move { crate::expenses::post_vendor_bill(&pool, input).await.unwrap().id }
    };
    let (first, second) = (bill("B-1", 10_000).await, bill("B-2", 4_000).await);
    crate::credits::create_vendor_credit(
        &pool,
        crate::credits::VendorCreditInput {
            vendor_name: "Staples".into(),
            reference: Some("RMA-7".into()),
            credit_date: d(3, 5),
            memo: None,
            lines: vec![crate::credits::VendorCreditLineInput { account_id: supplies, amount_cents: 12_000, memo: None, class_id: None, customer_id: None }],
        },
    )
    .await
    .unwrap();
    let pay = |bill_id: i64, cents: i64| BillPaymentInput { bill_id, payment_account_id: checking, payment_date: d(3, 20), amount_cents: cents, reference: None };

    // The 120.00 credit settles the first bill and 20.00 of the second; 20.00 is paid from the bank.
    let result = pay_bills(&pool, vec![pay(first, 10_000), pay(second, 4_000)]).await.unwrap();
    assert_eq!((result.payments, result.credits_cents), (1, 12_000));
    let bills = crate::expenses::list_bills(&pool).await.unwrap();
    for id in [first, second] {
        assert_eq!(bills.iter().find(|b| b.id == id).unwrap().balance_cents, 0, "bill {id} settled");
    }
    let paid: i64 = sqlx::query_scalar("SELECT SUM(amount_cents) FROM bill_payments WHERE is_voided = 0").fetch_one(&pool).await.unwrap();
    assert_eq!(paid, 2_000);

    // Turned off, Pay Bills pays the whole amount and leaves credits alone.
    crate::preferences::set(&pool, AUTO_APPLY_VENDOR_CREDITS, "false").await.unwrap();
    let third = bill("B-3", 3_000).await;
    let result = pay_bills(&pool, vec![pay(third, 3_000)]).await.unwrap();
    assert_eq!((result.payments, result.credits_cents), (1, 0));
}
