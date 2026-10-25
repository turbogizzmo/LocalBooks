use super::*;
use crate::db::test_pool;
use crate::statements::balance_sheet;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

fn bill(vendor: &str, lines: Vec<(i64, i64)>) -> BillInput {
    BillInput {
        vendor_name: vendor.into(),
        bill_number: Some("INV-778".into()),
        bill_date: d(1),
        due_date: d(31),
        memo: None,
        total_cents: lines.iter().map(|(_, cents)| cents).sum(),
        lines: lines.into_iter().map(|(account, cents)| BillLineInput{ account_id: account, amount_cents: cents, memo: Some("Line".into()), quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None }).collect(),
    }
}

fn pay(bill_id: i64, account: i64, cents: i64) -> BillPaymentInput {
    BillPaymentInput { bill_id, payment_account_id: account, payment_date: d(10), amount_cents: cents, reference: Some("1001".into()) }
}

#[tokio::test]
async fn bill_accrues_payable_and_payments_relieve_it() {
    let pool = test_pool().await;
    let (ap, checking, card, rent, utilities) = (
        account_id(&pool, "2000").await,
        account_id(&pool, "1010").await,
        account_id(&pool, "2100").await,
        account_id(&pool, "6300").await,
        account_id(&pool, "6500").await,
    );

    let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 150_000), (utilities, 12_345)])).await.expect("bill posts");
    assert_eq!((posted.status.as_str(), posted.total_cents, posted.line_count), ("UNPAID", 162_345, 2));
    assert_eq!(balance(&pool, ap).await, 162_345);
    assert_eq!(balance(&pool, rent).await, 150_000);
    assert_eq!(balance(&pool, utilities).await, 12_345);

    let partial = pay_vendor_bill(&pool, pay(posted.id, checking, 100_000)).await.expect("partial payment");
    assert_eq!((partial.status.as_str(), partial.balance_cents), ("PARTIAL", 62_345));
    let err = pay_vendor_bill(&pool, pay(posted.id, checking, 62_346)).await.expect_err("overpayment");
    assert!(err.starts_with("BILL_OVERPAYMENT"), "{err}");

    let paid = pay_vendor_bill(&pool, pay(posted.id, card, 62_345)).await.expect("final payment by card");
    assert_eq!((paid.status.as_str(), paid.balance_cents), ("PAID", 0));
    assert_eq!(balance(&pool, ap).await, 0);
    assert_eq!(balance(&pool, checking).await, -100_000);
    assert_eq!(balance(&pool, card).await, 62_345);

    let vendors = list_vendors(&pool).await.unwrap();
    assert_eq!((vendors.len(), vendors[0].open_balance_cents), (1, 0));
    assert!(balance_sheet(&pool, d(31)).await.unwrap().is_balanced);
}

#[tokio::test]
async fn bill_rejects_bad_categories_and_rolls_back() {
    let pool = test_pool().await;
    let (rent, sales, ar) = (account_id(&pool, "6300").await, account_id(&pool, "4000").await, account_id(&pool, "1200").await);
    for bad in [sales, ar] {
        let err = post_vendor_bill(&pool, bill("Oops Inc", vec![(rent, 1_000), (bad, 500)])).await.expect_err("bad category");
        assert!(err.starts_with("BILL_BAD_CATEGORY: line 2"), "{err}");
    }
    let (bills, entries, vendors): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM bills), (SELECT COUNT(*) FROM journal_entries), (SELECT COUNT(*) FROM vendors)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((bills, entries, vendors), (0, 0, 0));

    let mut early = bill("Late Co", vec![(rent, 1_000)]);
    early.due_date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    assert!(post_vendor_bill(&pool, early).await.unwrap_err().starts_with("BILL_DUE_BEFORE_DATE"));
}

#[tokio::test]
async fn split_bill_must_match_total_and_debits_each_account_once() {
    let pool = test_pool().await;
    let (ap, rent, utilities) = (account_id(&pool, "2000").await, account_id(&pool, "6300").await, account_id(&pool, "6500").await);

    let mut short = bill("Landlord LLC", vec![(rent, 100_000), (utilities, 20_000)]);
    short.total_cents = 125_000;
    let err = post_vendor_bill(&pool, short).await.expect_err("split must match total");
    assert!(err.starts_with("BILL_SPLIT_MISMATCH") && err.contains("$50.00 unallocated"), "{err}");

    // Two splits to the same account collapse into one debit line on the journal entry.
    let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 100_000), (utilities, 20_000), (rent, 5_000)])).await.unwrap();
    let lines: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT account_id, debit_cents, credit_cents FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
    )
    .bind(posted.journal_entry_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(lines, vec![(rent, 105_000, 0), (utilities, 20_000, 0), (ap, 0, 125_000)]);
    assert_eq!((posted.line_count, posted.category_account_id), (3, None));
}

#[tokio::test]
async fn expenses_and_checks_credit_the_payment_account() {
    let pool = test_pool().await;
    let (checking, card, rent, fuel, ar) =
        (account_id(&pool, "1010").await, account_id(&pool, "2100").await, account_id(&pool, "6300").await, account_id(&pool, "6100").await, account_id(&pool, "1200").await);
    let input = |kind: &str, account: i64, lines: Vec<(i64, i64)>| ExpenseInput {
        kind: kind.into(),
        payee_name: Some("Shell".into()),
        payment_account_id: account,
        expense_date: d(5),
        check_number: Some("1042".into()),
        payment_method: None,
        memo: None,
        lines: lines.into_iter().map(|(account_id, amount_cents)| SplitLineInput { account_id, amount_cents, memo: None, class_id: None }).collect(),
        currency: None,
    };

    let check = post_expense(&pool, input("CHECK", checking, vec![(rent, 90_000), (fuel, 4_000)])).await.expect("check posts");
    assert_eq!((check.kind.as_str(), check.check_number.as_deref(), check.total_cents), ("CHECK", Some("1042"), 94_000));
    let card_expense = post_expense(&pool, input("EXPENSE", card, vec![(fuel, 6_000)])).await.expect("card expense posts");
    assert_eq!(card_expense.check_number, None);

    assert_eq!(balance(&pool, checking).await, -94_000);
    assert_eq!(balance(&pool, card).await, 6_000);
    assert_eq!(balance(&pool, fuel).await, 10_000);

    let err = post_expense(&pool, input("CHECK", card, vec![(fuel, 100)])).await.expect_err("checks need a bank");
    assert!(err.starts_with("EXP_BAD_PAYMENT_ACCOUNT"), "{err}");
    let err = post_expense(&pool, input("EXPENSE", checking, vec![(ar, 100)])).await.expect_err("A/R needs a customer");
    assert!(err.starts_with("EXP_BAD_CATEGORY"), "{err}");
    assert_eq!(list_expenses(&pool).await.unwrap().len(), 2);
}

#[tokio::test]
async fn pay_bills_is_all_or_nothing() {
    let pool = test_pool().await;
    let (checking, rent) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await);
    let a = post_vendor_bill(&pool, bill("A Co", vec![(rent, 10_000)])).await.unwrap();
    let b = post_vendor_bill(&pool, bill("B Co", vec![(rent, 20_000)])).await.unwrap();

    let err = pay_vendor_bills(&pool, vec![pay(a.id, checking, 10_000), pay(b.id, checking, 99_999)]).await.expect_err("second overpays");
    assert!(err.starts_with("BILL_OVERPAYMENT"), "{err}");
    assert_eq!(balance(&pool, checking).await, 0, "first payment rolled back too");

    assert_eq!(pay_vendor_bills(&pool, vec![pay(a.id, checking, 10_000), pay(b.id, checking, 5_000)]).await.unwrap(), 2);
    assert_eq!(balance(&pool, checking).await, -15_000);
    assert_eq!(list_bill_payments(&pool, b.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn posted_bills_are_immutable_at_the_database_layer() {
    let pool = test_pool().await;
    let rent = account_id(&pool, "6300").await;
    let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 5_000)])).await.unwrap();
    let err = sqlx::query("UPDATE bills SET total_cents = 1 WHERE id = ?1").bind(posted.id).execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("BILL_IMMUTABLE"), "{err}");
    let err = sqlx::query("DELETE FROM bill_lines WHERE bill_id = ?1").bind(posted.id).execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("BILL_IMMUTABLE"), "{err}");
}

#[tokio::test]
async fn foreign_expense_credits_the_bank_in_home_currency() {
    let pool = test_pool().await;
    let (checking, travel, meals) = (account_id(&pool, "1010").await, account_id(&pool, "6450").await, account_id(&pool, "6200").await);
    let posted = post_expense(
        &pool,
        ExpenseInput {
            kind: "EXPENSE".into(),
            payee_name: Some("Hotel Tokyo".into()),
            payment_account_id: checking,
            expense_date: d(3),
            check_number: None,
            payment_method: Some("CREDIT_CARD".into()),
            memo: None,
            lines: vec![
                SplitLineInput { account_id: travel, amount_cents: 3_200_000, memo: Some("2 nights".into()), class_id: None },
                SplitLineInput { account_id: meals, amount_cents: 450_050, memo: None, class_id: None },
            ],
            currency: Some(ForeignCurrency { code: "JPY".into(), exchange_rate_e6: 6_667 }),
        },
    )
    .await
    .expect("foreign expense posts");
    assert_eq!((posted.currency_code.as_deref(), posted.total_cents), (Some("JPY"), 3_650_050));
    let (t, m, c) = (balance(&pool, travel).await, balance(&pool, meals).await, balance(&pool, checking).await);
    // ¥3,200,000 → $213.344 and ¥450,050 → $30.0048 round to $243.34, but the ¥3,650,050 total
    // converts to $243.35; the $0.01 difference lands on the largest debit (travel).
    assert_eq!((t, m), (21_335, 3_000));
    assert_eq!(c, -(t + m), "the bank credit equals the converted debits");
    assert_eq!(posted.home_total_cents, t + m);
}

#[tokio::test]
async fn bill_lines_can_be_priced_by_quantity_and_rate() {
    let pool = test_pool().await;
    let (ap, supplies) = (account_id(&pool, "2000").await, account_id(&pool, "6250").await);
    let priced = |q: i64, r: i64, amount: i64| BillLineInput{ account_id: supplies, amount_cents: amount, memo: Some("Paper".into()), quantity_e4: Some(q), rate_cents: Some(r), product_service_id: None, class_id: None };
    let mut input = bill("Staples", vec![]);
    input.lines = vec![priced(25_000, 1_299, 3_248)];
    input.total_cents = 3_248;
    post_vendor_bill(&pool, input.clone()).await.expect("2.5 × $12.99 = $32.475 → $32.48");
    assert_eq!(balance(&pool, ap).await, 3_248);

    input.lines = vec![priced(25_000, 1_299, 3_247)];
    input.total_cents = 3_247;
    assert!(post_vendor_bill(&pool, input.clone()).await.unwrap_err().starts_with("BILL_LINE_MISMATCH"));
    input.lines = vec![BillLineInput { quantity_e4: Some(10_000), rate_cents: None, ..priced(0, 0, 3_247) }];
    assert!(post_vendor_bill(&pool, input).await.unwrap_err().starts_with("BILL_LINE_INCOMPLETE"));
}
