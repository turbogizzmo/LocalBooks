use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, day).unwrap()
}

#[test]
fn tax_rounds_half_up() {
    assert_eq!(tax_on(10_000, 82_500), Some(825));
    assert_eq!(tax_on(1_000, 82_500), Some(83), "82.5 cents rounds up");
    assert_eq!(tax_on(0, 82_500), Some(0));
}

#[tokio::test]
async fn sales_tax_is_charged_reported_and_paid() {
    let pool = test_pool().await;
    let (checking, ar, payable, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "2200").await, account(&pool, "4100").await);
    let rate = create_tax_rate(&pool, TaxRateInput { name: "State 8.25%".into(), rate_ppm: 82_500, agency_name: Some("Dept. of Revenue".into()), liability_account_id: None }).await.unwrap();
    assert_eq!((rate.liability_account_id, rate.agency_name.as_deref()), (payable, Some("Dept. of Revenue")));

    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": rate.id,
        "lines": [
            { "description": "Widgets", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": true },
            { "description": "Labor", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services, "isTaxable": false }
        ]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!(inv.total_cents, 15_825);
    assert_eq!((balance(&pool, ar).await, balance(&pool, payable).await), (-15_825, 825));

    let receipt: crate::sales::SalesReceiptInput = serde_json::from_value(serde_json::json!({
        "customerName": null, "receiptDate": "2026-07-02", "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null, "taxRateId": rate.id,
        "lines": [{ "productServiceId": null, "description": "Widget", "quantityE4": 10_000, "rateCents": 2_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    assert_eq!(crate::sales::post_sales_receipt(&pool, receipt).await.unwrap().total_cents, 2_165);

    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            customer_name: "Acme".into(),
            credit_date: d(3),
            memo: None,
            refund_account_id: None,
            tax_rate_id: Some(rate.id),
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: 1_000, income_account_id: services, class_id: None, is_taxable: true }],
        },
    )
    .await
    .unwrap();
    assert_eq!(memo.total_cents, 1_083);
    assert_eq!(balance(&pool, payable).await, 825 + 165 - 83);

    // An estimate keeps its tax when it becomes an invoice.
    let estimate: crate::estimates::EstimateInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "estimateDate": "2026-07-04", "expirationDate": null, "memo": null, "taxRateId": rate.id,
        "lines": [{ "productServiceId": null, "description": "Install", "quantityE4": 10_000, "rateCents": 4_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    let est = crate::estimates::create_estimate(&pool, estimate).await.unwrap();
    assert_eq!((est.total_cents, est.tax_cents), (4_330, 330));
    let converted = crate::estimates::convert_to_invoice(&pool, est.id, d(5), d(30)).await.unwrap();
    let invoice_total: i64 = sqlx::query_scalar("SELECT total_cents FROM invoices WHERE id = ?1").bind(converted.converted_to_invoice_id).fetch_one(&pool).await.unwrap();
    assert_eq!(invoice_total, 4_330);

    let report = tax_liability(&pool, d(1), d(31)).await.unwrap();
    let row = &report.rows[0];
    assert_eq!((row.taxable_sales_cents, row.non_taxable_sales_cents, row.tax_collected_cents), (10_000 + 2_000 - 1_000 + 4_000, 5_000, 825 + 165 - 83 + 330));
    assert_eq!(report.owed_cents, 1_237);

    pay_sales_tax(&pool, SalesTaxPaymentInput { tax_rate_id: Some(rate.id), liability_account_id: None, account_id: checking, payment_date: d(31), amount_cents: 1_237, reference: Some("Q3".into()) }).await.unwrap();
    let after = tax_liability(&pool, d(1), d(31)).await.unwrap();
    assert_eq!((after.paid_cents, after.owed_cents), (1_237, 0));
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
