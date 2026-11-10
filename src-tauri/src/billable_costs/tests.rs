use super::*;
use crate::db::test_pool;
use crate::expenses::ExpenseInput;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

/// Debit-positive balance.
async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, day).unwrap()
}

/// A check for $200 of supplies (billable or not as given, or by default) and $50 of unrelated rent.
fn check(checking: i64, supplies: i64, rent: i64, job: i64, billable: Option<bool>) -> ExpenseInput {
    serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Harbor Hardware", "paymentAccountId": checking, "expenseDate": "2026-05-02",
        "checkNumber": null, "paymentMethod": null, "memo": null,
        "lines": [{ "accountId": supplies, "amountCents": 20_000, "memo": "Lumber", "customerId": job, "billable": billable },
                  { "accountId": rent, "amountCents": 5_000, "memo": null }] }))
    .unwrap()
}

#[tokio::test]
async fn billable_expenses_are_invoiced_with_markup_and_reduce_the_expense() {
    let pool = test_pool().await;
    let (checking, supplies, rent, services) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await, account(&pool, "4100").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;

    // Not marked: not billable (Preferences › "Mark all expenses as billable" is off by default).
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, None)).await.unwrap();
    assert!(list_unbilled(&pool, None).await.unwrap().is_empty());
    // With the preference on, a line for a customer is billable unless the form says otherwise.
    crate::preferences::set(&pool, "expenses_billable_default", "true").await.unwrap();
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, None)).await.unwrap();
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, Some(false))).await.unwrap();
    let unbilled = list_unbilled(&pool, Some(acme)).await.unwrap();
    assert_eq!(unbilled.iter().map(|c| (c.source.as_str(), c.amount_cents, c.memo.as_deref())).collect::<Vec<_>>(), [("EXPENSE", 20_000, Some("Lumber"))]);

    // A markup needs its account.
    let bill = |markup: Option<&str>| BillCostsInput {
        customer_id: acme,
        costs: vec![CostRef { source: "EXPENSE".into(), line_id: unbilled[0].line_id }],
        time: Vec::new(),
        issue_date: d(10),
        due_date: d(31),
        markup_percent: markup.map(str::to_string),
        tax_rate_id: None,
    };
    crate::preferences::set(&pool, "billable_markup_percent", "15").await.unwrap();
    assert!(bill_costs(&pool, bill(None)).await.unwrap_err().starts_with("COST_NO_MARKUP_ACCOUNT"));
    crate::preferences::set(&pool, "billable_markup_account_id", &services.to_string()).await.unwrap();
    let expense_before = balance(&pool, supplies).await;
    let invoice = bill_costs(&pool, bill(None)).await.unwrap();
    // $200 back, plus 15% markup ($30).
    assert_eq!(invoice.total_cents, 23_000);
    assert_eq!(balance(&pool, supplies).await, expense_before - 20_000, "the reimbursement reduces the expense");
    assert_eq!(balance(&pool, services).await, -3_000, "the markup is income");
    assert!(list_unbilled(&pool, Some(acme)).await.unwrap().is_empty(), "billed costs leave the list");
    assert!(bill_costs(&pool, bill(Some("0"))).await.unwrap_err().starts_with("COST_NOT_BILLABLE"), "never billed twice");

    // An edit of the invoice keeps the cost billed; voiding it frees the cost again.
    let lines = crate::invoices::invoice_detail(&pool, invoice.id).await.unwrap().items;
    let edited = crate::invoices::InvoiceInput {
        invoice_number: None,
        customer_name: "Acme".into(),
        issue_date: d(11),
        due_date: d(31),
        lines: lines
            .iter()
            .map(|l| crate::invoices::InvoiceLineInput { description: l.description.clone(), quantity_e4: l.quantity_e4, rate_cents: l.rate_cents, income_account_id: l.income_account_id, product_service_id: None, class_id: None, is_taxable: false, service_date: None })
            .collect(),
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    let new_invoice = crate::edits::replace_invoice(&pool, invoice.id, edited).await.unwrap();
    assert!(list_unbilled(&pool, Some(acme)).await.unwrap().is_empty(), "the edited invoice still bills it");
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(new_invoice).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert_eq!(list_unbilled(&pool, Some(acme)).await.unwrap().len(), 1, "a voided invoice frees the cost");

    // Counted as income instead, at a 10% markup typed on the screen.
    crate::preferences::set(&pool, "reimbursed_as_income", "true").await.unwrap();
    assert!(bill_costs(&pool, bill(Some("10"))).await.unwrap_err().starts_with("COST_NO_INCOME_ACCOUNT"));
    let reimbursed = account(&pool, "4000").await;
    crate::preferences::set(&pool, "reimbursed_income_account_id", &reimbursed.to_string()).await.unwrap();
    assert!(crate::preferences::set(&pool, "reimbursed_income_account_id", &supplies.to_string()).await.unwrap_err().starts_with("PREF_BAD_INCOME_ACCOUNT"));
    let (supplies_before, income_before) = (balance(&pool, supplies).await, balance(&pool, reimbursed).await);
    let as_income = bill_costs(&pool, bill(Some("10"))).await.unwrap();
    assert_eq!(as_income.total_cents, 22_000);
    assert_eq!(balance(&pool, supplies).await, supplies_before, "the expense stays");
    assert_eq!(balance(&pool, reimbursed).await, income_before - 20_000);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

#[test]
fn markup_rounds_half_away_from_zero() {
    assert_eq!(markup_cents(20_000, 15_000), Some(3_000));
    assert_eq!(markup_cents(333, 12_500), Some(42), "41.625 → 42");
    assert_eq!(markup_cents(1_000, 0), Some(0));
    assert_eq!(tax_time::parse_markup("12.5%").unwrap(), 12_500);
    assert!(tax_time::parse_markup("1001").is_err());
}

/// Preferences › Checking › the vendor's account number in the check memo; a typed memo wins.
#[tokio::test]
async fn checks_carry_the_vendors_account_number_in_the_memo() {
    let pool = test_pool().await;
    let (checking, supplies, rent) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    let vendor = crate::expenses::create_vendor(&pool, "Harbor Hardware", None).await.unwrap();
    assert_eq!(crate::expenses::set_vendor_account_number(&pool, vendor.id, Some(" 55-1234 ")).await.unwrap().as_deref(), Some("55-1234"));
    let memo_of = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, Option<String>>("SELECT memo FROM expenses WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap() }
    };
    let first = crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, None)).await.unwrap();
    assert_eq!(memo_of(first.id).await, None, "off by default");
    crate::preferences::set(&pool, "check_memo_account_number", "true").await.unwrap();
    let second = crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, None)).await.unwrap();
    assert_eq!(memo_of(second.id).await.as_deref(), Some("Acct #55-1234"));
    let mut typed = check(checking, supplies, rent, acme, None);
    typed.memo = Some("Deck job".into());
    let third = crate::expenses::post_expense(&pool, typed).await.unwrap();
    assert_eq!(memo_of(third.id).await.as_deref(), Some("Deck job"));
    assert!(crate::expenses::set_vendor_account_number(&pool, vendor.id, Some(&"9".repeat(41))).await.is_err());
}

/// Preferences › Checking › "Change check date when a check is printed".
#[tokio::test]
async fn printed_checks_take_the_print_date() {
    let pool = test_pool().await;
    let (checking, supplies, rent) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    crate::preferences::set(&pool, "expenses_billable_default", "true").await.unwrap();
    let queued = crate::print_queue::post_expense(&pool, check(checking, supplies, rent, acme, None), true).await.unwrap();
    let printed = |id: i64, number: &str| vec![crate::print_queue::PrintedCheck { expense_id: id, check_number: number.into(), bill_check_id: None }];

    // Off: the check keeps its date.
    crate::print_queue::mark_checks_printed_on(&pool, printed(queued.id, "501"), d(9)).await.unwrap();
    let kept: (String, Option<String>) = sqlx::query_as("SELECT expense_date, check_number FROM expenses WHERE id = ?1").bind(queued.id).fetch_one(&pool).await.unwrap();
    assert_eq!(kept, ("2026-05-02".into(), Some("501".into())));

    // On: re-dated by an edit, with the number on the replacement, which stays billable.
    crate::preferences::set(&pool, "date_checks_when_printed", "true").await.unwrap();
    let second = crate::print_queue::post_expense(&pool, check(checking, supplies, rent, acme, None), true).await.unwrap();
    crate::print_queue::mark_checks_printed_on(&pool, printed(second.id, "502"), d(9)).await.unwrap();
    let old: bool = sqlx::query_scalar("SELECT is_voided FROM expenses WHERE id = ?1").bind(second.id).fetch_one(&pool).await.unwrap();
    assert!(old, "the queued check is replaced");
    let new: (i64, String, Option<String>, bool) = sqlx::query_as("SELECT id, expense_date, check_number, to_be_printed FROM expenses WHERE check_number = '502'").fetch_one(&pool).await.unwrap();
    assert_eq!((new.1.as_str(), new.2.as_deref(), new.3), ("2026-05-09", Some("502"), false));
    let billable: Vec<i64> = list_unbilled(&pool, Some(acme)).await.unwrap().iter().map(|c| c.line_id).collect();
    assert_eq!(billable.len(), 2, "the first check's cost and the replacement's");
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

/// My Preferences › General › recall a vendor's last transaction.
#[tokio::test]
async fn a_vendors_last_transaction_is_recalled() {
    let pool = test_pool().await;
    let (checking, supplies, rent) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    assert!(crate::edits::last_for_vendor(&pool, "Harbor Hardware", None).await.unwrap().is_none());
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, Some(true))).await.unwrap();
    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({ "vendorName": "Harbor Hardware", "billNumber": "H-9", "billDate": "2026-05-01", "dueDate": "2026-05-31", "memo": "Monthly", "totalCents": 7_500,
        "lines": [{ "accountId": rent, "amountCents": 7_500, "memo": "Shed" }] })).unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();

    let latest = crate::edits::last_for_vendor(&pool, "harbor hardware", None).await.unwrap().unwrap();
    assert_eq!((latest.kind.as_str(), latest.total_cents), ("expense", 25_000), "the check (May 2) is newer than the bill (May 1)");
    assert_eq!(latest.lines.iter().map(|l| (l.account_id, l.amount_cents, l.billable)).collect::<Vec<_>>(), [(supplies, 20_000, true), (rent, 5_000, false)]);
    let bill = crate::edits::last_for_vendor(&pool, "Harbor Hardware", Some("bill")).await.unwrap().unwrap();
    assert_eq!((bill.memo.as_deref(), bill.lines.len(), bill.lines[0].memo.as_deref()), (Some("Monthly"), 1, Some("Shed")));
}

#[tokio::test]
async fn reimbursements_go_to_the_expense_accounts_own_income_account() {
    let pool = test_pool().await;
    let (checking, supplies, rent, services, sales) =
        (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await, account(&pool, "4100").await, account(&pool, "4000").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, Some(true))).await.unwrap();
    crate::preferences::set(&pool, "reimbursed_as_income", "true").await.unwrap();
    crate::preferences::set(&pool, "reimbursed_income_account_id", &sales.to_string()).await.unwrap();

    // Only an expense account maps, and only to an income account.
    assert!(set_reimbursement_account(&pool, sales, Some(services)).await.unwrap_err().starts_with("COST_REIMBURSE_NOT_EXPENSE"));
    assert!(set_reimbursement_account(&pool, supplies, Some(rent)).await.unwrap_err().starts_with("COST_REIMBURSE_NOT_INCOME"));
    set_reimbursement_account(&pool, supplies, Some(services)).await.unwrap();
    let listed = crate::reports::list_accounts(&pool).await.unwrap().into_iter().find(|a| a.id == supplies).unwrap();
    assert_eq!(listed.reimbursement_income_account_id, Some(services));

    let cost = list_unbilled(&pool, Some(acme)).await.unwrap().remove(0);
    let input = BillCostsInput { customer_id: acme, costs: vec![CostRef { source: "EXPENSE".into(), line_id: cost.line_id }], time: Vec::new(), issue_date: d(10), due_date: d(31), markup_percent: Some("0".into()), tax_rate_id: None };
    bill_costs(&pool, input).await.unwrap();
    assert_eq!(balance(&pool, services).await, -20_000, "the supplies account's own income account");
    assert_eq!(balance(&pool, sales).await, 0, "not the one in Preferences");
}

#[tokio::test]
async fn billable_time_is_invoiced_once_at_its_hours_and_rate() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    let design = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Design".into(), description: None, rate_cents: 9_500, income_account_id: services, is_taxable: false })
        .await
        .unwrap()
        .id;
    let dana = crate::timesheet::create_employee(&pool, "Dana", None).await.unwrap().id;
    let entry = |minutes: i64, item: Option<i64>, billable: bool| crate::timesheet::TimeActivityInput {
        employee_id: dana,
        activity_date: d(4),
        customer_id: Some(acme),
        product_service_id: item,
        minutes,
        billable,
        hourly_rate_cents: billable.then_some(9_500),
        description: Some("Logo concepts".into()),
        class_id: None,
    };
    crate::timesheet::create_time_activities(&pool, vec![entry(90, Some(design), true), entry(30, Some(design), false), entry(60, None, true)]).await.unwrap();
    let time = list_unbilled_time(&pool, Some(acme)).await.unwrap();
    assert_eq!(time.iter().map(|t| (t.minutes, t.amount_cents)).collect::<Vec<_>>(), [(90, 14_250), (60, 9_500)], "billable only; 1.5 h × $95");
    assert_eq!(time[0].hours_e4, 15_000, "hours as the invoice line's quantity");

    let bill = |ids: Vec<i64>| BillCostsInput { customer_id: acme, costs: Vec::new(), time: ids, issue_date: d(10), due_date: d(31), markup_percent: Some("0".into()), tax_rate_id: None };
    // Time with no service item can't be billed: the item gives the invoice line its income account.
    assert!(bill_costs(&pool, bill(vec![time[1].id])).await.unwrap_err().starts_with("TIME_NO_ITEM"));
    let invoice = bill_costs(&pool, bill(vec![time[0].id])).await.unwrap();
    assert_eq!(invoice.total_cents, 14_250);
    assert_eq!(balance(&pool, services).await, -14_250, "the service item's income account");
    let lines = crate::invoices::invoice_detail(&pool, invoice.id).await.unwrap().items;
    assert_eq!((lines[0].quantity_e4, lines[0].rate_cents, lines[0].product_service_id), (15_000, 9_500, Some(design)));
    assert_eq!(list_unbilled_time(&pool, Some(acme)).await.unwrap().len(), 1, "billed time leaves the list");
    assert!(bill_costs(&pool, bill(vec![time[0].id])).await.unwrap_err().starts_with("TIME_NOT_BILLABLE"), "never billed twice");
    let removed = sqlx::query("DELETE FROM billed_time").execute(&pool).await.unwrap_err().to_string();
    assert!(removed.contains("TIME_BILLED_IMMUTABLE"), "a billing record can't be removed: {removed}");

    // Voiding the invoice frees the time again.
    let entry_id: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(invoice.id).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry_id).await.unwrap();
    assert_eq!(list_unbilled_time(&pool, Some(acme)).await.unwrap().len(), 2);
}

/// A cost on an expense in another currency is billed at its home amount, converted at the
/// expense's rate, and the reimbursement takes off exactly what the expense put on.
#[tokio::test]
async fn a_foreign_currency_cost_is_billed_at_its_home_amount() {
    let pool = test_pool().await;
    let (checking, supplies, rent) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    let mut euro = check(checking, supplies, rent, acme, Some(true));
    euro.currency = Some(crate::ledger::ForeignCurrency { code: "EUR".into(), exchange_rate_e6: 1_105_000 });
    crate::expenses::post_expense(&pool, euro).await.unwrap();
    let unbilled = list_unbilled(&pool, Some(acme)).await.unwrap();
    assert_eq!(unbilled.len(), 1);
    let cost = &unbilled[0];
    // €200.00 at 1.105 is $221.00.
    assert_eq!((cost.amount_cents, cost.foreign_amount_cents, cost.currency_code.as_deref()), (22_100, Some(20_000), Some("EUR")));
    let expense_before = balance(&pool, supplies).await;
    let invoice = bill_costs(
        &pool,
        BillCostsInput {
            customer_id: acme,
            costs: vec![CostRef { source: "EXPENSE".into(), line_id: cost.line_id }],
            time: Vec::new(),
            issue_date: d(10),
            due_date: d(31),
            markup_percent: Some("0".into()),
            tax_rate_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(invoice.total_cents, 22_100);
    assert_eq!(balance(&pool, supplies).await, expense_before - 22_100, "the expense is back to where it was");
    let line = &crate::invoices::invoice_detail(&pool, invoice.id).await.unwrap().items[0];
    assert!(line.description.ends_with(", EUR 200.00)"), "{}", line.description);
}

/// Editing an invoice that billed costs can move a reimbursed line to another expense account,
/// but not to an asset or liability.
#[tokio::test]
async fn an_edited_reimbursement_line_can_move_to_another_expense_account() {
    let pool = test_pool().await;
    let (checking, supplies, rent) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "6300").await);
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    crate::expenses::post_expense(&pool, check(checking, supplies, rent, acme, Some(true))).await.unwrap();
    let cost = list_unbilled(&pool, Some(acme)).await.unwrap()[0].line_id;
    let invoice = bill_costs(
        &pool,
        BillCostsInput { customer_id: acme, costs: vec![CostRef { source: "EXPENSE".into(), line_id: cost }], time: Vec::new(), issue_date: d(10), due_date: d(31), markup_percent: Some("0".into()), tax_rate_id: None },
    )
    .await
    .unwrap();
    let edited = |account_id: i64| crate::invoices::InvoiceInput {
        invoice_number: None,
        customer_name: "Acme".into(),
        issue_date: d(11),
        due_date: d(31),
        lines: vec![crate::invoices::InvoiceLineInput { description: "Lumber".into(), quantity_e4: 10_000, rate_cents: 20_000, income_account_id: account_id, product_service_id: None, class_id: None, is_taxable: false, service_date: None }],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    let err = crate::edits::replace_invoice(&pool, invoice.id, edited(checking)).await.unwrap_err();
    assert!(err.starts_with("INV_NOT_INCOME_ACCOUNT"), "{err}");
    let (supplies_before, rent_before) = (balance(&pool, supplies).await, balance(&pool, rent).await);
    crate::edits::replace_invoice(&pool, invoice.id, edited(rent)).await.unwrap();
    assert_eq!((balance(&pool, supplies).await - supplies_before, balance(&pool, rent).await - rent_before), (20_000, -20_000), "the reimbursement moved to Rent");
    assert!(list_unbilled(&pool, Some(acme)).await.unwrap().is_empty(), "the cost stays billed");
}
