use super::*;
use crate::db::test_pool;
use crate::money::Cents;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
}

fn line(account: i64, cents: i64) -> SalesLineInput {
    SalesLineInput {
        product_service_id: None,
        description: "Returned work".into(),
        quantity_e4: 10_000,
        rate_cents: Cents(cents),
        income_account_id: account,
        class_id: None,
        is_taxable: false,
        service_date: None,
    }
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
async fn aging_nets_unused_credit_but_counts_only_invoices() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    invoice(&pool, "Acme", services, 50_000).await;
    invoice(&pool, "Other Co", services, 10_000).await;
    create_credit_memo(
        &pool,
        CreditMemoInput {
            credit_number: None,
            form_lines: Vec::new(),
            customer_name: "acme".into(),
            credit_date: d(5),
            memo: None,
            refund_account_id: None,
            lines: vec![line(services, 20_000)],
            tax_rate_id: None,
            header: Default::default(),
        },
    )
    .await
    .unwrap();

    // Both invoices fell due on July 1 and the credit is dated June 5: all three are 61+ days old
    // on September 30, but the bucket holds two invoices, not three.
    let aging = crate::invoices::ar_aging(&pool, NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()).await.unwrap();
    let late = aging.buckets.iter().find(|b| b.key == "days61Plus").unwrap();
    assert_eq!((late.invoice_count, late.amount_cents), (2, 40_000));
    assert_eq!((aging.total_cents, aging.unused_credit_cents), (40_000, 20_000));
    assert_eq!(aging.buckets.iter().map(|b| b.invoice_count).sum::<i64>(), 2);
}

#[tokio::test]
async fn credit_memos_reduce_receivables_apply_to_invoices_and_refund() {
    let pool = test_pool().await;
    let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
    let inv = invoice(&pool, "Acme", services, 50_000).await;
    let other = invoice(&pool, "Other Co", services, 10_000).await;

    let memo = create_credit_memo(
        &pool,
        CreditMemoInput {
            credit_number: None,
            form_lines: Vec::new(),
            customer_name: "acme".into(),
            credit_date: d(5),
            memo: None,
            refund_account_id: None,
            lines: vec![line(services, 20_000)],
            tax_rate_id: None,
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!((memo.customer_name.as_str(), memo.remaining_cents), ("Acme", 20_000));
    assert_eq!(balance(&pool, ar).await, 40_000, "A/R: 50,000 + 10,000 invoiced less 20,000 credit");
    assert_eq!(balance(&pool, services).await, -40_000);

    let apply = |doc, cents| apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: doc, amount_cents: cents, applied_date: d(6) });
    assert!(apply(other, Cents(1_000)).await.unwrap_err().starts_with("CM_OTHER_CUSTOMER"));
    assert!(apply(inv, Cents(25_000)).await.unwrap_err().starts_with("CM_BAD_AMOUNT"));
    assert_eq!(apply(inv, Cents(15_000)).await.unwrap().remaining_cents, 5_000);
    assert_eq!(invoice_state(&pool, inv).await, (15_000, "PARTIAL".into(), 35_000));
    assert_eq!(balance(&pool, ar).await, 40_000, "applying a credit posts nothing");

    // The invoice can still be paid; the credit counts toward what is paid.
    let pay = crate::invoices::PaymentInput {
        discount_cents: Cents::ZERO,
        payment_method_id: None,
        invoice_id: inv,
        deposit_account_id: checking,
        payment_date: d(7),
        amount_cents: Cents(35_000),
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
    let refunded = refund_customer_credit(
        &pool,
        RefundCreditInput { credit_id: memo.id, account_id: checking, refund_date: d(8), amount_cents: Cents(5_000), reference: Some("Chk 101".into()) },
    )
    .await
    .unwrap();
    assert_eq!(refunded.remaining_cents, 0);
    assert_eq!(balance(&pool, ar).await, 10_000, "only the other customer's invoice is still owed");
    let refund_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_refunds").fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, refund_entry).await.unwrap();
    assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 5_000);

    let applications = list_customer_credit_applications(&pool, memo.id).await.unwrap();
    assert_eq!(applications.len(), 1);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
    let party: Option<String> =
        sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(memo_entry).fetch_one(&pool).await.unwrap();
    assert_eq!(party.as_deref(), Some("Acme"));
}

#[tokio::test]
async fn removing_a_credit_reopens_the_invoice_and_refund_receipts_credit_the_bank() {
    let pool = test_pool().await;
    let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
    let inv = invoice(&pool, "Acme", services, 30_000).await;
    let memo = create_credit_memo(
        &pool,
        CreditMemoInput {
            credit_number: None,
            form_lines: Vec::new(),
            customer_name: "Acme".into(),
            credit_date: d(5),
            memo: None,
            refund_account_id: None,
            lines: vec![line(services, 30_000)],
            tax_rate_id: None,
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: inv, amount_cents: Cents(30_000), applied_date: d(6) }).await.unwrap();
    assert_eq!(invoice_state(&pool, inv).await, (30_000, "PAID".into(), 0));
    let app = list_customer_credit_applications(&pool, memo.id).await.unwrap()[0].id;
    remove_customer_credit(&pool, app).await.unwrap();
    assert_eq!(invoice_state(&pool, inv).await, (0, "UNPAID".into(), 30_000));
    assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 30_000);

    // A refund receipt pays the customer back at once: no credit is left to apply.
    let before = balance(&pool, checking).await;
    let receipt = create_credit_memo(
        &pool,
        CreditMemoInput {
            credit_number: None,
            form_lines: Vec::new(),
            customer_name: "Walk-in".into(),
            credit_date: d(9),
            memo: None,
            refund_account_id: Some(checking),
            lines: vec![line(services, 4_500)],
            tax_rate_id: None,
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!((receipt.remaining_cents, balance(&pool, checking).await - before), (0, -4_500));
    assert_eq!(balance(&pool, ar).await, 0, "invoice 30,000 less the 30,000 credit memo; the refund receipt never touches A/R");
    let err = apply_customer_credit(&pool, ApplyCreditInput { credit_id: receipt.id, document_id: inv, amount_cents: Cents(100), applied_date: d(9) })
        .await
        .unwrap_err();
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
        VendorCreditInput {
            form_lines: Vec::new(),
            vendor_name: "staples".into(),
            reference: Some("RMA-7".into()),
            credit_date: d(3),
            memo: None,
            lines: vec![VendorCreditLineInput {
                account_id: supplies,
                amount_cents: Cents(12_000),
                memo: Some("Returned toner".into()),
                class_id: None,
                customer_id: None,
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(balance(&pool, ap).await, -28_000);
    assert_eq!(balance(&pool, supplies).await, 28_000);

    let applied = apply_vendor_credit(&pool, ApplyCreditInput { credit_id: credit.id, document_id: bill, amount_cents: Cents(10_000), applied_date: d(4) })
        .await
        .unwrap();
    assert_eq!(applied.remaining_cents, 2_000);
    let (paid, status): (i64, String) = sqlx::query_as("SELECT amount_paid_cents, status FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    assert_eq!((paid, status.as_str()), (10_000, "PARTIAL"));

    let refunded = refund_vendor_credit(
        &pool,
        RefundCreditInput { credit_id: credit.id, account_id: checking, refund_date: d(5), amount_cents: Cents(2_000), reference: None },
    )
    .await
    .unwrap();
    assert_eq!(refunded.remaining_cents, 0);
    assert_eq!(balance(&pool, ap).await, -30_000, "bill 40,000 less the credit's 10,000 applied; the 2,000 refunded came back to the bank");
    assert_eq!(balance(&pool, checking).await, 2_000);
    let app = list_vendor_credit_applications(&pool, credit.id).await.unwrap()[0].id;
    remove_vendor_credit(&pool, app).await.unwrap();
    let paid: i64 = sqlx::query_scalar("SELECT amount_paid_cents FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    assert_eq!(paid, 0);
}

#[tokio::test]
async fn an_invoice_takes_the_customers_open_credits_oldest_first() {
    let pool = test_pool().await;
    let (ar, services) = (account(&pool, "1200").await, account(&pool, "4100").await);
    let memo = |day, cents| {
        create_credit_memo(
            &pool,
            CreditMemoInput {
                credit_number: None,
                form_lines: Vec::new(),
                customer_name: "Acme".into(),
                credit_date: d(day),
                memo: None,
                refund_account_id: None,
                lines: vec![line(services, cents)],
                tax_rate_id: None,
                header: Default::default(),
            },
        )
    };
    let newer = memo(4, 30_000).await.unwrap();
    let older = memo(2, 10_000).await.unwrap();
    // Another customer's credit and a refund receipt are never used.
    create_credit_memo(
        &pool,
        CreditMemoInput {
            credit_number: None,
            form_lines: Vec::new(),
            customer_name: "Other Co".into(),
            credit_date: d(1),
            memo: None,
            refund_account_id: None,
            lines: vec![line(services, 5_000)],
            tax_rate_id: None,
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    let ar_before = balance(&pool, ar).await;

    let inv = invoice(&pool, "Acme", services, 25_000).await;
    let used = apply_open_credits_to_invoice(&pool, inv, d(6)).await.unwrap();
    assert_eq!((used.count, used.applied_cents), (2, 25_000));
    assert_eq!(invoice_state(&pool, inv).await, (25_000, "PAID".into(), 0));
    assert_eq!(credit_memo(&pool, older.id).await.unwrap().remaining_cents, 0, "the oldest credit goes first");
    assert_eq!(credit_memo(&pool, newer.id).await.unwrap().remaining_cents, 15_000);
    assert_eq!(balance(&pool, ar).await, ar_before + 25_000, "applying credits posts nothing");

    // Nothing left to pay, or no credit left: nothing is applied.
    assert_eq!(apply_open_credits_to_invoice(&pool, inv, d(6)).await.unwrap().count, 0);
    let second = invoice(&pool, "Other Co", services, 1_000).await;
    let used = apply_open_credits_to_invoice(&pool, second, d(6)).await.unwrap();
    assert_eq!((used.count, used.applied_cents), (1, 1_000));
}

#[test]
fn credit_amounts_arrive_as_bare_integers() {
    let apply: ApplyCreditInput = serde_json::from_str(r#"{"creditId":1,"documentId":2,"amountCents":12345,"appliedDate":"2026-06-01"}"#).unwrap();
    assert_eq!(apply.amount_cents, Cents(12_345));
    let refund: RefundCreditInput =
        serde_json::from_str(r#"{"creditId":1,"accountId":3,"refundDate":"2026-06-02","amountCents":500,"reference":null}"#).unwrap();
    assert_eq!(refund.amount_cents, Cents(500));
    let unapplied: UnappliedPaymentInput = serde_json::from_str(
        r#"{"customerName":"Acme","depositAccountId":3,"paymentDate":"2026-06-03","amountCents":2500,"paymentMethod":"CHECK","reference":null}"#,
    )
    .unwrap();
    assert_eq!(unapplied.amount_cents, Cents(2_500));
    let vendor: VendorCreditLineInput = serde_json::from_str(r#"{"accountId":4,"amountCents":700,"memo":null}"#).unwrap();
    assert_eq!(vendor.amount_cents, Cents(700));
    assert!(
        serde_json::from_str::<ApplyCreditInput>(r#"{"creditId":1,"documentId":2,"amountCents":1.5,"appliedDate":"2026-06-01"}"#).is_err(),
        "a fractional amount is refused"
    );
}
