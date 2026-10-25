use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
}

fn line(account: i64, cents: i64) -> SalesLineInput {
    SalesLineInput { product_service_id: None, description: "Returned work".into(), quantity_e4: 10_000, rate_cents: cents, income_account_id: account, class_id: None, is_taxable: false }
}

async fn invoice(pool: &SqlitePool, customer: &str, services: i64, cents: i64) -> i64 {
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": customer, "issueDate": "2026-06-01", "dueDate": "2026-07-01",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
}

async fn invoice_state(pool: &SqlitePool, id: i64) -> (i64, String, i64) {
    sqlx::query_as("SELECT i.amount_paid_cents, i.status, b.home_open_cents FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id WHERE i.id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn credit_memos_reduce_receivables_apply_to_invoices_and_refund() {
    let pool = test_pool().await;
    let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
    let inv = invoice(&pool, "Acme", services, 50_000).await;
    let other = invoice(&pool, "Other Co", services, 10_000).await;

    let memo = create_credit_memo(&pool, CreditMemoInput { customer_name: "acme".into(), credit_date: d(5), memo: None, refund_account_id: None, lines: vec![line(services, 20_000)], tax_rate_id: None })
        .await
        .unwrap();
    assert_eq!((memo.customer_name.as_str(), memo.remaining_cents), ("Acme", 20_000));
    assert_eq!(balance(&pool, ar).await, 40_000, "A/R: 50,000 + 10,000 invoiced less 20,000 credit");
    assert_eq!(balance(&pool, services).await, -40_000);

    let apply = |doc, cents| apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: doc, amount_cents: cents, applied_date: d(6) });
    assert!(apply(other, 1_000).await.unwrap_err().starts_with("CM_OTHER_CUSTOMER"));
    assert!(apply(inv, 25_000).await.unwrap_err().starts_with("CM_BAD_AMOUNT"));
    assert_eq!(apply(inv, 15_000).await.unwrap().remaining_cents, 5_000);
    assert_eq!(invoice_state(&pool, inv).await, (15_000, "PARTIAL".into(), 35_000));
    assert_eq!(balance(&pool, ar).await, 40_000, "applying a credit posts nothing");

    // The invoice can still be paid; the credit counts toward what is paid.
    let pay = crate::invoices::PaymentInput {
        invoice_id: inv,
        deposit_account_id: checking,
        payment_date: d(7),
        amount_cents: 35_000,
        payment_method: "CHECK".into(),
        reference: None,
        exchange_rate_e6: None,
    };
    crate::invoices::record_payment(&pool, pay).await.unwrap();
    assert_eq!(invoice_state(&pool, inv).await, (50_000, "PAID".into(), 0));

    // An invoice with a credit applied cannot be voided; nor can a credit memo in use.
    let memo_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_memos WHERE id = ?1").bind(memo.id).fetch_one(&pool).await.unwrap();
    assert!(crate::ledger::void_transaction(&pool, memo_entry).await.unwrap_err().starts_with("VOID_CREDIT_USED"));

    // Refund the 5,000 left, then undo it by voiding the refund's entry.
    let refunded = refund_customer_credit(&pool, RefundCreditInput { credit_id: memo.id, account_id: checking, refund_date: d(8), amount_cents: 5_000, reference: Some("Chk 101".into()) })
        .await
        .unwrap();
    assert_eq!(refunded.remaining_cents, 0);
    assert_eq!(balance(&pool, ar).await, 10_000 + 0, "only the other customer's invoice is still owed");
    let refund_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_refunds").fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, refund_entry).await.unwrap();
    assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 5_000);

    let applications = list_customer_credit_applications(&pool, memo.id).await.unwrap();
    assert_eq!(applications.len(), 1);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
    let party: Option<String> = sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(memo_entry).fetch_one(&pool).await.unwrap();
    assert_eq!(party.as_deref(), Some("Acme"));
}

#[tokio::test]
async fn removing_a_credit_reopens_the_invoice_and_refund_receipts_credit_the_bank() {
    let pool = test_pool().await;
    let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
    let inv = invoice(&pool, "Acme", services, 30_000).await;
    let memo = create_credit_memo(&pool, CreditMemoInput { customer_name: "Acme".into(), credit_date: d(5), memo: None, refund_account_id: None, lines: vec![line(services, 30_000)], tax_rate_id: None })
        .await
        .unwrap();
    apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: inv, amount_cents: 30_000, applied_date: d(6) }).await.unwrap();
    assert_eq!(invoice_state(&pool, inv).await, (30_000, "PAID".into(), 0));
    let app = list_customer_credit_applications(&pool, memo.id).await.unwrap()[0].id;
    remove_customer_credit(&pool, app).await.unwrap();
    assert_eq!(invoice_state(&pool, inv).await, (0, "UNPAID".into(), 30_000));
    assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 30_000);

    // A refund receipt pays the customer back at once: no credit is left to apply.
    let before = balance(&pool, checking).await;
    let receipt = create_credit_memo(
        &pool,
        CreditMemoInput { customer_name: "Walk-in".into(), credit_date: d(9), memo: None, refund_account_id: Some(checking), lines: vec![line(services, 4_500)], tax_rate_id: None },
    )
    .await
    .unwrap();
    assert_eq!((receipt.remaining_cents, balance(&pool, checking).await - before), (0, -4_500));
    assert_eq!(balance(&pool, ar).await, 0, "invoice 30,000 less the 30,000 credit memo; the refund receipt never touches A/R");
    let err = apply_customer_credit(&pool, ApplyCreditInput { credit_id: receipt.id, document_id: inv, amount_cents: 100, applied_date: d(9) }).await.unwrap_err();
    assert!(err.starts_with("CM_NOT_AVAILABLE"), "{err}");
    let receipt_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_memos WHERE id = ?1").bind(receipt.id).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, receipt_entry).await.unwrap();
    assert!(list_credit_memos(&pool).await.unwrap().iter().any(|m| m.id == receipt.id && m.is_voided));
}

#[tokio::test]
async fn vendor_credits_reduce_payables_and_apply_to_bills() {
    let pool = test_pool().await;
    let (checking, ap, supplies) = (account(&pool, "1010").await, account(&pool, "2000").await, account(&pool, "6250").await);
    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Staples", "billNumber": "B-1", "billDate": "2026-06-01", "dueDate": "2026-07-01", "memo": null, "totalCents": 40_000,
        "lines": [{ "accountId": supplies, "amountCents": 40_000, "memo": null }]
    }))
    .unwrap();
    let bill = crate::expenses::post_vendor_bill(&pool, bill).await.unwrap().id;
    let credit = create_vendor_credit(
        &pool,
        VendorCreditInput { vendor_name: "staples".into(), reference: Some("RMA-7".into()), credit_date: d(3), memo: None, lines: vec![VendorCreditLineInput { account_id: supplies, amount_cents: 12_000, memo: Some("Returned toner".into()), class_id: None }] },
    )
    .await
    .unwrap();
    assert_eq!(balance(&pool, ap).await, -28_000);
    assert_eq!(balance(&pool, supplies).await, 28_000);

    let applied = apply_vendor_credit(&pool, ApplyCreditInput { credit_id: credit.id, document_id: bill, amount_cents: 10_000, applied_date: d(4) }).await.unwrap();
    assert_eq!(applied.remaining_cents, 2_000);
    let (paid, status): (i64, String) = sqlx::query_as("SELECT amount_paid_cents, status FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    assert_eq!((paid, status.as_str()), (10_000, "PARTIAL"));

    let refunded = refund_vendor_credit(&pool, RefundCreditInput { credit_id: credit.id, account_id: checking, refund_date: d(5), amount_cents: 2_000, reference: None }).await.unwrap();
    assert_eq!(refunded.remaining_cents, 0);
    assert_eq!(balance(&pool, ap).await, -30_000, "bill 40,000 less the credit's 10,000 applied; the 2,000 refunded came back to the bank");
    assert_eq!(balance(&pool, checking).await, 2_000);
    let app = list_vendor_credit_applications(&pool, credit.id).await.unwrap()[0].id;
    remove_vendor_credit(&pool, app).await.unwrap();
    let paid: i64 = sqlx::query_scalar("SELECT amount_paid_cents FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    assert_eq!(paid, 0);
}
