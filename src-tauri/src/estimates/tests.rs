use super::*;
use crate::db::test_pool;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

#[tokio::test]
async fn estimate_does_not_post_until_converted() {
    let pool = test_pool().await;
    let (services, ar) = (account_id(&pool, "4100").await, account_id(&pool, "1200").await);
    let line = |qty: i64, rate: i64| SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: qty * 10_000, rate_cents: rate, income_account_id: services, class_id: None, is_taxable: false };
    let estimate = create_estimate(
        &pool,
        EstimateInput { customer_name: "Initech".into(), estimate_date: d(1), expiration_date: Some(d(31)), memo: None, lines: vec![line(10, 12_500), line(1, 5_000)], tax_rate_id: None, discount_cents: 0, details: Default::default() },
    )
    .await
    .expect("estimate saves");
    assert_eq!((estimate.estimate_number, estimate.status.as_str(), estimate.total_cents), (1001, "PENDING", 130_000));

    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(entries, 0, "estimates are non-posting");

    set_estimate_status(&pool, estimate.id, "REJECTED").await.unwrap();
    assert!(convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_REJECTED"));
    set_estimate_status(&pool, estimate.id, "ACCEPTED").await.unwrap();

    let closed = convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.expect("converts");
    assert_eq!(closed.status, "CONVERTED");
    assert!(closed.invoice_number.is_some());
    let ar_balance: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
        .bind(ar)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ar_balance, 130_000);
    assert!(convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_CONVERTED"));
    assert!(set_estimate_status(&pool, estimate.id, "PENDING").await.is_err());
}

#[tokio::test]
async fn an_estimate_loaded_into_the_invoice_form_converts_only_when_the_invoice_posts() {
    let pool = test_pool().await;
    let services = account_id(&pool, "4100").await;
    let estimate = create_estimate(
        &pool,
        EstimateInput {
            customer_name: "Initech".into(),
            estimate_date: d(1),
            expiration_date: None,
            memo: None,
            lines: vec![SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: 100_000, rate_cents: 12_500, income_account_id: services, class_id: None, is_taxable: false }],
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    let detail = estimate_detail(&pool, estimate.id).await.unwrap();
    assert_eq!((detail.lines.len(), detail.lines[0].quantity_e4, detail.lines[0].rate_cents), (1, 100_000, 12_500));

    // The form's lines, edited before posting: 8 hours instead of 10.
    let form = |account: i64| InvoiceInput {
        customer_name: detail.estimate.customer_name.clone(),
        issue_date: d(5),
        due_date: d(30),
        lines: detail
            .lines
            .iter()
            .map(|l| InvoiceLineInput { description: l.description.clone(), quantity_e4: 80_000, rate_cents: l.rate_cents, income_account_id: account, product_service_id: l.product_service_id, class_id: None, is_taxable: false })
            .collect(),
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    // A failed post leaves the estimate open.
    let checking = account_id(&pool, "1010").await;
    assert!(invoice_from_estimate(&pool, estimate.id, form(checking)).await.is_err());
    assert_eq!(get_estimate(&pool, estimate.id).await.unwrap().status, "PENDING");

    let invoice = invoice_from_estimate(&pool, estimate.id, form(services)).await.unwrap();
    assert_eq!(invoice.total_cents, 100_000, "the edited quantity was posted");
    let closed = get_estimate(&pool, estimate.id).await.unwrap();
    assert_eq!((closed.status.as_str(), closed.invoice_number), ("CONVERTED", Some(invoice.invoice_number)));
    assert!(invoice_from_estimate(&pool, estimate.id, form(services)).await.unwrap_err().starts_with("EST_CONVERTED"));

    // The invoice must still be for the estimate's customer.
    let other = create_estimate(
        &pool,
        EstimateInput { customer_name: "Initech".into(), estimate_date: d(2), expiration_date: None, memo: None, lines: vec![SalesLineInput { product_service_id: None, description: "Audit".into(), quantity_e4: 10_000, rate_cents: 5_000, income_account_id: services, class_id: None, is_taxable: false }], tax_rate_id: None, discount_cents: 0, details: Default::default() },
    )
    .await
    .unwrap();
    // The converted estimate is kept exactly as quoted, linked to its invoice, and can't be removed.
    let kept = estimate_detail(&pool, estimate.id).await.unwrap();
    assert_eq!(kept.estimate.converted_to_invoice_id, Some(invoice.id));
    assert_eq!((kept.estimate.total_cents, kept.lines[0].quantity_e4), (125_000, 100_000), "the quote, not the edited invoice");
    let err = |r: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>| r.map_err(db_err).unwrap_err();
    assert!(err(sqlx::query("DELETE FROM estimate_lines WHERE estimate_id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
    assert!(err(sqlx::query("DELETE FROM estimates WHERE id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
    assert!(err(sqlx::query("UPDATE estimates SET memo = 'x' WHERE id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
    assert!(set_estimate_status(&pool, estimate.id, "PENDING").await.is_err());

    let elsewhere = InvoiceInput { customer_name: "Someone Else".into(), ..form(services) };
    assert!(invoice_from_estimate(&pool, other.id, elsewhere).await.unwrap_err().starts_with("EST_INVOICE_MISMATCH"));
    assert_eq!(get_estimate(&pool, other.id).await.unwrap().status, "PENDING");
}
