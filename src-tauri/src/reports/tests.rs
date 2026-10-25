use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

#[tokio::test]
async fn trial_balance_balances_and_series_accumulates() {
    let pool = test_pool().await;
    let (checking, card, owner, rent) = (
        account_id(&pool, "1010").await,
        account_id(&pool, "2100").await,
        account_id(&pool, "3100").await,
        account_id(&pool, "6300").await,
    );
    let post = |date, lines| post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines, currency: None });

    post(ymd(2025, 6, 1), vec![JournalLine::debit(checking, 500_000), JournalLine::credit(owner, 500_000)])
        .await
        .unwrap();
    post(ymd(2026, 9, 5), vec![JournalLine::debit(rent, 120_000), JournalLine::credit(card, 120_000)])
        .await
        .unwrap();
    post(ymd(2026, 10, 2), vec![JournalLine::debit(card, 20_000), JournalLine::credit(checking, 20_000)])
        .await
        .unwrap();

    let tb = trial_balance(&pool).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
    assert_eq!(tb.total_debit_cents, 600_000);
    assert_eq!(tb.rows.len(), 4);

    let series = asset_liability_series(&pool, ymd(2026, 10, 13)).await.unwrap();
    assert_eq!(series.len(), 12);
    assert_eq!(series[0], MonthPoint { month: "2025-11".into(), assets_cents: 500_000, liabilities_cents: 0 });
    assert_eq!(series[10], MonthPoint { month: "2026-09".into(), assets_cents: 500_000, liabilities_cents: 120_000 });
    assert_eq!(series[11], MonthPoint { month: "2026-10".into(), assets_cents: 480_000, liabilities_cents: 100_000 });

    let dash = dashboard(&pool, ymd(2026, 10, 13)).await.unwrap();
    assert_eq!((dash.total_assets_cents, dash.total_liabilities_cents), (480_000, 100_000));
}
