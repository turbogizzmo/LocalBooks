use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
use chrono::NaiveDate;

async fn invoice(pool: &SqlitePool, customer: &str) -> i64 {
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap();
    let d = |day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
    create_and_post_invoice(
        pool,
        InvoiceInput {
            customer_name: customer.into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: 5_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn queue_then_send_records_history_and_dequeues() {
    let pool = test_pool().await;
    let a = invoice(&pool, "Acme").await;
    let b = invoice(&pool, "Initech").await;
    sqlx::query("UPDATE customers SET email = 'ap@acme.test' WHERE name = 'Acme'").execute(&pool).await.unwrap();

    set_queued(&pool, FormKind::Invoice, a, true).await.unwrap();
    set_queued(&pool, FormKind::Invoice, a, true).await.unwrap(); // idempotent
    set_queued(&pool, FormKind::Invoice, b, true).await.unwrap();
    let queued = queue(&pool).await.unwrap();
    assert_eq!(queued.len(), 2);
    assert_eq!((queued[0].customer_name.as_str(), queued[0].customer_email.as_deref()), ("Acme", Some("ap@acme.test")));
    assert_eq!(queued[1].customer_email, None);
    assert!(set_queued(&pool, FormKind::Estimate, 999, true).await.is_err());

    record(&pool, FormKind::Invoice, a, &["ap@acme.test".into(), "cfo@acme.test".into()], Method::Smtp, "Invoice 1001").await.unwrap();
    record(&pool, FormKind::Invoice, a, &["ap@acme.test".into()], Method::MailApp, "Invoice 1001 again").await.unwrap();
    let sent = history(&pool, FormKind::Invoice, a).await.unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!((sent[0].method.as_str(), sent[0].subject.as_str()), ("MAIL_APP", "Invoice 1001 again"));
    assert_eq!(sent[1].recipients, "ap@acme.test, cfo@acme.test");

    let status = statuses(&pool, FormKind::Invoice).await.unwrap();
    let sa = status.iter().find(|s| s.document_id == a).unwrap();
    assert_eq!((sa.send_count, sa.queued, sa.last_recipients.as_deref()), (2, false, Some("ap@acme.test")));
    let sb = status.iter().find(|s| s.document_id == b).unwrap();
    assert_eq!((sb.send_count, sb.queued, sb.last_sent_at.as_deref()), (0, true, None));
    assert_eq!(queue(&pool).await.unwrap().len(), 1, "a sent form leaves the queue");

    set_queued(&pool, FormKind::Invoice, b, false).await.unwrap();
    assert!(queue(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn history_is_append_only_and_leaves_the_ledger_alone() {
    let pool = test_pool().await;
    let a = invoice(&pool, "Acme").await;
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    record(&pool, FormKind::Invoice, a, &["ap@acme.test".into()], Method::Smtp, "Invoice").await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap(), entries);
    let edit = sqlx::query("UPDATE form_email_log SET recipients = 'x@y.test'").execute(&pool).await.unwrap_err();
    assert!(edit.to_string().contains("EMAIL_LOG_APPEND_ONLY"));
    assert!(sqlx::query("DELETE FROM form_email_log").execute(&pool).await.is_err());
}

#[tokio::test]
async fn sales_receipts_queue_and_statements_do_not() {
    let pool = test_pool().await;
    let id = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (services, checking) = (id("4100").await, id("1010").await);
    let receipt = crate::sales::post_sales_receipt(
        &pool,
        serde_json::from_value(serde_json::json!({ "customerName": "Globex", "receiptDate": "2026-10-05", "depositAccountId": checking,
            "paymentMethod": "CASH", "lines": [{ "description": "Workshop", "quantityE4": 10_000, "rateCents": 9_900, "incomeAccountId": services }] }))
        .unwrap(),
    )
    .await
    .unwrap();
    set_queued(&pool, FormKind::SalesReceipt, receipt.id, true).await.unwrap();
    let queued = queue(&pool).await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!((queued[0].kind, queued[0].number, queued[0].customer_name.as_str(), queued[0].total_cents), (FormKind::SalesReceipt, receipt.receipt_number, "Globex", 9_900));
    let customer: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Globex'").fetch_one(&pool).await.unwrap();
    assert!(set_queued(&pool, FormKind::Statement, customer, true).await.unwrap_err().starts_with("EMAIL_STATEMENT_QUEUE"));
    // Statements still keep a sent history, by customer.
    record(&pool, FormKind::Statement, customer, &["ap@globex.test".into()], Method::Smtp, "Statement").await.unwrap();
    assert_eq!(history(&pool, FormKind::Statement, customer).await.unwrap().len(), 1);
}

#[tokio::test]
async fn voided_invoices_cannot_be_queued() {
    let pool = test_pool().await;
    let a = invoice(&pool, "Acme").await;
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(a).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert!(set_queued(&pool, FormKind::Invoice, a, true).await.unwrap_err().starts_with("EMAIL_VOIDED"));
}
