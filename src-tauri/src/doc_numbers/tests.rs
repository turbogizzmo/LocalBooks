use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
use chrono::NaiveDate;
use sqlx::SqlitePool;

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, day).unwrap()
}

async fn services(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap()
}

fn invoice(number: Option<i64>, customer: &str, income: i64) -> InvoiceInput {
    InvoiceInput {
        invoice_number: number,
        customer_name: customer.into(),
        issue_date: d(4),
        due_date: d(30),
        lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: 5_000, income_account_id: income, product_service_id: None, class_id: None, is_taxable: false, service_date: None }],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    }
}

async fn next(pool: &SqlitePool, kind: DocKind) -> i64 {
    next_number(&mut pool.acquire().await.unwrap(), kind).await.unwrap()
}

#[tokio::test]
async fn the_form_shows_the_number_it_will_get_and_a_typed_one_is_kept() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    assert_eq!(next(&pool, DocKind::Invoice).await, 1001);
    assert_eq!(next(&pool, DocKind::Estimate).await, 1001);
    assert_eq!(next(&pool, DocKind::SalesReceipt).await, 1);
    assert_eq!(next(&pool, DocKind::CreditMemo).await, 1);
    assert_eq!(next(&pool, DocKind::SalesOrder).await, 1);

    // Left as shown, the number is the next one.
    let first = create_and_post_invoice(&pool, invoice(None, "Acme", income)).await.unwrap();
    assert_eq!(first.invoice_number, 1001);
    // A typed number is used as typed, and numbering carries on from the highest.
    let typed = create_and_post_invoice(&pool, invoice(Some(2500), "Acme", income)).await.unwrap();
    assert_eq!(typed.invoice_number, 2500);
    assert_eq!(next(&pool, DocKind::Invoice).await, 2501);
    // A gap below the highest can be filled by typing it.
    let gap = create_and_post_invoice(&pool, invoice(Some(1500), "Acme", income)).await.unwrap();
    assert_eq!(gap.invoice_number, 1500);
    assert_eq!(next(&pool, DocKind::Invoice).await, 2501);
}

#[tokio::test]
async fn a_number_already_used_is_refused_with_the_next_free_one() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    create_and_post_invoice(&pool, invoice(Some(1005), "Acme", income)).await.unwrap();
    let err = create_and_post_invoice(&pool, invoice(Some(1005), "Bayside", income)).await.unwrap_err();
    assert_eq!(err, "DOC_NUMBER_TAKEN: Invoice #1005 is already used (Acme, dated 03/04/2026). The next free number is #1006.");
    // Nothing was saved for Bayside, and taking the offered number works.
    let customers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!(customers, 1);
    let taken = create_and_post_invoice(&pool, invoice(Some(1006), "Bayside", income)).await.unwrap();
    assert_eq!(taken.invoice_number, 1006);

    // A voided invoice still holds its number.
    crate::ledger::void_transaction(&pool, taken.journal_entry_id.unwrap()).await.unwrap();
    let err = create_and_post_invoice(&pool, invoice(Some(1006), "Bayside", income)).await.unwrap_err();
    assert!(err.starts_with("DOC_NUMBER_TAKEN: Invoice #1006 is already used"), "{err}");
}

#[tokio::test]
async fn numbers_out_of_range_are_refused() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    for bad in [0, -3, MAX_DOC_NUMBER + 1] {
        let err = create_and_post_invoice(&pool, invoice(Some(bad), "Acme", income)).await.unwrap_err();
        assert!(err.starts_with("DOC_NUMBER_RANGE:"), "{bad}: {err}");
    }
    create_and_post_invoice(&pool, invoice(Some(MAX_DOC_NUMBER), "Acme", income)).await.unwrap();
    // The next number isn't capped: numbering carries on past the highest a person may type.
    let after = create_and_post_invoice(&pool, invoice(None, "Acme", income)).await.unwrap();
    assert_eq!(after.invoice_number, MAX_DOC_NUMBER + 1);
}

#[tokio::test]
async fn each_sales_form_takes_a_typed_number() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let line = crate::sales::SalesLineInput { product_service_id: None, description: "Work".into(), quantity_e4: 10_000, rate_cents: 2_000, income_account_id: income, class_id: None, is_taxable: false, service_date: None };

    let estimate = crate::estimates::create_estimate(
        &pool,
        crate::estimates::EstimateInput { estimate_number: Some(77), customer_name: "Acme".into(), estimate_date: d(4), expiration_date: None, memo: None, lines: vec![line.clone()], tax_rate_id: None, discount_cents: 0, details: Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(estimate.estimate_number, 77);

    let receipt = crate::sales::post_sales_receipt(
        &pool,
        crate::sales::SalesReceiptInput {
            receipt_number: Some(40),
            customer_name: None,
            receipt_date: d(4),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            payment_method_id: None,
            reference: None,
            memo: None,
            lines: vec![line.clone()],
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(receipt.receipt_number, 40);
    assert_eq!(next(&pool, DocKind::SalesReceipt).await, 41);

    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            credit_number: Some(9),
            customer_name: "Acme".into(),
            credit_date: d(4),
            memo: None,
            refund_account_id: None,
            lines: vec![line.clone()],
            tax_rate_id: None,
            header: Default::default(),
            form_lines: Vec::new(),
        },
    )
    .await
    .unwrap();
    assert_eq!(memo.credit_number, 9);
    let err = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            credit_number: Some(9),
            customer_name: "Acme".into(),
            credit_date: d(5),
            memo: None,
            refund_account_id: None,
            lines: vec![line],
            tax_rate_id: None,
            header: Default::default(),
            form_lines: Vec::new(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, "DOC_NUMBER_TAKEN: Credit memo #9 is already used (Acme, dated 03/04/2026). The next free number is #10.");
}

#[tokio::test]
async fn an_edited_invoice_takes_the_next_number_and_a_duplicate_too() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    let first = create_and_post_invoice(&pool, invoice(Some(1200), "Acme", income)).await.unwrap();
    // The voided original keeps 1200, so the corrected one is posted under the next number.
    let edited = crate::edits::replace_invoice(&pool, first.id, invoice(None, "Acme", income)).await.unwrap();
    let number: i64 = sqlx::query_scalar("SELECT invoice_number FROM invoices WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(number, 1201);
    // A duplicate is a new invoice with no number of its own: it gets the next.
    let copy = create_and_post_invoice(&pool, invoice(None, "Acme", income)).await.unwrap();
    assert_eq!(copy.invoice_number, 1202);
}
