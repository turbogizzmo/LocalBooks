use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

#[tokio::test]
async fn registers_run_a_balance_from_the_opening_and_name_the_other_side() {
    let pool = test_pool().await;
    let id = |n: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n);
    let (checking, sales, rent, card) = (
        id("1010").fetch_one(&pool).await.unwrap(),
        id("4000").fetch_one(&pool).await.unwrap(),
        id("6300").fetch_one(&pool).await.unwrap(),
        id("2100").fetch_one(&pool).await.unwrap(),
    );
    let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    let post = |date, lines| post_journal_entry(&pool, JournalEntry { id: None, date, description: "x".into(), lines, currency: None });
    post(d(1, 5), vec![JournalLine::debit(checking, 10_000), JournalLine::credit(sales, 10_000)]).await.unwrap();
    post(d(2, 5), vec![JournalLine::debit(rent, 3_000), JournalLine::credit(checking, 3_000)]).await.unwrap();
    post(d(2, 9), vec![JournalLine::debit(rent, 500), JournalLine::debit(card, 500), JournalLine::credit(checking, 1_000)]).await.unwrap();

    let reg = account_register(&pool, checking, Some(d(2, 1)), None, false).await.unwrap();
    assert_eq!(reg.opening_balance_cents, 10_000);
    let rows: Vec<(Option<&str>, i64)> = reg.rows.iter().map(|r| (r.split.as_deref(), r.balance_cents)).collect();
    assert_eq!(rows, [(Some("Rent & Lease"), 7_000), (Some("-Split-"), 6_000)]);
    assert_eq!(reg.ending_balance_cents, 6_000);

    // Credit-normal accounts run positive the other way.
    let income = account_register(&pool, sales, None, None, false).await.unwrap();
    assert_eq!((income.is_debit_normal, income.ending_balance_cents), (false, 10_000));
}
