use super::*;
use crate::db::test_pool;
use crate::expenses::SplitLineInput;
use crate::invoices::InvoiceLineInput;
use chrono::NaiveDate;
use crate::money::Cents;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
}

async fn check(pool: &SqlitePool, payee: &str, number: Option<&str>, cents: i64, queued: bool) -> Result<ExpenseView, String> {
    let (checking, rent) = (account_id(pool, "1010").await, account_id(pool, "6300").await);
    post_expense(
        pool,
        ExpenseInput { form_lines: Vec::new(),
            payment_method_id: None,
            kind: "CHECK".into(),
            payee_name: Some(payee.into()),
            payment_account_id: checking,
            expense_date: d(5),
            check_number: number.map(Into::into),
            payment_method: None,
            memo: None,
            lines: vec![SplitLineInput { account_id: rent, amount_cents: cents, memo: None, class_id: None, customer_id: None, billable: None }],
            currency: None,
        },
        queued,
    )
    .await
}

fn invoice_input(pool_services: i64) -> InvoiceInput {
    InvoiceInput {
        invoice_number: None,
        customer_name: "Acme".into(),
        issue_date: d(1),
        due_date: d(31),
        lines: vec![InvoiceLineInput {
            description: "Work".into(),
            quantity_e4: 10_000,
            rate_cents: Cents(12_500),
            income_account_id: pool_services,
            product_service_id: None,
            class_id: None,
            is_taxable: false, service_date: None,
        }],
        currency: None,
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details: Default::default(),
    }
}

#[tokio::test]
async fn a_queued_check_posts_without_a_number_and_takes_one_when_it_prints() {
    let pool = test_pool().await;
    // A number typed on the form is dropped: the check gets its number when it prints.
    let first = check(&pool, "Landlord", Some("999"), 120_000, true).await.expect("queued check posts");
    let second = check(&pool, "Plumber", None, 30_000, true).await.expect("queued check posts");
    let printed_now = check(&pool, "Cleaner", Some("1001"), 5_000, false).await.expect("ordinary check posts");
    assert!(first.to_be_printed && first.check_number.is_none());
    assert!(!printed_now.to_be_printed);
    // It is a real, balanced posting from the start.
    assert!(first.journal_entry_id.is_some());

    let queue = queued_checks(&pool, None).await.unwrap();
    assert_eq!(queue.iter().map(|x| x.id).collect::<Vec<_>>(), vec![first.id, second.id], "oldest first");

    // 1001 is taken by the check written by hand.
    let clash = mark_checks_printed(&pool, vec![PrintedCheck { expense_id: first.id, check_number: "1001".into(), bill_check_id: None }]).await.unwrap_err();
    assert!(clash.starts_with("PRINT_NUMBER_USED"), "{clash}");
    let twice = mark_checks_printed(
        &pool,
        vec![PrintedCheck { expense_id: first.id, check_number: "1002".into(), bill_check_id: None }, PrintedCheck { expense_id: second.id, check_number: "1002".into(), bill_check_id: None }],
    )
    .await
    .unwrap_err();
    assert!(twice.starts_with("PRINT_DUPLICATE_NUMBER"), "{twice}");
    assert_eq!(queued_checks(&pool, None).await.unwrap().len(), 2, "a refused batch changes nothing");

    // Only the first printed cleanly; the second stays queued without a number.
    assert_eq!(mark_checks_printed(&pool, vec![PrintedCheck { expense_id: first.id, check_number: "1002".into(), bill_check_id: None }]).await.unwrap(), 1);
    let queue = queued_checks(&pool, None).await.unwrap();
    assert_eq!(queue.iter().map(|x| x.id).collect::<Vec<_>>(), vec![second.id]);
    let all = crate::expenses::list_expenses(&pool).await.unwrap();
    let first_now = all.iter().find(|x| x.id == first.id).unwrap();
    assert_eq!(first_now.check_number.as_deref(), Some("1002"));
    assert!(!first_now.to_be_printed);
    assert_eq!(crate::expenses::next_check_number(&pool, first.payment_account_id).await.unwrap(), "1003");

    // Once printed, its number is fixed like any posted check's.
    let renumber = sqlx::query("UPDATE expenses SET check_number = '2000' WHERE id = ?1").bind(first.id).execute(&pool).await.unwrap_err();
    assert!(renumber.to_string().contains("EXP_IMMUTABLE"), "{renumber}");
    // And a printed check can't be printed again through the queue.
    let again = mark_checks_printed(&pool, vec![PrintedCheck { expense_id: first.id, check_number: "1010".into(), bill_check_id: None }]).await.unwrap_err();
    assert!(again.starts_with("PRINT_NOT_QUEUED"), "{again}");
}

#[tokio::test]
async fn only_unnumbered_live_checks_join_the_queue() {
    let pool = test_pool().await;
    let numbered = check(&pool, "Landlord", Some("1001"), 10_000, false).await.unwrap();
    let refused = set_to_be_printed(&pool, Form::Check, numbered.id, true).await.unwrap_err();
    assert!(refused.contains("PRINT_HAS_NUMBER"), "{refused}");

    let (card, supplies) = (account_id(&pool, "2100").await, account_id(&pool, "6250").await);
    let expense = ExpenseInput { form_lines: Vec::new(),
        payment_method_id: None,
        kind: "EXPENSE".into(),
        payee_name: None,
        payment_account_id: card,
        expense_date: d(5),
        check_number: None,
        payment_method: None,
        memo: None,
        lines: vec![SplitLineInput { account_id: supplies, amount_cents: 1_000, memo: None, class_id: None, customer_id: None, billable: None }],
        currency: None,
    };
    let not_a_check = post_expense(&pool, expense, true).await.unwrap_err();
    assert!(not_a_check.starts_with("PRINT_NOT_A_CHECK"), "{not_a_check}");

    let unnumbered = check(&pool, "Plumber", None, 5_000, false).await.unwrap();
    set_to_be_printed(&pool, Form::Check, unnumbered.id, true).await.expect("an unnumbered check can be queued later");
    assert_eq!(queued_checks(&pool, None).await.unwrap().len(), 1);
    set_to_be_printed(&pool, Form::Check, unnumbered.id, false).await.unwrap();
    assert!(queued_checks(&pool, None).await.unwrap().is_empty());

    // A voided check leaves the queue and can't rejoin it.
    set_to_be_printed(&pool, Form::Check, unnumbered.id, true).await.unwrap();
    crate::ledger::void_transaction(&pool, unnumbered.journal_entry_id.unwrap()).await.unwrap();
    assert!(queued_checks(&pool, None).await.unwrap().is_empty());
    let voided = set_to_be_printed(&pool, Form::Check, unnumbered.id, true).await.unwrap_err();
    assert!(voided.starts_with("PRINT_VOIDED"), "{voided}");
}

#[tokio::test]
async fn an_edited_queued_check_stays_queued() {
    let pool = test_pool().await;
    let queued = check(&pool, "Landlord", None, 10_000, true).await.unwrap();
    let (checking, rent) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await);
    let edited = ExpenseInput { form_lines: Vec::new(),
        payment_method_id: None,
        kind: "CHECK".into(),
        payee_name: Some("Landlord".into()),
        payment_account_id: checking,
        expense_date: d(6),
        check_number: None,
        payment_method: None,
        memo: Some("October rent".into()),
        lines: vec![SplitLineInput { account_id: rent, amount_cents: 11_000, memo: None, class_id: None, customer_id: None, billable: None }],
        currency: None,
    };
    let new_id = crate::edits::replace_expense(&pool, queued.id, edited).await.unwrap();
    let queue = queued_checks(&pool, None).await.unwrap();
    assert_eq!(queue.iter().map(|x| (x.id, x.total_cents)).collect::<Vec<_>>(), vec![(new_id, 11_000)]);
}

#[tokio::test]
async fn invoices_queue_and_leave_the_queue_when_printed() {
    let pool = test_pool().await;
    let services = account_id(&pool, "4100").await;
    let queued = create_invoice(&pool, invoice_input(services), true, None).await.unwrap();
    let plain = create_invoice(&pool, invoice_input(services), false, None).await.unwrap();
    assert!(queued.to_be_printed && !plain.to_be_printed);
    assert_eq!(queued.status, "UNPAID", "it posts like any invoice");

    set_to_be_printed(&pool, Form::Invoice, plain.id, true).await.unwrap();
    let queue = queued_invoices(&pool).await.unwrap();
    assert_eq!(queue.iter().map(|i| i.id).collect::<Vec<_>>(), vec![queued.id, plain.id]);

    // Editing keeps it queued.
    let replaced = crate::edits::replace_invoice(&pool, plain.id, invoice_input(services)).await.unwrap();
    let queue = queued_invoices(&pool).await.unwrap();
    assert_eq!(queue.iter().map(|i| i.id).collect::<Vec<_>>(), vec![queued.id, replaced]);

    assert_eq!(mark_invoices_printed(&pool, vec![queued.id, 9_999]).await.unwrap(), 1, "only queued invoices count");
    assert_eq!(mark_invoices_printed(&pool, vec![queued.id]).await.unwrap(), 0, "already out of the queue");
    assert_eq!(queued_invoices(&pool).await.unwrap().iter().map(|i| i.id).collect::<Vec<_>>(), vec![replaced]);
}

#[tokio::test]
async fn the_print_later_default_is_remembered_per_form() {
    let pool = test_pool().await;
    remember_default(&pool, Form::Check, true).await.unwrap();
    remember_default(&pool, Form::Invoice, false).await.unwrap();
    assert_eq!(crate::preferences::get(&pool, PREF_CHECKS).await.unwrap(), "true");
    assert_eq!(crate::preferences::get(&pool, PREF_INVOICES).await.unwrap(), "false");
    assert!(Form::parse("BILL").is_err());
}

#[tokio::test]
async fn pay_bills_checks_wait_in_the_queue_one_per_vendor_and_take_their_number_when_printed() {
    use crate::expenses::{BillInput, BillLineInput, BillPaymentInput};
    let pool = test_pool().await;
    let (checking, rent, card) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await, account_id(&pool, "2100").await);
    let bill = |vendor: &str, number: &str, cents: i64| BillInput { form_lines: Vec::new(),
        terms_id: None,
        vendor_name: vendor.into(),
        bill_number: Some(number.into()),
        bill_date: d(1),
        due_date: d(31),
        memo: None,
        total_cents: cents,
        lines: vec![BillLineInput { account_id: rent, amount_cents: cents, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None, customer_id: None, billable: None }],
    };
    let mut ids = Vec::new();
    for (vendor, number, cents) in [("Landlord", "L-1", 100_000), ("Landlord", "L-2", 20_000), ("Plumber", "P-9", 7_500)] {
        ids.push(crate::expenses::post_vendor_bill(&pool, bill(vendor, number, cents)).await.unwrap().id);
    }
    let pay = |bill_id: i64, cents: i64| BillPaymentInput { bill_id, payment_account_id: checking, payment_date: d(10), amount_cents: cents, reference: Some("777".into()), to_be_printed: true };
    crate::expenses::pay_vendor_bills(&pool, vec![pay(ids[0], 100_000), pay(ids[1], 20_000), pay(ids[2], 7_500)]).await.unwrap();

    // Posted already, with no number yet; one check per vendor.
    let refs: Vec<Option<String>> = sqlx::query_scalar("SELECT reference FROM bill_payments ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(refs, vec![None, None, None]);
    let queue = queued_bill_checks(&pool, None).await.unwrap();
    assert_eq!(
        queue.iter().map(|c| (c.payee_name.as_str(), c.total_cents, c.memo.as_deref(), c.bill_count)).collect::<Vec<_>>(),
        [("Landlord", 120_000, Some("Bills L-1, L-2"), 2), ("Plumber", 7_500, Some("Bill P-9"), 1)]
    );

    let landlord = PrintedCheck { expense_id: 0, check_number: "1001".into(), bill_check_id: Some(queue[0].id) };
    assert_eq!(mark_checks_printed(&pool, vec![landlord.clone()]).await.unwrap(), 1);
    let refs: Vec<Option<String>> = sqlx::query_scalar("SELECT reference FROM bill_payments ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(refs, vec![Some("1001".into()), Some("1001".into()), None]);
    assert!(mark_checks_printed(&pool, vec![landlord]).await.unwrap_err().starts_with("PRINT_NOT_QUEUED"));
    let reused = PrintedCheck { expense_id: 0, check_number: "1001".into(), bill_check_id: Some(queue[1].id) };
    assert!(mark_checks_printed(&pool, vec![reused]).await.unwrap_err().starts_with("PRINT_NUMBER_USED"));
    assert_eq!(crate::expenses::next_check_number(&pool, checking).await.unwrap(), "1002");
    assert_eq!(queued_bill_checks(&pool, None).await.unwrap().len(), 1);

    // A printed number is fixed; a card can't print a check.
    assert!(sqlx::query("UPDATE bill_payments SET reference = '2000' WHERE reference = '1001'").execute(&pool).await.is_err());
    let other = crate::expenses::post_vendor_bill(&pool, bill("Plumber", "P-10", 1_000)).await.unwrap().id;
    let err = crate::expenses::pay_vendor_bills(&pool, vec![BillPaymentInput { payment_account_id: card, ..pay(other, 1_000) }]).await.unwrap_err();
    assert!(err.starts_with("PRINT_NOT_A_CHECK"), "{err}");
}

#[tokio::test]
async fn a_payment_taken_on_the_invoice_is_saved_with_it() {
    let pool = test_pool().await;
    let (services, ar, checking) = (account_id(&pool, "4100").await, account_id(&pool, "1200").await, account_id(&pool, "1010").await);
    let balance = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id WHERE l.account_id = ?1 AND e.is_voided = 0")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let paid = |cents: i64| crate::invoices::PaidNow { deposit_account_id: checking, amount_cents: Cents(cents), payment_method: "CHECK".into(), payment_method_id: None, reference: Some("1042".into()) };

    // Part paid at the counter: the invoice is PARTIAL and the money is in the bank, dated the invoice's date.
    let part = create_invoice(&pool, invoice_input(services), false, Some(&paid(5_000))).await.unwrap();
    assert_eq!((part.status.as_str(), part.balance_cents), ("PARTIAL", 7_500));
    let payments = crate::invoices::list_payments(&pool, part.id).await.unwrap();
    assert_eq!(payments.iter().map(|p| (p.amount_cents, p.payment_date.clone(), p.reference.clone())).collect::<Vec<_>>(), [(5_000, d(1).to_string(), Some("1042".into()))]);
    assert_eq!((balance(ar).await, balance(checking).await), (7_500, 5_000));

    // Paid in full and queued to print.
    let full = create_invoice(&pool, invoice_input(services), true, Some(&paid(12_500))).await.unwrap();
    assert_eq!((full.status.as_str(), full.balance_cents, full.to_be_printed), ("PAID", 0, true));

    // More than the invoice, or into an account money can't be deposited to: nothing is saved.
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert!(create_invoice(&pool, invoice_input(services), false, Some(&paid(12_501))).await.unwrap_err().starts_with("INV_OVERPAYMENT"));
    let wrong = crate::invoices::PaidNow { deposit_account_id: services, ..paid(100) };
    assert!(create_invoice(&pool, invoice_input(services), false, Some(&wrong)).await.unwrap_err().starts_with("INV_BAD_DEPOSIT_ACCOUNT"));
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after, "a refused payment leaves no invoice behind");
}
