use super::*;
use crate::db::test_pool;
use crate::expenses::{post_expense, post_vendor_bill, BillInput, BillLineInput, ExpenseInput, SplitLineInput};
use crate::ledger::{JournalEntry, JournalLine};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn check(account: i64, rent: i64, number: &str, date: NaiveDate) -> ExpenseInput {
    ExpenseInput { form_lines: Vec::new(),
        kind: "CHECK".into(),
        payee_name: Some("Harbor Hardware".into()),
        payment_account_id: account,
        expense_date: date,
        check_number: Some(number.into()),
        payment_method: None,
        payment_method_id: None,
        memo: None,
        lines: vec![SplitLineInput { account_id: rent, amount_cents: 5_000, memo: None, class_id: None, customer_id: None, billable: None }],
        currency: None,
    }
}

fn bill(vendor: &str, number: &str, expense: i64, date: NaiveDate) -> BillInput {
    BillInput { form_lines: Vec::new(),
        vendor_name: vendor.into(),
        bill_number: Some(number.into()),
        bill_date: date,
        due_date: date,
        memo: None,
        total_cents: 2_500,
        terms_id: None,
        lines: vec![BillLineInput { account_id: expense, amount_cents: 2_500, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None, customer_id: None, billable: None }],
    }
}

fn codes(warnings: &[Warning]) -> Vec<&str> {
    warnings.iter().map(|w| w.code.as_str()).collect()
}

#[test]
fn confirmation_lists_only_the_warnings_not_yet_confirmed() {
    let w = |c: &str| Warning::new(c, format!("{c} happened."));
    assert_eq!(confirm(vec![], None), Ok(()));
    let err = confirm(vec![w("A"), w("B")], Some(&["A".to_string()])).unwrap_err();
    assert!(err.starts_with("NEEDS_CONFIRMATION: "), "{err}");
    assert_eq!(warnings_in(&err).unwrap(), vec![w("B")]);
    assert_eq!(confirm(vec![w("A"), w("B")], Some(&["A".to_string(), "B".to_string()])), Ok(()));
    assert_eq!(warnings_in("JE_UNBALANCED: no"), None);
}

#[tokio::test]
async fn dates_far_in_the_past_or_future_warn_by_the_set_days() {
    let pool = test_pool().await;
    let today = d(2026, 11, 6);
    let defaults = preferences::get_all(&pool).await.unwrap();
    assert_eq!((defaults[WARN_PAST_DAYS].as_str(), defaults[WARN_FUTURE_DAYS].as_str()), ("90", "30"));

    assert_eq!(date_warning(&pool, d(2026, 8, 8), today).await.unwrap(), None, "exactly 90 days back is fine");
    assert_eq!(date_warning(&pool, d(2026, 8, 7), today).await.unwrap().unwrap().code, "DATE_FAR_PAST");
    assert_eq!(date_warning(&pool, d(2026, 12, 6), today).await.unwrap(), None, "exactly 30 days ahead is fine");
    let future = date_warning(&pool, d(2026, 12, 7), today).await.unwrap().unwrap();
    assert_eq!(future.code, "DATE_FAR_FUTURE");
    assert!(future.message.contains("31 days in the future"), "{}", future.message);

    // Two-digit-year slips such as 2062 for 2026 are what this catches; 0 turns a side off.
    preferences::set(&pool, WARN_FUTURE_DAYS, "0").await.unwrap();
    assert_eq!(date_warning(&pool, d(2062, 1, 1), today).await.unwrap(), None);
    preferences::set(&pool, WARN_PAST_DAYS, "7").await.unwrap();
    assert_eq!(date_warning(&pool, d(2026, 10, 29), today).await.unwrap().unwrap().code, "DATE_FAR_PAST");
    for bad in ["-1", "3651", "ten"] {
        assert!(preferences::set(&pool, WARN_PAST_DAYS, bad).await.unwrap_err().starts_with("PREF_BAD_WARN_DAYS"), "{bad}");
    }
}

#[tokio::test]
async fn duplicate_check_numbers_warn_per_account_and_ignore_voided_and_replaced_checks() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let today = Local::now().date_naive();
    let first = post_expense(&pool, check(checking, rent, "1001", today)).await.unwrap();

    let again = check(checking, rent, " 1001 ", today);
    let warnings = warnings_on(&pool, &expense(&again, None), today).await.unwrap();
    assert_eq!(codes(&warnings), ["DUPLICATE_CHECK_NUMBER"]);
    assert!(warnings[0].message.contains("Harbor Hardware"), "{}", warnings[0].message);
    // Editing that same check keeps its number without a warning.
    assert!(warnings_on(&pool, &expense(&again, Some(first.id)), today).await.unwrap().is_empty());
    // Another number, or a non-check expense, is fine.
    assert!(warnings_on(&pool, &expense(&check(checking, rent, "1002", today), None), today).await.unwrap().is_empty());
    let mut card = check(checking, rent, "1001", today);
    card.kind = "EXPENSE".into();
    assert!(warnings_on(&pool, &expense(&card, None), today).await.unwrap().is_empty());

    // The guard runs through `check`: refused until confirmed, then it saves.
    let err = super::check(&pool, expense(&again, None), None).await.unwrap_err();
    assert_eq!(codes(&warnings_in(&err).unwrap()), ["DUPLICATE_CHECK_NUMBER"]);
    super::check(&pool, expense(&again, None), Some(&["DUPLICATE_CHECK_NUMBER".to_string()])).await.unwrap();

    // A voided check's number may be written again.
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE id = ?1").bind(first.id).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert!(warnings_on(&pool, &expense(&again, None), today).await.unwrap().is_empty());

    preferences::set(&pool, WARN_DUPLICATE_CHECKS, "false").await.unwrap();
    post_expense(&pool, check(checking, rent, "2001", today)).await.unwrap();
    assert!(warnings_on(&pool, &expense(&check(checking, rent, "2001", today), None), today).await.unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_bill_numbers_warn_only_for_the_same_vendor() {
    let pool = test_pool().await;
    let supplies = account(&pool, "6300").await;
    let today = Local::now().date_naive();
    let first = post_vendor_bill(&pool, bill("Acme Supply", "INV-9", supplies, today)).await.unwrap();

    let same = bill("acme supply", "inv-9", supplies, today);
    let warnings = warnings_on(&pool, &super::bill(&same, None), today).await.unwrap();
    assert_eq!(codes(&warnings), ["DUPLICATE_BILL_NUMBER"]);
    assert!(warnings_on(&pool, &super::bill(&same, Some(first.id)), today).await.unwrap().is_empty(), "editing the bill itself");
    assert!(warnings_on(&pool, &super::bill(&bill("Other Vendor", "INV-9", supplies, today), None), today).await.unwrap().is_empty());

    preferences::set(&pool, WARN_DUPLICATE_BILLS, "false").await.unwrap();
    assert!(warnings_on(&pool, &super::bill(&same, None), today).await.unwrap().is_empty());
}

#[tokio::test]
async fn posting_to_retained_earnings_warns_until_turned_off() {
    let pool = test_pool().await;
    let (checking, retained) = (account(&pool, "1010").await, account(&pool, "3900").await);
    let today = Local::now().date_naive();
    let entry = JournalEntry {
        id: None,
        date: today,
        description: "Prior-year correction".into(),
        lines: vec![JournalLine::debit(retained, 1_000), JournalLine::credit(checking, 1_000)],
        currency: None,
    };
    let warnings = warnings_on(&pool, &journal(&entry), today).await.unwrap();
    assert_eq!(codes(&warnings), ["POSTS_TO_RETAINED_EARNINGS"]);
    assert!(warnings[0].message.contains("Retained Earnings"));

    // Warnings add up: a far date and Retained Earnings are asked together, once.
    let mut old = entry.clone();
    old.date = d(2020, 1, 1);
    let err = super::check(&pool, journal(&old), None).await.unwrap_err();
    assert_eq!(codes(&warnings_in(&err).unwrap()), ["DATE_FAR_PAST", "POSTS_TO_RETAINED_EARNINGS"]);

    preferences::set(&pool, WARN_RETAINED_EARNINGS, "false").await.unwrap();
    assert!(warnings_on(&pool, &journal(&entry), today).await.unwrap().is_empty());
}

#[tokio::test]
async fn new_preferences_have_defaults_without_a_migration_and_validate() {
    let pool = test_pool().await;
    assert_eq!(preferences::get(&pool, "four_digit_years").await.unwrap(), "true");
    assert_eq!(preferences::get(&pool, "hours_format").await.unwrap(), "hhmm");
    assert_eq!(preferences::set(&pool, "hours_format", "decimal").await.unwrap(), "decimal");
    assert_eq!(preferences::get_all(&pool).await.unwrap()["hours_format"], "decimal");
    assert!(preferences::set(&pool, "hours_format", "minutes").await.is_err());
    assert!(preferences::set(&pool, "four_digit_years", "yes").await.is_err());
}

/// Preferences › Items & Inventory › when a sale would take stock below zero: warn (the default),
/// stop, or say nothing. An edit counts the units of the sale it replaces as back in stock.
#[tokio::test]
async fn selling_below_zero_warns_stops_or_is_allowed() {
    use serde_json::json;
    let pool = test_pool().await;
    let sales = account(&pool, "4000").await;
    let product = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Widget".into(), description: None, rate_cents: 2_500, income_account_id: sales, is_taxable: false })
        .await
        .unwrap()
        .id;
    let item = crate::inventory::track_product(&pool, &crate::inventory::TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap();
    let equity = account(&pool, "3000").await;
    crate::inventory::adjust(&pool, &crate::inventory::AdjustInput { item_id: item.id, date: d(2026, 4, 1), qty_change_e4: 50_000, total_cost_cents: Some(5_000), offset_account_id: equity, memo: None })
        .await
        .unwrap();
    let sale = |qty_e4: i64| -> crate::invoices::InvoiceInput {
        serde_json::from_value(json!({ "customerName": "Acme", "issueDate": "2026-04-10", "dueDate": "2026-05-10",
            "lines": [{ "description": "Widget", "quantityE4": qty_e4, "rateCents": 2_500, "incomeAccountId": sales, "productServiceId": product },
                      { "description": "Delivery", "quantityE4": 10_000, "rateCents": 1_000, "incomeAccountId": sales }] }))
        .unwrap()
    };
    let today = d(2026, 4, 10);
    let codes = |w: Vec<Warning>| w.into_iter().map(|w| w.code).collect::<Vec<_>>();

    // 5 on hand: selling 5 is fine, selling 7 warns.
    assert!(codes(warnings_on(&pool, &invoice(&sale(50_000), None), today).await.unwrap()).is_empty());
    let warned = warnings_on(&pool, &invoice(&sale(70_000), None), today).await.unwrap();
    assert_eq!(codes(warned.clone()), ["NEGATIVE_STOCK"]);
    assert!(warned[0].message.contains("Widget (5 on hand, 7 on this sale)"), "{}", warned[0].message);
    let answered = ["NEGATIVE_STOCK".to_string(), "DATE_FAR_PAST".to_string(), "DATE_FAR_FUTURE".to_string()];
    assert!(super::check(&pool, invoice(&sale(70_000), None), Some(&answered)).await.is_ok(), "Save anyway");

    // Selling 4 leaves 1; an edit of that invoice to 5 is fine, because its 4 come back first.
    let first = crate::invoices::create_and_post_invoice(&pool, sale(40_000)).await.unwrap();
    assert_eq!(codes(warnings_on(&pool, &invoice(&sale(50_000), None), today).await.unwrap()), ["NEGATIVE_STOCK"], "a new sale of 5 with 1 left");
    assert!(codes(warnings_on(&pool, &invoice(&sale(50_000), first.journal_entry_id), today).await.unwrap()).is_empty(), "the edit returns its own 4");

    // Stop refuses outright; Save anyway can't answer it.
    crate::preferences::set(&pool, "negative_stock", "stop").await.unwrap();
    let stopped = super::check(&pool, invoice(&sale(20_000), None), Some(&answered)).await.unwrap_err();
    assert!(stopped.starts_with("INV_NOT_ENOUGH_STOCK"), "{stopped}");
    // Sales receipts are guarded the same way.
    assert!(super::check(&pool, sale_posting(product, 20_000), None).await.unwrap_err().starts_with("INV_NOT_ENOUGH_STOCK"));

    // Off: no warning; the sale is costed at the last cost, as before the preference.
    crate::preferences::set(&pool, "negative_stock", "off").await.unwrap();
    assert!(codes(warnings_on(&pool, &invoice(&sale(20_000), None), today).await.unwrap()).is_empty());
    assert!(crate::preferences::set(&pool, "negative_stock", "maybe").await.unwrap_err().starts_with("PREF_BAD_NEGATIVE_STOCK"));
}

fn sale_posting(product: i64, qty_e4: i64) -> Posting<'static> {
    stock_sale(d(2026, 4, 10), [(Some(product), qty_e4), (None, 10_000)].into_iter(), None)
}

/// Preferences › Items & Inventory › inventory and purchase orders off: no new tracked items or
/// purchase orders; items already tracked keep working.
#[tokio::test]
async fn inventory_can_be_turned_off() {
    let pool = test_pool().await;
    let sales = account(&pool, "4000").await;
    let product = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Bolt".into(), description: None, rate_cents: 100, income_account_id: sales, is_taxable: false }).await.unwrap().id;
    crate::preferences::set(&pool, "inventory_enabled", "false").await.unwrap();
    let track = crate::inventory::TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None };
    assert!(crate::inventory::track_product(&pool, &track).await.unwrap_err().starts_with("INV_OFF"));
    let po: crate::workflows::PurchaseOrderInput = serde_json::from_value(serde_json::json!({ "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [{ "description": "Bolts", "quantityE4": 10_000, "rateCents": 100, "amountCents": 100, "accountId": account(&pool, "6250").await }] })).unwrap();
    assert!(crate::workflows::create_purchase_order(&pool, po).await.unwrap_err().starts_with("INV_OFF"));
    crate::preferences::set(&pool, "inventory_enabled", "true").await.unwrap();
    crate::inventory::track_product(&pool, &track).await.unwrap();
}

#[tokio::test]
async fn paying_a_vendor_or_customer_around_open_bills_and_invoices_warns() {
    let pool = test_pool().await;
    let (checking, supplies, services) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "4100").await);
    let today = d(2026, 5, 1);
    let mut pay = check(checking, supplies, "501", today);
    pay.payee_name = Some("Cedar Lumber".into());
    assert!(warnings_on(&pool, &expense(&pay, None), today).await.unwrap().is_empty(), "no bills yet");

    post_vendor_bill(&pool, bill("Cedar Lumber", "C-1", supplies, today)).await.unwrap();
    let warnings = warnings_on(&pool, &expense(&pay, None), today).await.unwrap();
    assert_eq!(codes(&warnings), ["VENDOR_OPEN_BILLS"]);
    assert!(warnings[0].message.contains("1 open bill with $25.00"), "{}", warnings[0].message);
    // An expense paid by card too; not a card credit, and not an edit of a saved check.
    pay.kind = "EXPENSE".into();
    assert_eq!(codes(&warnings_on(&pool, &expense(&pay, None), today).await.unwrap()), ["VENDOR_OPEN_BILLS"]);
    pay.kind = "CARD_CREDIT".into();
    assert!(warnings_on(&pool, &expense(&pay, None), today).await.unwrap().is_empty());
    pay.kind = "CHECK".into();
    assert!(warnings_on(&pool, &expense(&pay, Some(1)), today).await.unwrap().is_empty());
    crate::preferences::set(&pool, WARN_VENDOR_OPEN_BILLS, "false").await.unwrap();
    assert!(warnings_on(&pool, &expense(&pay, None), today).await.unwrap().is_empty());

    let receipt = |customer: &str| -> crate::sales::SalesReceiptInput {
        serde_json::from_value(serde_json::json!({ "customerName": customer, "receiptDate": "2026-05-01", "depositAccountId": checking, "paymentMethod": "CASH",
            "lines": [{ "description": "Mulch", "quantityE4": 10_000, "rateCents": 4_000, "incomeAccountId": services, "isTaxable": false }] })).unwrap()
    };
    assert!(warnings_on(&pool, &sales_receipt(&receipt("Elm Street")), today).await.unwrap().is_empty());
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Elm Street:Front Yard", "issueDate": "2026-05-01", "dueDate": "2026-05-31",
        "lines": [{ "description": "Pruning", "quantityE4": 10_000, "rateCents": 30_000, "incomeAccountId": services, "isTaxable": false }] })).unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    // The customer's job's invoice counts, and so does the job's own.
    assert_eq!(codes(&warnings_on(&pool, &sales_receipt(&receipt("Elm Street:Front Yard")), today).await.unwrap()), ["CUSTOMER_OPEN_INVOICES"]);
    let warnings = warnings_on(&pool, &sales_receipt(&receipt("Elm Street")), today).await.unwrap();
    assert_eq!(codes(&warnings), ["CUSTOMER_OPEN_INVOICES"]);
    assert!(warnings[0].message.contains("1 open invoice with $300.00"), "{}", warnings[0].message);
    crate::preferences::set(&pool, WARN_CUSTOMER_OPEN_INVOICES, "false").await.unwrap();
    assert!(warnings_on(&pool, &sales_receipt(&receipt("Elm Street")), today).await.unwrap().is_empty());
}
