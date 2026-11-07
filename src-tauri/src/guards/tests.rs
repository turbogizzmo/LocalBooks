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
    ExpenseInput {
        kind: "CHECK".into(),
        payee_name: Some("Harbor Hardware".into()),
        payment_account_id: account,
        expense_date: date,
        check_number: Some(number.into()),
        payment_method: None,
        payment_method_id: None,
        memo: None,
        lines: vec![SplitLineInput { account_id: rent, amount_cents: 5_000, memo: None, class_id: None, customer_id: None }],
        currency: None,
    }
}

fn bill(vendor: &str, number: &str, expense: i64, date: NaiveDate) -> BillInput {
    BillInput {
        vendor_name: vendor.into(),
        bill_number: Some(number.into()),
        bill_date: date,
        due_date: date,
        memo: None,
        total_cents: 2_500,
        terms_id: None,
        lines: vec![BillLineInput { account_id: expense, amount_cents: 2_500, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None, customer_id: None }],
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
