use super::*;
use crate::banking::{post_bank_deposit, DepositInput, DepositItemRef, DepositOtherLine};
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, get_summary, record_payment, PaymentInput};
use crate::ledger::void_transaction;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

async fn ar(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_RECEIVABLE' AND is_system = 1").fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

/// Every posted entry balances, the trial balance nets to zero, and the stored totals agree.
async fn books_balance(pool: &SqlitePool) {
    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT journal_entry_id FROM journal_lines GROUP BY journal_entry_id HAVING SUM(debit_cents) <> SUM(credit_cents))",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(unbalanced, 0);
    let report = crate::file_dentist::checkup(pool, 0).await.unwrap();
    let totals = report.checks.iter().find(|c| c.id == "TOTALS").expect("totals check");
    assert_eq!(totals.status, crate::file_dentist::CheckStatus::Pass, "{:?}", totals.details);
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
}

async fn invoice(pool: &SqlitePool, cents: i64) -> i64 {
    let services = account_id(pool, "4100").await;
    create_and_post_invoice(
        pool,
        InvoiceInput {
            customer_name: "Acme".into(),
            issue_date: d(1),
            due_date: d(15),
            lines: vec![InvoiceLineInput {
                description: "Work".into(),
                quantity_e4: QUANTITY_SCALE,
                rate_cents: cents,
                income_account_id: services,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap()
    .id
}

async fn pay(pool: &SqlitePool, invoice_id: i64, account: i64, cents: i64) -> i64 {
    record_payment(
        pool,
        PaymentInput {
            invoice_id,
            deposit_account_id: account,
            payment_date: d(5),
            amount_cents: cents,
            payment_method: "CHECK".into(),
            reference: Some("4417".into()),
            exchange_rate_e6: None,
        },
    )
    .await
    .unwrap();
    sqlx::query_scalar("SELECT MAX(id) FROM invoice_payments WHERE invoice_id = ?1").bind(invoice_id).fetch_one(pool).await.unwrap()
}

fn bounce(kind: &str, id: i64) -> BounceInput {
    BounceInput {
        kind: kind.into(),
        id,
        bounce_date: d(10),
        bank_fee_cents: 0,
        bank_fee_account_id: None,
        customer_fee_cents: 0,
        customer_fee_account_id: None,
        memo: None,
    }
}

#[tokio::test]
async fn a_bounced_payment_reopens_the_invoice_takes_the_bank_down_and_charges_fees() {
    let pool = test_pool().await;
    let (checking, bank_fees, services, ar) = (account_id(&pool, "1010").await, account_id(&pool, "6050").await, account_id(&pool, "4100").await, ar(&pool).await);
    let invoice_id = invoice(&pool, 50_000).await;
    let payment = pay(&pool, invoice_id, checking, 50_000).await;
    assert_eq!(get_summary(&pool, invoice_id).await.unwrap().status, "PAID");
    assert_eq!(balance(&pool, checking).await, 50_000);

    let listed = candidates(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].kind.as_str(), listed[0].bank_account_id, listed[0].amount_cents), ("PAYMENT", checking, 50_000));

    let recorded = record(
        &pool,
        BounceInput {
            bank_fee_cents: 2_500,
            bank_fee_account_id: Some(bank_fees),
            customer_fee_cents: 3_000,
            customer_fee_account_id: Some(services),
            memo: Some("NSF".into()),
            ..bounce("PAYMENT", payment)
        },
    )
    .await
    .expect("bounce posts");
    assert_eq!(recorded.amount_cents, 50_000);
    assert!(recorded.fee_invoice_number.is_some());

    // The invoice is open again; the bank is down by the check and the fee.
    let reopened = get_summary(&pool, invoice_id).await.unwrap();
    assert_eq!((reopened.status.as_str(), reopened.balance_cents, reopened.home_balance_cents), ("UNPAID", 50_000, 50_000));
    assert_eq!(balance(&pool, checking).await, -2_500);
    assert_eq!(balance(&pool, bank_fees).await, 2_500);
    // A/R holds the reopened invoice plus the returned-check charge.
    assert_eq!(balance(&pool, ar).await, 53_000);
    let fee_invoice: (i64, i64) = sqlx::query_as("SELECT total_cents, customer_id FROM invoices WHERE invoice_number = ?1")
        .bind(recorded.fee_invoice_number)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fee_invoice.0, 3_000);
    let party: String = sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(recorded.journal_entry_id).fetch_one(&pool).await.unwrap();
    assert_eq!(party, "Acme");
    books_balance(&pool).await;

    // As of before the bounce the invoice was paid; from the bounce date it is open.
    let before = crate::report_center::aging(&pool, false, d(9)).await.unwrap();
    assert!(before.items.iter().all(|i| i.number.as_deref() != Some(&reopened.invoice_number.to_string())));
    let after = crate::report_center::aging(&pool, false, d(10)).await.unwrap();
    assert!(after.items.iter().any(|i| i.number.as_deref() == Some(&reopened.invoice_number.to_string()) && i.open_cents == 50_000));

    // It can't bounce twice, and its payment, the invoice and the entry are protected while it stands.
    assert!(candidates(&pool).await.unwrap().is_empty());
    let twice = record(&pool, bounce("PAYMENT", payment)).await.unwrap_err();
    assert!(twice.starts_with("BOUNCE_UNAVAILABLE"), "{twice}");
    let payment_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoice_payments WHERE id = ?1").bind(payment).fetch_one(&pool).await.unwrap();
    let refused = void_transaction(&pool, payment_entry).await.unwrap_err();
    assert!(refused.starts_with("VOID_BOUNCED"), "{refused}");
    let invoice_entry = reopened.journal_entry_id.unwrap();
    let refused = void_transaction(&pool, invoice_entry).await.unwrap_err();
    assert!(refused.starts_with("VOID_BOUNCED"), "{refused}");
    let edit = crate::edits::replace_journal_entry(
        &pool,
        recorded.journal_entry_id,
        JournalEntry { id: None, date: d(10), description: "x".into(), lines: vec![JournalLine::debit(ar, 1), JournalLine::credit(checking, 1)], currency: None },
    )
    .await
    .unwrap_err();
    assert!(!edit.is_empty());

    // The customer pays again with a good check.
    pay(&pool, invoice_id, checking, 50_000).await;
    assert_eq!(get_summary(&pool, invoice_id).await.unwrap().status, "PAID");
    books_balance(&pool).await;
}

#[tokio::test]
async fn voiding_a_bounce_puts_everything_back() {
    let pool = test_pool().await;
    let (checking, bank_fees, services, ar) = (account_id(&pool, "1010").await, account_id(&pool, "6050").await, account_id(&pool, "4100").await, ar(&pool).await);
    let invoice_id = invoice(&pool, 20_000).await;
    let payment = pay(&pool, invoice_id, checking, 20_000).await;
    let recorded = record(
        &pool,
        BounceInput {
            bank_fee_cents: 1_500,
            bank_fee_account_id: Some(bank_fees),
            customer_fee_cents: 2_000,
            customer_fee_account_id: Some(services),
            ..bounce("PAYMENT", payment)
        },
    )
    .await
    .unwrap();

    void(&pool, recorded.id).await.expect("bounce voids");
    assert_eq!(get_summary(&pool, invoice_id).await.unwrap().status, "PAID");
    assert_eq!(balance(&pool, checking).await, 20_000);
    assert_eq!(balance(&pool, bank_fees).await, 0);
    assert_eq!(balance(&pool, ar).await, 0, "the fee invoice is voided with it");
    let fee_voided: bool = sqlx::query_scalar("SELECT is_voided FROM invoices WHERE invoice_number = ?1").bind(recorded.fee_invoice_number).fetch_one(&pool).await.unwrap();
    assert!(fee_voided);
    assert!(list(&pool).await.unwrap()[0].is_voided);
    let again = void(&pool, recorded.id).await.unwrap_err();
    assert!(again.starts_with("BOUNCE_ALREADY_VOIDED"), "{again}");
    // It can be recorded again after the void.
    assert_eq!(candidates(&pool).await.unwrap().len(), 1);
    books_balance(&pool).await;
}

#[tokio::test]
async fn voiding_the_bounce_entry_from_a_register_also_counts_the_payment_again() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let invoice_id = invoice(&pool, 10_000).await;
    let payment = pay(&pool, invoice_id, checking, 10_000).await;
    let recorded = record(&pool, bounce("PAYMENT", payment)).await.unwrap();
    void_transaction(&pool, recorded.journal_entry_id).await.unwrap();
    assert_eq!(get_summary(&pool, invoice_id).await.unwrap().status, "PAID");
    assert!(list(&pool).await.unwrap()[0].is_voided);
    books_balance(&pool).await;
}

#[tokio::test]
async fn a_payment_banked_through_a_deposit_bounces_out_of_that_bank() {
    let pool = test_pool().await;
    let (checking, savings, undeposited) = (account_id(&pool, "1010").await, account_id(&pool, "1020").await, account_id(&pool, "1300").await);
    let invoice_id = invoice(&pool, 40_000).await;
    let payment = pay(&pool, invoice_id, undeposited, 40_000).await;
    // Still in Undeposited Funds: void it instead.
    assert!(candidates(&pool).await.unwrap().is_empty());
    let early = record(&pool, bounce("PAYMENT", payment)).await.unwrap_err();
    assert!(early.starts_with("BOUNCE_UNAVAILABLE"), "{early}");

    let deposit = post_bank_deposit(
        &pool,
        DepositInput { bank_account_id: savings, deposit_date: d(6), memo: None, items: vec![DepositItemRef { kind: "PAYMENT".into(), id: payment }], other_lines: vec![] },
    )
    .await
    .unwrap();
    let too_soon = record(&pool, BounceInput { bounce_date: d(5), ..bounce("PAYMENT", payment) }).await.unwrap_err();
    assert!(too_soon.starts_with("BOUNCE_BEFORE_DEPOSIT"), "{too_soon}");
    let recorded = record(&pool, bounce("PAYMENT", payment)).await.unwrap();
    assert_eq!(recorded.bank_account_id, savings);
    assert_eq!(balance(&pool, savings).await, 0);
    assert_eq!(balance(&pool, undeposited).await, 0);
    assert_eq!(balance(&pool, checking).await, 0);

    let deposit_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1").bind(deposit).fetch_one(&pool).await.unwrap();
    let refused = void_transaction(&pool, deposit_entry).await.unwrap_err();
    assert!(refused.starts_with("VOID_BOUNCED"), "{refused}");
    books_balance(&pool).await;
}

#[tokio::test]
async fn other_funds_on_a_deposit_bounce_back_to_their_account() {
    let pool = test_pool().await;
    let (checking, interest, bank_fees, services) = (account_id(&pool, "1010").await, account_id(&pool, "7000").await, account_id(&pool, "6050").await, account_id(&pool, "4100").await);
    post_bank_deposit(
        &pool,
        DepositInput {
            bank_account_id: checking,
            deposit_date: d(3),
            memo: None,
            items: vec![],
            other_lines: vec![DepositOtherLine { account_id: interest, amount_cents: 7_000, memo: Some("Refund check".into()), class_id: None }],
        },
    )
    .await
    .unwrap();
    let listed = candidates(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, "DEPOSIT_LINE");

    let no_customer = record(&pool, BounceInput { customer_fee_cents: 1_000, customer_fee_account_id: Some(services), ..bounce("DEPOSIT_LINE", listed[0].id) })
        .await
        .unwrap_err();
    assert!(no_customer.starts_with("BOUNCE_FEE_NO_CUSTOMER"), "{no_customer}");
    let bad_fee_account = record(&pool, BounceInput { bank_fee_cents: 1_000, bank_fee_account_id: Some(services), ..bounce("DEPOSIT_LINE", listed[0].id) })
        .await
        .unwrap_err();
    assert!(bad_fee_account.starts_with("BOUNCE_FEE_ACCOUNT"), "{bad_fee_account}");

    record(&pool, BounceInput { bank_fee_cents: 1_000, bank_fee_account_id: Some(bank_fees), ..bounce("DEPOSIT_LINE", listed[0].id) }).await.unwrap();
    assert_eq!(balance(&pool, checking).await, -1_000);
    assert_eq!(balance(&pool, interest).await, 0);
    assert!(candidates(&pool).await.unwrap().is_empty());
    books_balance(&pool).await;
}

#[tokio::test]
async fn the_bounce_row_is_protected_by_the_schema() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let invoice_id = invoice(&pool, 10_000).await;
    let payment = pay(&pool, invoice_id, checking, 10_000).await;
    let recorded = record(&pool, bounce("PAYMENT", payment)).await.unwrap();
    let changed = sqlx::query("UPDATE bounced_checks SET amount_cents = 1 WHERE id = ?1").bind(recorded.id).execute(&pool).await.unwrap_err();
    assert!(changed.to_string().contains("BOUNCE_IMMUTABLE"), "{changed}");
    let voided = sqlx::query("UPDATE bounced_checks SET is_voided = 1 WHERE id = ?1").bind(recorded.id).execute(&pool).await.unwrap_err();
    assert!(voided.to_string().contains("BOUNCE_VOID_ENTRY_ACTIVE"), "{voided}");
    let deleted = sqlx::query("DELETE FROM bounced_checks").execute(&pool).await.unwrap_err();
    assert!(deleted.to_string().contains("BOUNCE_IMMUTABLE"), "{deleted}");
    // The invoice's amount paid can't drift from its payments less live bounces.
    let drift = sqlx::query("UPDATE invoices SET amount_paid_cents = 10000, status = 'PAID' WHERE id = ?1").bind(invoice_id).execute(&pool).await.unwrap_err();
    assert!(drift.to_string().contains("INV_PAID_MISMATCH"), "{drift}");
}
