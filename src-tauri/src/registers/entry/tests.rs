use super::*;
use crate::db::test_pool;
use crate::money::Cents;
use crate::registers::account_register;
use crate::registers::history::transaction_history;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn d(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn split(account_id: i64, amount_cents: i64) -> RegisterSplit {
    RegisterSplit { account_id, amount_cents, memo: None, class_id: None, customer_id: None }
}

fn row(account_id: i64, side: RegisterSide, payee: Option<&str>, number: Option<&str>, lines: Vec<RegisterSplit>) -> RegisterEntryInput {
    RegisterEntryInput {
        account_id,
        entry_date: d("2026-03-04"),
        number: number.map(str::to_string),
        payee_name: payee.map(str::to_string),
        memo: Some("typed in the register".into()),
        side,
        lines,
    }
}

#[tokio::test]
async fn a_bank_payment_is_recorded_as_a_check_and_shows_in_the_register() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let posted = post_register_entry(&pool, row(checking, RegisterSide::Payment, Some("Landlord LLC"), Some("1042"), vec![split(rent, 120_000)])).await.unwrap();
    assert_eq!(posted.source_type, "CHECK");

    // It is an ordinary check: the Write Checks list has it, with its number and a new vendor.
    let checks = crate::expenses::list_expenses(&pool).await.unwrap();
    let check = checks.iter().find(|x| x.id == posted.source_id).unwrap();
    assert_eq!((check.kind.as_str(), check.check_number.as_deref(), check.payee_name.as_deref(), check.total_cents), ("CHECK", Some("1042"), Some("Landlord LLC"), 120_000));
    assert_eq!(check.journal_entry_id, Some(posted.journal_entry_id));

    let reg = account_register(&pool, checking, None, None, false).await.unwrap();
    let r = reg.rows.iter().find(|r| r.journal_entry_id == posted.journal_entry_id).unwrap();
    assert_eq!((r.source_type.as_str(), r.party_name.as_deref(), r.split.as_deref(), r.credit_cents), ("CHECK", Some("Landlord LLC"), Some("Rent & Lease"), 120_000));
    assert_eq!(r.num.as_deref(), Some("1042"), "the register shows the check number");
    assert_eq!(reg.ending_balance_cents, -120_000);
}

#[tokio::test]
async fn a_bank_deposit_is_recorded_as_a_deposit_and_keeps_the_payee_in_its_memo() {
    let pool = test_pool().await;
    let (checking, sales) = (account(&pool, "1010").await, account(&pool, "4000").await);
    let posted = post_register_entry(&pool, row(checking, RegisterSide::Deposit, Some("Walk-in"), Some("99"), vec![split(sales, 45_000)])).await.unwrap();
    assert_eq!(posted.source_type, "DEPOSIT");
    let (memo, total, entry): (Option<String>, i64, Option<i64>) =
        sqlx::query_as("SELECT memo, total_cents, journal_entry_id FROM deposits WHERE id = ?1").bind(posted.source_id).fetch_one(&pool).await.unwrap();
    assert_eq!((memo.as_deref(), total, entry), (Some("Walk-in · typed in the register"), 45_000, Some(posted.journal_entry_id)));
    let reg = account_register(&pool, checking, None, None, false).await.unwrap();
    assert_eq!(reg.ending_balance_cents, 45_000);
    assert_eq!(reg.rows[0].split.as_deref(), Some("Sales"));
}

#[tokio::test]
async fn a_deposit_from_a_customer_or_vendor_shows_them_as_payee_and_in_their_transactions() {
    use crate::custom_reports::find::{find, FindQuery};
    use crate::custom_reports::{Filters, NameKind, NameRef};
    let pool = test_pool().await;
    let (checking, sales, supplies) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "6100").await);
    let customer = crate::sales::create_customer(&pool, "Harbor Cafe", None).await.unwrap().id;
    let vendor = crate::expenses::create_vendor(&pool, "Paper Supply Co", None).await.unwrap().id;

    // Typed in the register: the payee becomes the deposit's Received From, not part of its memo.
    let posted = post_register_entry(&pool, row(checking, RegisterSide::Deposit, Some("harbor cafe"), None, vec![split(sales, 30_000)])).await.unwrap();
    let (memo, named): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT d.memo, l.customer_id FROM deposits d JOIN deposit_lines l ON l.deposit_id = d.id WHERE d.id = ?1").bind(posted.source_id).fetch_one(&pool).await.unwrap();
    assert_eq!((memo.as_deref(), named), (Some("typed in the register"), Some(customer)));

    // On the Make Deposits form: a vendor's refund.
    let refund = crate::banking::post_bank_deposit(
        &pool,
        DepositInput {
            bank_account_id: checking,
            deposit_date: d("2026-03-05"),
            memo: None,
            items: vec![],
            other_lines: vec![DepositOtherLine { account_id: supplies, amount_cents: 2_500, memo: Some("Returned toner".into()), class_id: None, customer_id: None, vendor_id: Some(vendor) }],
            cash_back: None,
            deductions: Vec::new(),
        },
    )
    .await
    .unwrap();
    let refund_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1").bind(refund).fetch_one(&pool).await.unwrap();

    let reg = account_register(&pool, checking, None, None, false).await.unwrap();
    let payee = |entry: i64| reg.rows.iter().find(|r| r.journal_entry_id == entry).unwrap().party_name.clone();
    assert_eq!(payee(posted.journal_entry_id).as_deref(), Some("Harbor Cafe"));
    assert_eq!(payee(refund_entry).as_deref(), Some("Paper Supply Co"));

    let listed = |kind: NameKind, id: i64| {
        let pool = pool.clone();
        async move {
            let query = FindQuery { filters: Filters { names: vec![NameRef { kind, id }], ..Default::default() }, end_date: Some(d("2026-12-31")), ..Default::default() };
            find(&pool, &query, d("2026-12-31")).await.unwrap().rows.iter().map(|r| r.journal_entry_id).collect::<Vec<_>>()
        }
    };
    assert_eq!(listed(NameKind::Customer, customer).await, vec![posted.journal_entry_id]);
    assert_eq!(listed(NameKind::Vendor, vendor).await, vec![refund_entry]);

    // A line names a customer or a vendor, not both.
    let both = DepositOtherLine { account_id: sales, amount_cents: 100, memo: None, class_id: None, customer_id: Some(customer), vendor_id: Some(vendor) };
    let err = crate::banking::post_bank_deposit(&pool, DepositInput { bank_account_id: checking, deposit_date: d("2026-03-06"), memo: None, items: vec![], other_lines: vec![both], cash_back: None, deductions: Vec::new() }).await.unwrap_err();
    assert!(err.starts_with("DEP_TWO_NAMES"), "{err}");
}

#[tokio::test]
async fn a_deposit_whose_lines_name_different_people_has_no_payee_and_unknown_names_stay_in_the_memo() {
    let pool = test_pool().await;
    let (checking, sales) = (account(&pool, "1010").await, account(&pool, "4000").await);
    let a = crate::sales::create_customer(&pool, "Ann", None).await.unwrap().id;
    let b = crate::sales::create_customer(&pool, "Bo", None).await.unwrap().id;
    let line = |customer_id| DepositOtherLine { account_id: sales, amount_cents: 1_000, memo: None, class_id: None, customer_id: Some(customer_id), vendor_id: None };
    let id = crate::banking::post_bank_deposit(&pool, DepositInput { bank_account_id: checking, deposit_date: d("2026-03-05"), memo: None, items: vec![], other_lines: vec![line(a), line(b)], cash_back: None, deductions: Vec::new() }).await.unwrap();
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap();
    let party: Option<String> = sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(entry).fetch_one(&pool).await.unwrap();
    assert_eq!(party, None);

    let posted = post_register_entry(&pool, row(checking, RegisterSide::Deposit, Some("Walk-in"), None, vec![split(sales, 500)])).await.unwrap();
    let named: Option<i64> = sqlx::query_scalar("SELECT customer_id FROM deposit_lines WHERE deposit_id = ?1").bind(posted.source_id).fetch_one(&pool).await.unwrap();
    assert_eq!(named, None);
}

#[tokio::test]
async fn card_charges_and_credits_post_as_card_expenses_and_card_credits() {
    let pool = test_pool().await;
    let (card, rent) = (account(&pool, "2100").await, account(&pool, "6300").await);
    let charge = post_register_entry(&pool, row(card, RegisterSide::Charge, Some("Office Depot"), Some("ignored"), vec![split(rent, 8_000)])).await.unwrap();
    let credit = post_register_entry(&pool, row(card, RegisterSide::Credit, Some("Office Depot"), None, vec![split(rent, 3_000)])).await.unwrap();
    assert_eq!((charge.source_type.as_str(), credit.source_type.as_str()), ("EXPENSE", "CARD_CREDIT"));
    let reg = account_register(&pool, card, None, None, false).await.unwrap();
    let types: Vec<&str> = reg.rows.iter().map(|r| r.source_type.as_str()).collect();
    assert_eq!(types, ["EXPENSE", "CARD_CREDIT"]);
    // A liability: the charge raises what is owed and the credit lowers it.
    assert_eq!(reg.ending_balance_cents, 5_000);
}

#[tokio::test]
async fn a_split_row_posts_every_account() {
    let pool = test_pool().await;
    let (checking, rent, card) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "2100").await);
    let posted = post_register_entry(&pool, row(checking, RegisterSide::Payment, Some("Landlord LLC"), None, vec![split(rent, 1_000), split(card, 2_500)])).await.unwrap();
    let reg = account_register(&pool, checking, None, None, false).await.unwrap();
    let r = reg.rows.iter().find(|r| r.journal_entry_id == posted.journal_entry_id).unwrap();
    assert_eq!((r.split.as_deref(), r.credit_cents), (Some("-Split-"), 3_500));
}

#[tokio::test]
async fn rows_that_cannot_be_recorded_are_refused_with_a_reason() {
    let pool = test_pool().await;
    let (checking, rent, card, sales) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "2100").await, account(&pool, "4000").await);
    let err = |r: Result<RegisterEntryPosted, String>| r.unwrap_err().split(':').next().unwrap().to_string();
    assert_eq!(err(post_register_entry(&pool, row(checking, RegisterSide::Charge, None, None, vec![split(rent, 100)])).await), "REG_ENTRY_BAD_SIDE");
    assert_eq!(err(post_register_entry(&pool, row(card, RegisterSide::Deposit, None, None, vec![split(rent, 100)])).await), "REG_ENTRY_BAD_SIDE");
    assert_eq!(err(post_register_entry(&pool, row(sales, RegisterSide::Payment, None, None, vec![split(rent, 100)])).await), "REG_ENTRY_UNSUPPORTED");
    assert_eq!(err(post_register_entry(&pool, row(checking, RegisterSide::Payment, None, None, vec![])).await), "REG_ENTRY_NO_ACCOUNT");
    assert_eq!(err(post_register_entry(&pool, row(checking, RegisterSide::Payment, None, None, vec![split(checking, 100)])).await), "REG_ENTRY_SAME_ACCOUNT");
    // The form's own checks still apply: a zero amount is refused by the expense code.
    assert!(err(post_register_entry(&pool, row(checking, RegisterSide::Payment, None, None, vec![split(rent, 0)])).await).starts_with("EXP"));
    let posted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(posted, 0, "nothing was posted");
}

#[tokio::test]
async fn transaction_history_links_an_invoice_its_payment_and_the_deposit() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let undeposited: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Undeposited Funds'").fetch_one(&pool).await.unwrap();
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-03-01", "dueDate": "2026-03-31",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 50_000, "incomeAccountId": services }]
    }))
    .unwrap();
    let invoice = crate::invoices::create_and_post_invoice(&pool, input).await.unwrap();
    let pay = |cents| crate::invoices::PaymentInput {
        discount_cents: Cents::ZERO,
        payment_method_id: None,
        invoice_id: invoice.id,
        deposit_account_id: undeposited,
        payment_date: d("2026-03-10"),
        amount_cents: cents,
        payment_method: "CHECK".into(),
        reference: None,
        exchange_rate_e6: None,
    };
    crate::invoices::record_payment(&pool, pay(Cents(20_000))).await.unwrap();
    let waiting = crate::banking::list_undeposited(&pool).await.unwrap();
    let payment_id = waiting[0].id;
    crate::banking::post_bank_deposit(
        &pool,
        DepositInput { bank_account_id: checking, deposit_date: d("2026-03-11"), memo: None, items: vec![crate::banking::DepositItemRef { kind: "PAYMENT".into(), id: payment_id }], other_lines: vec![], cash_back: None, deductions: Vec::new() },
    )
    .await
    .unwrap();
    let entry_of = |t: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM journal_entries WHERE source_type = ?1").bind(t);
    let (invoice_entry, payment_entry, deposit_entry) =
        (entry_of("INVOICE").fetch_one(&pool).await.unwrap(), entry_of("RECEIVE_PAYMENT").fetch_one(&pool).await.unwrap(), entry_of("DEPOSIT").fetch_one(&pool).await.unwrap());

    let links = |h: crate::registers::history::TransactionHistory| h.links.into_iter().map(|l| (l.relation, l.entry.journal_entry_id, l.amount_cents)).collect::<Vec<_>>();
    assert_eq!(links(transaction_history(&pool, invoice_entry).await.unwrap()), [("Payment".to_string(), payment_entry, 20_000)]);
    assert_eq!(
        links(transaction_history(&pool, payment_entry).await.unwrap()),
        [("Invoice".to_string(), invoice_entry, 20_000), ("Deposit".to_string(), deposit_entry, 20_000)]
    );
    assert_eq!(links(transaction_history(&pool, deposit_entry).await.unwrap()), [("Payment".to_string(), payment_entry, 20_000)]);

    // A check typed into the register has no linked transactions.
    let rent = account(&pool, "6300").await;
    let check = post_register_entry(&pool, row(checking, RegisterSide::Payment, None, None, vec![split(rent, 500)])).await.unwrap();
    let history = transaction_history(&pool, check.journal_entry_id).await.unwrap();
    assert_eq!((history.entry.source_type.as_str(), history.links.len()), ("CHECK", 0));
    assert!(transaction_history(&pool, 999_999).await.unwrap_err().starts_with("JE_NOT_FOUND"));
}

#[tokio::test]
async fn a_transaction_journal_report_shows_only_that_entry() {
    let pool = test_pool().await;
    let (checking, rent, sales) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "4000").await);
    let check = post_register_entry(&pool, row(checking, RegisterSide::Payment, None, None, vec![split(rent, 700)])).await.unwrap();
    post_register_entry(&pool, row(checking, RegisterSide::Deposit, None, None, vec![split(sales, 900)])).await.unwrap();
    let journal = crate::custom_reports::presets().into_iter().find(|p| p.id == "journal").unwrap().settings;
    let settings = crate::custom_reports::ReportSettings {
        filters: crate::custom_reports::Filters { entry_ids: vec![check.journal_entry_id], ..journal.filters.clone() },
        start_date: None,
        end_date: Some(d("2026-12-31")),
        ..journal
    };
    let report = crate::custom_reports::run(&pool, &settings, d("2026-12-31")).await.unwrap();
    let entries: std::collections::BTreeSet<i64> = report.rows.iter().filter_map(|r| r.journal_entry_id).collect();
    assert_eq!(entries.into_iter().collect::<Vec<_>>(), [check.journal_entry_id]);
}

#[tokio::test]
async fn another_bank_account_in_the_entry_row_records_a_transfer() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let savings: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Reserve Savings', 'BANK') RETURNING id").fetch_one(&pool).await.unwrap();
    let out = post_register_entry(&pool, row(checking, RegisterSide::Payment, None, Some("1001"), vec![split(savings, 25_000)])).await.unwrap();
    assert_eq!(out.source_type, "TRANSFER");
    let back = post_register_entry(&pool, row(checking, RegisterSide::Deposit, None, None, vec![split(savings, 5_000)])).await.unwrap();
    assert_eq!(back.source_type, "TRANSFER");
    let (from, to, cents): (i64, i64, i64) =
        sqlx::query_as("SELECT from_account_id, to_account_id, amount_cents FROM transfers WHERE id = ?1").bind(out.source_id).fetch_one(&pool).await.unwrap();
    assert_eq!((from, to, cents), (checking, savings, 25_000));
    assert_eq!(account_register(&pool, checking, None, None, false).await.unwrap().ending_balance_cents, -20_000);
    assert_eq!(account_register(&pool, savings, None, None, false).await.unwrap().ending_balance_cents, 20_000);
    // No check was written, so the number stays free.
    assert_eq!(crate::expenses::next_check_number(&pool, checking).await.unwrap(), "1001");
}

#[tokio::test]
async fn an_undeposited_funds_deposit_in_the_entry_row_banks_the_waiting_payments() {
    let pool = test_pool().await;
    let (checking, sales) = (account(&pool, "1010").await, account(&pool, "4000").await);
    let undeposited: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_ASSET' AND is_system = 1").fetch_one(&pool).await.unwrap();
    let receipt = |cents: i64| -> crate::sales::SalesReceiptInput {
        serde_json::from_value(serde_json::json!({ "customerName": "Walk-in", "receiptDate": "2026-03-01", "depositAccountId": undeposited, "paymentMethod": "CASH",
            "lines": [{ "description": "Sale", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": sales, "isTaxable": false }] }))
        .unwrap()
    };
    for cents in [4_000, 6_000, 2_500] {
        crate::sales::post_sales_receipt(&pool, receipt(cents)).await.unwrap();
    }
    // One payment that is the amount.
    let one = post_register_entry(&pool, row(checking, RegisterSide::Deposit, None, None, vec![split(undeposited, 6_000)])).await.unwrap();
    assert_eq!(one.source_type, "DEPOSIT");
    assert_eq!(crate::banking::list_undeposited(&pool).await.unwrap().len(), 2);
    // No single payment or the whole lot: refused, nothing posted.
    let err = post_register_entry(&pool, row(checking, RegisterSide::Deposit, None, None, vec![split(undeposited, 4_100)])).await.unwrap_err();
    assert!(err.starts_with("REG_ENTRY_UNDEPOSITED_NO_MATCH"), "{err}");
    // All that is left, together.
    post_register_entry(&pool, row(checking, RegisterSide::Deposit, None, None, vec![split(undeposited, 6_500)])).await.unwrap();
    assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty());
    let reg = account_register(&pool, checking, None, None, false).await.unwrap();
    assert_eq!(reg.ending_balance_cents, 12_500);
}
