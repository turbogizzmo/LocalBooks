//! Overpayments, prepayments and unapplied payments (T24).

use super::*;
use crate::banking::{list_undeposited, post_bank_deposit, DepositInput, DepositItemRef};
use crate::db::test_pool;
use crate::invoices::{receive_payment, PaymentInput, ReceivePaymentInput};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
}

async fn customer(pool: &SqlitePool, name: &str) {
    sqlx::query("INSERT INTO customers (name) VALUES (?1)").bind(name).execute(pool).await.unwrap();
}

async fn invoice(pool: &SqlitePool, customer: &str, services: i64, cents: i64) -> i64 {
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": customer, "issueDate": "2026-06-01", "dueDate": "2026-07-01",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
}

async fn status(pool: &SqlitePool, id: i64) -> String {
    sqlx::query_scalar("SELECT status FROM invoices WHERE id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

fn pay(invoice_id: i64, account: i64, cents: i64) -> PaymentInput {
    PaymentInput {
        invoice_id,
        deposit_account_id: account,
        payment_date: d(10),
        amount_cents: cents,
        payment_method: "CHECK".into(),
        payment_method_id: None,
        reference: Some("4471".into()),
        exchange_rate_e6: None,
        discount_cents: 0,
    }
}

fn extra(customer: &str, account: i64, cents: i64, keep_as: &str, to: Option<i64>) -> UnappliedPaymentInput {
    UnappliedPaymentInput {
        customer_name: customer.into(),
        deposit_account_id: account,
        payment_date: d(10),
        amount_cents: cents,
        payment_method: "CHECK".into(),
        payment_method_id: None,
        reference: Some("4471".into()),
        keep_as: Some(keep_as.into()),
        account_id: to,
    }
}

#[tokio::test]
async fn an_overpayment_is_kept_as_credit_deposited_and_used_on_a_later_invoice() {
    let pool = test_pool().await;
    let (checking, undeposited, ar, services) =
        (account(&pool, "1010").await, account(&pool, "1300").await, account(&pool, "1200").await, account(&pool, "4100").await);
    let first = invoice(&pool, "Acme", services, 50_000).await;

    // 600.00 received against a 500.00 invoice: the invoice is paid and 100.00 is kept as credit.
    let received = receive_payment(
        &pool,
        ReceivePaymentInput { payments: vec![pay(first, undeposited, 50_000)], unapplied: Some(extra("Acme", undeposited, 10_000, "CREDIT", None)) },
    )
    .await
    .unwrap();
    assert_eq!(received.invoice_count, 1);
    let credit_id = received.credit_id.expect("the extra is a credit");
    assert_eq!(status(&pool, first).await, "PAID");
    let credit = credit_memo(&pool, credit_id).await.unwrap();
    assert!(credit.is_payment);
    assert_eq!((credit.customer_name.as_str(), credit.total_cents, credit.remaining_cents), ("Acme", 10_000, 10_000));
    assert_eq!(balance(&pool, ar).await, -10_000, "the customer is owed 100.00");
    assert_eq!(balance(&pool, undeposited).await, 60_000);

    // Both parts wait in Undeposited Funds and go to the bank in one deposit.
    let waiting = list_undeposited(&pool).await.unwrap();
    let kinds: Vec<(&str, i64)> = waiting.iter().map(|i| (i.kind.as_str(), i.amount_cents)).collect();
    assert_eq!(kinds, vec![("CREDIT", 10_000), ("PAYMENT", 50_000)]);
    let items = waiting.iter().map(|i| DepositItemRef { kind: i.kind.clone(), id: i.id }).collect();
    post_bank_deposit(&pool, DepositInput { bank_account_id: checking, deposit_date: d(11), memo: None, items, other_lines: vec![], cash_back: None })
        .await
        .unwrap();
    assert_eq!((balance(&pool, undeposited).await, balance(&pool, checking).await), (0, 60_000));
    assert!(list_undeposited(&pool).await.unwrap().is_empty());

    // A deposited unapplied payment can't be voided until its deposit is.
    let entry = credit.journal_entry_id.unwrap();
    let err = crate::ledger::void_transaction(&pool, entry).await.unwrap_err();
    assert!(err.contains("VOID_DEPOSITED"), "{err}");

    // The next invoice takes the credit, as a credit memo's would be.
    let second = invoice(&pool, "Acme", services, 8_000).await;
    let used = apply_open_credits_to_invoice(&pool, second, d(12)).await.unwrap();
    assert_eq!((used.count, used.applied_cents), (1, 8_000));
    assert_eq!(status(&pool, second).await, "PAID");
    assert_eq!(credit_memo(&pool, credit_id).await.unwrap().remaining_cents, 2_000);
    assert_eq!(balance(&pool, ar).await, -2_000);

    // It is no sale: sales lines are unchanged, and the aging lists it as a payment.
    let lines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM credit_memo_lines").fetch_one(&pool).await.unwrap();
    assert_eq!(lines, 0);
    assert_eq!(balance(&pool, services).await, -58_000);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

#[tokio::test]
async fn a_prepayment_can_be_refunded_or_written_off_and_bad_choices_post_nothing() {
    let pool = test_pool().await;
    let (checking, ar, services, interest) =
        (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await, account(&pool, "7000").await);
    customer(&pool, "Newco").await;

    // A customer with no invoice at all pays 200.00, which goes straight back.
    let refunded = receive_payment(&pool, ReceivePaymentInput { payments: vec![], unapplied: Some(extra("Newco", checking, 20_000, "REFUND", Some(checking))) })
        .await
        .unwrap();
    let credit = credit_memo(&pool, refunded.credit_id.unwrap()).await.unwrap();
    assert_eq!((credit.remaining_cents, balance(&pool, checking).await, balance(&pool, ar).await), (0, 0, 0));

    // 30.00 too much is kept as income.
    let inv = invoice(&pool, "Newco", services, 10_000).await;
    receive_payment(
        &pool,
        ReceivePaymentInput { payments: vec![pay(inv, checking, 10_000)], unapplied: Some(extra("Newco", checking, 3_000, "WRITE_OFF", Some(interest))) },
    )
    .await
    .unwrap();
    assert_eq!((balance(&pool, interest).await, balance(&pool, ar).await, balance(&pool, checking).await), (-3_000, 0, 13_000));
    let write_offs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM credit_refunds WHERE is_write_off = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(write_offs, 1);

    // Wrong accounts, a missing account and a foreign-currency payment are refused, and the
    // invoice payment that came with them is not kept.
    let open = invoice(&pool, "Newco", services, 5_000).await;
    let with = |unapplied| receive_payment(&pool, ReceivePaymentInput { payments: vec![pay(open, checking, 5_000)], unapplied: Some(unapplied) });
    let err = with(extra("Newco", checking, 100, "WRITE_OFF", Some(checking))).await.unwrap_err();
    assert!(err.starts_with("CM_BAD_ACCOUNT"), "{err}");
    let err = with(extra("Newco", checking, 100, "REFUND", Some(interest))).await.unwrap_err();
    assert!(err.starts_with("CM_BAD_ACCOUNT"), "{err}");
    let err = with(extra("Newco", checking, 100, "REFUND", None)).await.unwrap_err();
    assert!(err.starts_with("INV_REFUND_ACCOUNT"), "{err}");
    let err = with(extra("Newco", checking, 0, "CREDIT", None)).await.unwrap_err();
    assert!(err.starts_with("INV_BAD_AMOUNT"), "{err}");
    assert_eq!(status(&pool, open).await, "UNPAID");
    let mut foreign = pay(open, checking, 5_000);
    foreign.exchange_rate_e6 = Some(1_000_000);
    let err = receive_payment(&pool, ReceivePaymentInput { payments: vec![foreign], unapplied: Some(extra("Newco", checking, 100, "CREDIT", None)) })
        .await
        .unwrap_err();
    assert!(err.starts_with("INV_FOREIGN_UNAPPLIED"), "{err}");

    // A credit kept earlier can be written off later from its own side.
    let kept = receive_payment(&pool, ReceivePaymentInput { payments: vec![], unapplied: Some(extra("Newco", checking, 700, "CREDIT", None)) })
        .await
        .unwrap()
        .credit_id
        .unwrap();
    let after = write_off_customer_credit(&pool, RefundCreditInput { credit_id: kept, account_id: interest, refund_date: d(20), amount_cents: 700, reference: None })
        .await
        .unwrap();
    assert_eq!(after.remaining_cents, 0);
    assert_eq!(balance(&pool, interest).await, -3_700);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

#[tokio::test]
async fn the_center_totals_unapplied_payments_and_credit_left_and_takes_only_undeposited_funds() {
    let pool = test_pool().await;
    let (checking, undeposited, services) = (account(&pool, "1010").await, account(&pool, "1300").await, account(&pool, "4100").await);
    customer(&pool, "Lakeview").await;
    let id: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Lakeview'").fetch_one(&pool).await.unwrap();
    receive_payment(&pool, ReceivePaymentInput { payments: vec![], unapplied: Some(extra("Lakeview", undeposited, 4_000, "CREDIT", None)) }).await.unwrap();
    let line = SalesLineInput { product_service_id: None, description: "Returned".into(), quantity_e4: 10_000, rate_cents: 1_500, income_account_id: services, class_id: None, is_taxable: false, service_date: None };
    create_credit_memo(&pool, CreditMemoInput { customer_name: "Lakeview".into(), credit_date: d(11), memo: None, refund_account_id: None, lines: vec![line], tax_rate_id: None, header: Default::default() })
        .await
        .unwrap();
    let credit = customer_credit(&pool, id).await.unwrap();
    assert_eq!((credit.unapplied_cents, credit.remaining_cents), (4_000, 1_500));

    // A current asset other than Undeposited Funds would never reach Make Deposits.
    let other: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Prepaid Rent', 'OTHER_CURRENT_ASSET') RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    let err = receive_payment(&pool, ReceivePaymentInput { payments: vec![], unapplied: Some(extra("Lakeview", other, 100, "CREDIT", None)) }).await.unwrap_err();
    assert!(err.starts_with("INV_BAD_DEPOSIT_ACCOUNT"), "{err}");
    assert!(receive_payment(&pool, ReceivePaymentInput { payments: vec![], unapplied: Some(extra("Lakeview", checking, 100, "CREDIT", None)) }).await.is_ok());
}
