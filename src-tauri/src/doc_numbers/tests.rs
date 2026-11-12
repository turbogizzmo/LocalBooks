use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
use crate::money::Cents;
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
        lines: vec![InvoiceLineInput {
            description: "Work".into(),
            quantity_e4: 10_000,
            rate_cents: Cents::from(5_000),
            income_account_id: income,
            product_service_id: None,
            class_id: None,
            is_taxable: false,
            service_date: None,
        }],
        currency: None,
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
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
    let line = crate::sales::SalesLineInput {
        product_service_id: None,
        description: "Work".into(),
        quantity_e4: 10_000,
        rate_cents: Cents(2_000),
        income_account_id: income,
        class_id: None,
        is_taxable: false,
        service_date: None,
    };

    let estimate = crate::estimates::create_estimate(
        &pool,
        crate::estimates::EstimateInput {
            estimate_number: Some(77),
            customer_name: "Acme".into(),
            estimate_date: d(4),
            expiration_date: None,
            memo: None,
            lines: vec![line.clone()],
            tax_rate_id: None,
            discount_cents: Cents::ZERO,
            details: Default::default(),
        },
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
            discount_cents: Cents::ZERO,
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
async fn an_edited_invoice_keeps_its_number_and_a_duplicate_takes_the_next() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    let first = create_and_post_invoice(&pool, invoice(Some(1200), "Acme", income)).await.unwrap();
    let edited = crate::edits::replace_invoice(&pool, first.id, invoice(None, "Acme", income)).await.unwrap();
    let (number, voided): (i64, bool) =
        sqlx::query_as("SELECT invoice_number, is_voided FROM invoices WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!((number, voided), (1200, false), "the corrected invoice keeps the number");
    // The original keeps it too, voided and marked replaced; edited again, the number still holds.
    let (old_number, old_voided, replaced): (i64, bool, bool) =
        sqlx::query_as("SELECT invoice_number, is_voided, is_replaced FROM invoices WHERE id = ?1").bind(first.id).fetch_one(&pool).await.unwrap();
    assert_eq!((old_number, old_voided, replaced), (1200, true, true));
    let again = crate::edits::replace_invoice(&pool, edited, invoice(None, "Acme", income)).await.unwrap();
    let number: i64 = sqlx::query_scalar("SELECT invoice_number FROM invoices WHERE id = ?1").bind(again).fetch_one(&pool).await.unwrap();
    assert_eq!(number, 1200);

    // The number is still taken for a new invoice, and the next one carries on after it.
    let err = create_and_post_invoice(&pool, invoice(Some(1200), "Bayside", income)).await.unwrap_err();
    assert!(err.starts_with("DOC_NUMBER_TAKEN: Invoice #1200 is already used"), "{err}");
    let copy = create_and_post_invoice(&pool, invoice(None, "Acme", income)).await.unwrap();
    assert_eq!(copy.invoice_number, 1201, "a duplicate is a new invoice and takes the next number");

    // A replaced invoice can't be marked live again, and a live one can't be marked replaced.
    assert!(sqlx::query("UPDATE invoices SET is_replaced = 0 WHERE id = ?1").bind(first.id).execute(&pool).await.is_err());
    assert!(sqlx::query("UPDATE invoices SET is_replaced = 1 WHERE id = ?1").bind(copy.id).execute(&pool).await.is_err());
}

#[tokio::test]
async fn a_part_paid_invoice_keeps_its_number_and_its_payment_when_edited() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let mut input = invoice(None, "Acme", income);
    input.lines[0].rate_cents = Cents::from(10_000);
    let first = create_and_post_invoice(&pool, input).await.unwrap();
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput {
            invoice_id: first.id,
            deposit_account_id: checking,
            payment_date: d(10),
            amount_cents: Cents::from(4_000),
            payment_method: "CHECK".into(),
            payment_method_id: None,
            reference: None,
            exchange_rate_e6: None,
            discount_cents: Cents::ZERO,
        },
    )
    .await
    .unwrap();
    let mut changed = invoice(None, "Acme", income);
    changed.lines[0].rate_cents = Cents::from(12_000);
    let edited = crate::edits::replace_invoice(&pool, first.id, changed).await.unwrap();
    let after = crate::invoices::get_summary(&pool, edited).await.unwrap();
    assert_eq!((after.invoice_number, after.amount_paid_cents, after.status.as_str()), (first.invoice_number, 4_000, "PARTIAL"));
    // The payment, Receive Payments and the customer's open invoices all name the same number.
    let paid_on: i64 = sqlx::query_scalar("SELECT i.invoice_number FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id WHERE i.is_voided = 0")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(paid_on, first.invoice_number);
}

#[tokio::test]
async fn a_changed_sales_order_keeps_its_number() {
    let pool = test_pool().await;
    let income = services(&pool).await;
    let line = crate::sales_orders::SalesOrderLineInput {
        id: None,
        line: crate::sales::SalesLineInput {
            product_service_id: None,
            description: "Work".into(),
            quantity_e4: 10_000,
            rate_cents: Cents(2_000),
            income_account_id: income,
            class_id: None,
            is_taxable: false,
            service_date: None,
        },
    };
    let order = |number: Option<i64>, lines: Vec<crate::sales_orders::SalesOrderLineInput>| crate::sales_orders::SalesOrderInput {
        customer_name: "Acme".into(),
        order_number: number,
        order_date: d(4),
        expected_date: None,
        memo: None,
        lines,
        tax_rate_id: None,
        sales_rep_id: None,
        ship_method_id: None,
        customer_message_id: None,
        header: Default::default(),
    };
    let saved = crate::sales_orders::create(&pool, &order(Some(30), vec![line.clone()])).await.unwrap();
    assert_eq!(saved.order_number, 30);
    // A number sent with a change is ignored: the order keeps its own.
    let changed = crate::sales_orders::update(&pool, saved.id, &order(Some(99), vec![line.clone(), line])).await.unwrap();
    assert_eq!((changed.id, changed.order_number), (saved.id, 30));
}
