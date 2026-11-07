use super::*;
use crate::db::test_pool;
use chrono::NaiveDate;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn edits_void_and_repost_and_respect_payments() {
    let pool = test_pool().await;
    let (checking, ar, services, rent) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await, account(&pool, "6300").await);
    let invoice = |cents: i64| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": "Acme", "issueDate": "2026-05-01", "dueDate": "2026-05-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
        .unwrap()
    };
    let original = crate::invoices::create_and_post_invoice(&pool, invoice(10_000)).await.unwrap();
    let edited = replace_invoice(&pool, original.id, invoice(12_500)).await.unwrap();
    assert_eq!(balance(&pool, ar).await, 12_500, "only the edited invoice counts");
    let voided: bool = sqlx::query_scalar("SELECT is_voided FROM invoices WHERE id = ?1").bind(original.id).fetch_one(&pool).await.unwrap();
    assert!(voided);
    let edits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transaction_edits").fetch_one(&pool).await.unwrap();
    assert_eq!(edits, 1);

    // Once paid, the edit can't go below what's paid.
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { payment_method_id: None, invoice_id: edited, deposit_account_id: checking, payment_date: NaiveDate::from_ymd_opt(2026, 5, 9).unwrap(), amount_cents: 100, payment_method: "CASH".into(), reference: None, exchange_rate_e6: None }).await.unwrap();
    let err = replace_invoice(&pool, edited, invoice(1)).await.unwrap_err();
    assert!(err.starts_with("EDIT_BELOW_PAID"), "{err}");
    assert_eq!(balance(&pool, ar).await, 12_400, "a refused edit changes nothing");

    let expense: ExpenseInput = serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-05-02",
        "checkNumber": "101", "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 90_000, "memo": "May" }] })).unwrap();
    let (expense_id, _) = { let mut tx = crate::ledger::begin_write(&pool).await.unwrap(); let r = crate::expenses::write_expense(&mut tx, &expense).await.unwrap(); crate::ledger::commit_write(tx).await.unwrap(); r };
    assert_eq!(document_lines(&pool, "expense", expense_id).await.unwrap()[0].memo.as_deref(), Some("May"));
    let old_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE id = ?1").bind(expense_id).fetch_one(&pool).await.unwrap();
    crate::workflows::add_attachment(&pool, old_entry, "receipt.pdf", b"%PDF-1.4").await.unwrap();
    let mut fixed = expense.clone();
    fixed.lines[0].amount_cents = 95_000;
    let new_expense = replace_expense(&pool, expense_id, fixed).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 95_000);
    let new_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE id = ?1").bind(new_expense).fetch_one(&pool).await.unwrap();
    let moved = crate::workflows::list_attachments(&pool, new_entry).await.unwrap();
    assert_eq!(moved.iter().map(|a| a.file_name.as_str()).collect::<Vec<_>>(), ["receipt.pdf"], "attachments follow the edit");
    assert!(crate::workflows::list_attachments(&pool, old_entry).await.unwrap().is_empty());

    let je = crate::ledger::post_journal_entry(&pool, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: "Accrual".into(),
        lines: vec![crate::ledger::JournalLine::debit(rent, 500), crate::ledger::JournalLine::credit(checking, 500)], currency: None }).await.unwrap();
    let new_je = replace_journal_entry(&pool, je, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: "Accrual".into(),
        lines: vec![crate::ledger::JournalLine::debit(rent, 700), crate::ledger::JournalLine::credit(checking, 700)], currency: None }).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 95_700);
    let invoice_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert!(replace_journal_entry(&pool, invoice_entry, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: String::new(), lines: vec![], currency: None })
        .await
        .unwrap_err()
        .starts_with("EDIT_NOT_MANUAL"));
    assert!(new_je > je);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
}

/// A/R or A/P in the general ledger (as a positive amount owed), and the aging report's total.
async fn control_and_aging(pool: &SqlitePool, payable: bool) -> (i64, i64) {
    let kind = if payable { "ACCOUNTS_PAYABLE" } else { "ACCOUNTS_RECEIVABLE" };
    let gl: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM v_posted_lines l JOIN chart_of_accounts a ON a.id = l.account_id WHERE a.account_type = ?1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap();
    let aging = crate::report_center::aging(pool, payable, date(30)).await.unwrap();
    (if payable { -gl } else { gl }, aging.total_cents)
}

async fn books_balance(pool: &SqlitePool) {
    let tb = crate::reports::trial_balance(pool).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents, "debits equal credits");
    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT journal_entry_id FROM journal_lines GROUP BY journal_entry_id HAVING SUM(debit_cents) <> SUM(credit_cents))",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(unbalanced, 0, "every entry balances");
    assert!(crate::ledger::verify_ledger_audit(pool).await.unwrap().intact);
}

async fn invoice_row(pool: &SqlitePool, id: i64) -> (i64, i64, String, bool) {
    sqlx::query_as("SELECT total_cents, amount_paid_cents, status, is_voided FROM invoices WHERE id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn editing_a_part_paid_invoice_keeps_its_payment_and_credit() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let invoice = |customer: &str, cents: i64| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": "2026-06-01", "dueDate": "2026-06-30",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
        .unwrap()
    };
    let original = crate::invoices::create_and_post_invoice(&pool, invoice("Acme", 100_000)).await.unwrap().id;
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { payment_method_id: None, invoice_id: original, deposit_account_id: checking, payment_date: date(5), amount_cents: 30_000, payment_method: "CHECK".into(), reference: Some("1001".into()), exchange_rate_e6: None }).await.unwrap();
    let memo: crate::credits::CreditMemoInput = serde_json::from_value(serde_json::json!({ "customerName": "Acme", "creditDate": "2026-06-06", "memo": null,
        "lines": [{ "productServiceId": null, "description": "Goodwill", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services }] })).unwrap();
    let memo = crate::credits::create_credit_memo(&pool, memo).await.unwrap().id;
    crate::credits::apply_customer_credit(&pool, crate::credits::ApplyCreditInput { credit_id: memo, document_id: original, amount_cents: 5_000, applied_date: date(6) }).await.unwrap();
    let (gl, aging) = control_and_aging(&pool, false).await;
    assert_eq!((gl, aging), (65_000, 65_000));

    // Raise the price: the payment and credit stay applied to the edited invoice.
    let edited = replace_invoice(&pool, original, invoice("Acme", 120_000)).await.unwrap();
    assert_eq!(invoice_row(&pool, edited).await, (120_000, 35_000, "PARTIAL".into(), false));
    assert_eq!(invoice_row(&pool, original).await, (100_000, 0, "UNPAID".into(), true));
    let on_new: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoice_payments WHERE invoice_id = ?1 AND is_voided = 0").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(on_new, 1);
    let open: i64 = sqlx::query_scalar("SELECT home_open_cents FROM v_invoice_balances WHERE invoice_id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(open, 85_000);
    assert_eq!(control_and_aging(&pool, false).await, (85_000, 85_000), "A/R agrees with the aging");
    books_balance(&pool).await;

    // The audit trail says what moved.
    let payload: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'EDIT' ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["document"], "INVOICE");
    assert_eq!(payload["oldDocumentId"], original);
    assert_eq!(payload["newDocumentId"], edited);
    let kinds: Vec<&str> = payload["relinked"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["PAYMENT", "CREDIT"]);

    // Lowering the total below what's applied is refused, and changes nothing.
    let err = replace_invoice(&pool, edited, invoice("Acme", 34_999)).await.unwrap_err();
    assert!(err.starts_with("EDIT_BELOW_PAID"), "{err}");
    assert!(err.contains("$350.00") && err.contains("$349.99"), "{err}");
    assert_eq!(invoice_row(&pool, edited).await, (120_000, 35_000, "PARTIAL".into(), false));
    // Exactly what's applied is fine: the edited invoice is paid in full.
    let paid_off = replace_invoice(&pool, edited, invoice("Acme", 35_000)).await.unwrap();
    assert_eq!(invoice_row(&pool, paid_off).await, (35_000, 35_000, "PAID".into(), false));
    assert_eq!(control_and_aging(&pool, false).await, (0, 0));

    // The payments came from Acme; they can't follow the invoice to another customer.
    let err = replace_invoice(&pool, paid_off, invoice("Globex", 50_000)).await.unwrap_err();
    assert!(err.starts_with("EDIT_PAID_PARTY"), "{err}");
    // The schema refuses a re-link that isn't part of a recorded edit.
    let other = crate::invoices::create_and_post_invoice(&pool, invoice("Acme", 10_000)).await.unwrap().id;
    let err = sqlx::query("UPDATE invoice_payments SET invoice_id = ?1 WHERE invoice_id = ?2").bind(other).bind(paid_off).execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("INV_PAYMENT_IMMUTABLE"), "{err}");
    books_balance(&pool).await;
}

#[tokio::test]
async fn a_deposited_payment_stays_with_the_edited_invoice() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let undeposited: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_ASSET' ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    let invoice = |cents: i64| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": "Acme", "issueDate": "2026-06-01", "dueDate": "2026-06-30",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
        .unwrap()
    };
    let original = crate::invoices::create_and_post_invoice(&pool, invoice(40_000)).await.unwrap().id;
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { payment_method_id: None, invoice_id: original, deposit_account_id: undeposited, payment_date: date(5), amount_cents: 10_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None }).await.unwrap();
    let payment_id: i64 = sqlx::query_scalar("SELECT id FROM invoice_payments WHERE invoice_id = ?1").bind(original).fetch_one(&pool).await.unwrap();
    let deposit: crate::banking::DepositInput = serde_json::from_value(serde_json::json!({ "bankAccountId": checking, "depositDate": "2026-06-07", "memo": null,
        "items": [{ "kind": "PAYMENT", "id": payment_id }], "otherLines": [] })).unwrap();
    crate::banking::post_bank_deposit(&pool, deposit).await.unwrap();
    let bank_before = balance(&pool, checking).await;

    let edited = replace_invoice(&pool, original, invoice(45_000)).await.unwrap();
    assert_eq!(invoice_row(&pool, edited).await, (45_000, 10_000, "PARTIAL".into(), false));
    assert_eq!(balance(&pool, checking).await, bank_before, "the deposit is untouched");
    assert_eq!(control_and_aging(&pool, false).await, (35_000, 35_000));
    books_balance(&pool).await;
}

#[tokio::test]
async fn editing_a_part_paid_bill_keeps_its_payment_and_vendor_credit() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let bill = |vendor: &str, cents: i64| -> BillInput {
        serde_json::from_value(serde_json::json!({ "vendorName": vendor, "billNumber": "B-7", "billDate": "2026-06-01", "dueDate": "2026-06-30", "memo": null,
            "totalCents": cents, "lines": [{ "accountId": rent, "amountCents": cents, "memo": "June" }] }))
        .unwrap()
    };
    let original = crate::expenses::post_vendor_bill(&pool, bill("Landlord", 200_000)).await.unwrap().id;
    crate::expenses::pay_vendor_bill(&pool, crate::expenses::BillPaymentInput { bill_id: original, payment_account_id: checking, payment_date: date(3), amount_cents: 50_000, reference: None }).await.unwrap();
    let credit: crate::credits::VendorCreditInput = serde_json::from_value(serde_json::json!({ "vendorName": "Landlord", "reference": "CR-1", "creditDate": "2026-06-04", "memo": null,
        "lines": [{ "accountId": rent, "amountCents": 10_000, "memo": null }] })).unwrap();
    let credit = crate::credits::create_vendor_credit(&pool, credit).await.unwrap().id;
    crate::credits::apply_vendor_credit(&pool, crate::credits::ApplyCreditInput { credit_id: credit, document_id: original, amount_cents: 10_000, applied_date: date(4) }).await.unwrap();
    assert_eq!(control_and_aging(&pool, true).await, (140_000, 140_000));

    let edited = replace_bill(&pool, original, bill("Landlord", 180_000)).await.unwrap();
    let row: (i64, i64, String) = sqlx::query_as("SELECT total_cents, amount_paid_cents, status FROM bills WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(row, (180_000, 60_000, "PARTIAL".into()));
    assert_eq!(control_and_aging(&pool, true).await, (120_000, 120_000), "A/P agrees with the aging");
    assert_eq!(balance(&pool, rent).await, 170_000, "the bill's expense, less the vendor credit");
    books_balance(&pool).await;

    let err = replace_bill(&pool, edited, bill("Landlord", 59_999)).await.unwrap_err();
    assert!(err.starts_with("EDIT_BELOW_PAID"), "{err}");
    let err = replace_bill(&pool, edited, bill("Other Vendor", 180_000)).await.unwrap_err();
    assert!(err.starts_with("EDIT_PAID_PARTY"), "{err}");
    assert_eq!(control_and_aging(&pool, true).await, (120_000, 120_000), "refused edits change nothing");
    books_balance(&pool).await;
}

#[tokio::test]
async fn a_bounced_payment_follows_the_edited_invoice() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let invoice = |cents: i64| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": "Acme", "issueDate": "2026-06-01", "dueDate": "2026-06-30",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
        .unwrap()
    };
    let original = crate::invoices::create_and_post_invoice(&pool, invoice(50_000)).await.unwrap().id;
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { payment_method_id: None, invoice_id: original, deposit_account_id: checking, payment_date: date(5), amount_cents: 20_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None }).await.unwrap();
    let payment: i64 = sqlx::query_scalar("SELECT id FROM invoice_payments WHERE invoice_id = ?1").bind(original).fetch_one(&pool).await.unwrap();
    let bounce = crate::bounced_checks::record(&pool, crate::bounced_checks::BounceInput { kind: "PAYMENT".into(), id: payment, bounce_date: date(10), bank_fee_cents: 0, bank_fee_account_id: None, customer_fee_cents: 0, customer_fee_account_id: None, memo: None }).await.unwrap();

    let edited = replace_invoice(&pool, original, invoice(60_000)).await.unwrap();
    assert_eq!(invoice_row(&pool, edited).await, (60_000, 0, "UNPAID".into(), false), "the bounce still reopens it");
    let on: i64 = sqlx::query_scalar("SELECT invoice_id FROM bounced_checks WHERE id = ?1").bind(bounce.id).fetch_one(&pool).await.unwrap();
    assert_eq!(on, edited);
    assert_eq!(control_and_aging(&pool, false).await, (60_000, 60_000));
    // Voiding the bounce counts the payment toward the edited invoice again.
    crate::bounced_checks::void(&pool, bounce.id).await.unwrap();
    assert_eq!(invoice_row(&pool, edited).await, (60_000, 20_000, "PARTIAL".into(), false));
    assert_eq!(control_and_aging(&pool, false).await, (40_000, 40_000));
    books_balance(&pool).await;
}

/// Pay Bills can use vendor credits and an early-payment discount (posted as a vendor credit)
/// automatically; editing the bill afterwards keeps them all applied.
#[tokio::test]
async fn a_bill_paid_with_auto_applied_credits_and_discount_keeps_them_when_edited() {
    use crate::sales_prefs::{pay_bills_plan, pay_bills_with_credits, BILL_AUTO_CREDITS, BILL_AUTO_DISCOUNTS, BILL_DISCOUNT_ACCOUNT};
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let terms: i64 = sqlx::query_scalar("INSERT INTO terms (name, net_days, discount_ppm, discount_days) VALUES ('2% 10 Net 30', 30, 20000, 10) RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    let bill = |cents: i64| -> BillInput {
        serde_json::from_value(serde_json::json!({ "vendorName": "Landlord", "billNumber": "B-9", "billDate": "2026-06-01", "dueDate": "2026-07-01", "memo": null,
            "totalCents": cents, "termsId": terms, "lines": [{ "accountId": rent, "amountCents": cents, "memo": "June" }] }))
        .unwrap()
    };
    let original = crate::expenses::post_vendor_bill(&pool, bill(50_000)).await.unwrap().id;
    let credit: crate::credits::VendorCreditInput = serde_json::from_value(serde_json::json!({ "vendorName": "Landlord", "reference": "CR-2", "creditDate": "2026-06-02", "memo": null,
        "lines": [{ "accountId": rent, "amountCents": 5_000, "memo": null }] })).unwrap();
    crate::credits::create_vendor_credit(&pool, credit).await.unwrap();
    crate::preferences::set(&pool, BILL_AUTO_CREDITS, "true").await.unwrap();
    crate::preferences::set(&pool, BILL_AUTO_DISCOUNTS, "true").await.unwrap();
    crate::preferences::set(&pool, BILL_DISCOUNT_ACCOUNT, &rent.to_string()).await.unwrap();

    let plan = pay_bills_plan(&pool, &[original], date(5)).await.unwrap();
    assert_eq!((plan.discounts.len(), plan.credits.len()), (1, 1), "{plan:?}");
    let credits = plan.credits.iter().map(|u| crate::credits::ApplyCreditInput { credit_id: u.credit_id, document_id: u.bill_id, amount_cents: u.amount_cents, applied_date: date(5) }).collect();
    let pay = crate::expenses::BillPaymentInput { bill_id: original, payment_account_id: checking, payment_date: date(5), amount_cents: 20_000, reference: None };
    pay_bills_with_credits(&pool, vec![pay], credits, plan.discounts, date(5)).await.unwrap();
    // 200.00 paid, 50.00 vendor credit and a 10.00 discount (2% of 500.00).
    assert_eq!(control_and_aging(&pool, true).await, (24_000, 24_000));

    let edited = replace_bill(&pool, original, bill(55_000)).await.unwrap();
    let row: (i64, i64, String) = sqlx::query_as("SELECT total_cents, amount_paid_cents, status FROM bills WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(row, (55_000, 26_000, "PARTIAL".into()));
    let on_new: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vendor_credit_applications WHERE bill_id = ?1 AND is_voided = 0").bind(edited).fetch_one(&pool).await.unwrap();
    assert_eq!(on_new, 2, "the vendor credit and the discount both follow the edit");
    let unused: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(remaining_cents), 0) FROM v_vendor_credit_balances").fetch_one(&pool).await.unwrap();
    assert_eq!(unused, 0, "no credit is freed up by the edit");
    assert_eq!(control_and_aging(&pool, true).await, (29_000, 29_000), "A/P agrees with the aging");
    let payload: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'EDIT' ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let kinds: Vec<&str> = payload["relinked"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["BILL_PAYMENT", "VENDOR_CREDIT", "VENDOR_CREDIT"]);

    let err = replace_bill(&pool, edited, bill(25_999)).await.unwrap_err();
    assert!(err.starts_with("EDIT_BELOW_PAID"), "{err}");
    books_balance(&pool).await;
}
