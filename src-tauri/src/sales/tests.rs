use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, record_payment, InvoiceInput, InvoiceLineInput, PaymentInput};

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

fn line(product: Option<i64>, qty: i64, rate: i64, account: i64) -> SalesLineInput {
    SalesLineInput { product_service_id: product, description: "Item".into(), quantity_e4: qty * 10_000, rate_cents: rate, income_account_id: account, class_id: None, is_taxable: false }
}

#[tokio::test]
async fn sales_receipt_debits_cash_and_credits_revenue() {
    let pool = test_pool().await;
    let (checking, sales, services) = (account_id(&pool, "1010").await, account_id(&pool, "4000").await, account_id(&pool, "4100").await);
    let widget = create_product(
        &pool,
        ProductInput { name: "Widget".into(), description: Some("Blue widget".into()), rate_cents: 1_999, income_account_id: sales, is_taxable: true },
    )
    .await
    .expect("product saved");

    let receipt = post_sales_receipt(
        &pool,
        SalesReceiptInput {
            customer_name: Some("Walk-in Wanda".into()),
            receipt_date: d(12),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            reference: None,
            memo: Some("Counter sale".into()),
            lines: vec![line(Some(widget.id), 3, 1_999, sales), line(None, 1, 5_000, services)],
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .expect("receipt posts");

    assert_eq!((receipt.receipt_number, receipt.total_cents), (1, 10_997));
    assert_eq!(receipt.customer_name.as_deref(), Some("Walk-in Wanda"));
    assert_eq!(balance(&pool, checking).await, 10_997);
    assert_eq!(balance(&pool, sales).await, 5_997);
    assert_eq!(balance(&pool, services).await, 5_000);
    assert_eq!(list_customers(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sales_receipt_rejects_non_income_lines_and_rolls_back() {
    let pool = test_pool().await;
    let (checking, rent) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await);
    let err = post_sales_receipt(
        &pool,
        SalesReceiptInput {
            customer_name: Some("Nobody".into()),
            receipt_date: d(12),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            reference: None,
            memo: None,
            lines: vec![line(None, 1, 5_000, rent)],
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .expect_err("expense account rejected");
    assert!(err.starts_with("SR_NOT_INCOME_ACCOUNT"), "{err}");
    let (receipts, entries, customers): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM sales_receipts), (SELECT COUNT(*) FROM journal_entries), (SELECT COUNT(*) FROM customers)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((receipts, entries, customers), (0, 0, 0));

    let err = create_product(&pool, ProductInput { name: "Bad".into(), description: None, rate_cents: 100, income_account_id: rent, is_taxable: true })
        .await
        .expect_err("product must map to income");
    assert!(err.starts_with("PRODUCT_NOT_INCOME_ACCOUNT"), "{err}");
}

#[tokio::test]
async fn all_sales_lists_invoices_payments_and_receipts() {
    let pool = test_pool().await;
    let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "acme corp".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: 40_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .unwrap();
    create_customer(&pool, "Acme Corp", None).await.expect_err("duplicate (case-insensitive) customer");
    record_payment(
        &pool,
        PaymentInput { invoice_id: invoice.id, deposit_account_id: checking, payment_date: d(5), amount_cents: 15_000, payment_method: "ACH".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .unwrap();
    post_sales_receipt(
        &pool,
        SalesReceiptInput {
            customer_name: None,
            receipt_date: d(6),
            deposit_account_id: checking,
            payment_method: "CREDIT_CARD".into(),
            reference: None,
            memo: None,
            lines: vec![line(None, 2, 2_500, services)],
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .unwrap();

    let rows = list_all_sales(&pool).await.unwrap();
    let kinds: Vec<(&str, i64, i64)> = rows.iter().map(|r| (r.kind.as_str(), r.total_cents, r.balance_cents)).collect();
    assert_eq!(kinds, vec![("SALES_RECEIPT", 5_000, 0), ("PAYMENT", 15_000, 0), ("INVOICE", 40_000, 25_000)]);

    let customers = list_customers(&pool).await.unwrap();
    assert_eq!(customers.len(), 1);
    assert_eq!(customers[0].open_balance_cents, 25_000);
}
