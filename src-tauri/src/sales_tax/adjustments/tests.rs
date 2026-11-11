use super::*;
use crate::db::test_pool;
use crate::sales_tax::combined::{save_combined_rate, sales_tax_return, CombinedRateInput};
use crate::sales_tax::{create_tax_rate, Basis, TaxRateInput};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn owed(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, day).unwrap()
}

#[tokio::test]
async fn an_adjustment_moves_the_rates_liability_and_shows_on_its_agencys_return() {
    let pool = test_pool().await;
    let (payable, services, misc) = (account(&pool, "2200").await, account(&pool, "4100").await, account(&pool, "6000").await);
    let state = create_tax_rate(&pool, TaxRateInput { name: "State".into(), rate_ppm: 60_000, agency_name: Some("State DOR".into()), liability_account_id: None }).await.unwrap();

    // A $2.00 discount the state gives for filing on time lowers what is owed (Dr payable / Cr income).
    let lower = SalesTaxAdjustmentInput { tax_rate_id: state.id, adjustment_date: d(20), amount_cents: -200, account_id: services, memo: Some("Timely filing discount".into()) };
    adjust_sales_tax(&pool, &lower).await.unwrap();
    assert_eq!(owed(&pool, payable).await, -200);
    // A $5.00 penalty raises it (Dr expense / Cr payable).
    let raise = SalesTaxAdjustmentInput { tax_rate_id: state.id, adjustment_date: d(21), amount_cents: 500, account_id: misc, memo: None };
    let penalty = adjust_sales_tax(&pool, &raise).await.unwrap();
    assert_eq!(owed(&pool, payable).await, 300);

    let r = sales_tax_return(&pool, d(1), d(31), Basis::Accrual).await.unwrap();
    assert_eq!(r.agencies.len(), 1);
    assert_eq!((r.agencies[0].agency_name.as_str(), r.agencies[0].tax_cents), ("State DOR", 300));
    assert_eq!((r.agencies[0].lines[0].adjustment_cents, r.agencies[0].lines[0].tax_cents), (300, 300));
    assert!(sales_tax_return(&pool, d(22), d(31), Basis::Accrual).await.unwrap().agencies.is_empty(), "dated in the period only");

    // Voiding takes it off the return; it is not a journal entry Journal Entries edits.
    let listed = list_sales_tax_adjustments(&pool).await.unwrap();
    let entry = listed.iter().find(|a| a.id == penalty).unwrap().journal_entry_id;
    let manual = crate::ledger::JournalEntry { id: None, date: d(21), description: "x".into(), lines: vec![crate::ledger::JournalLine::debit(misc, 1), crate::ledger::JournalLine::credit(payable, 1)], currency: None };
    assert!(crate::edits::replace_journal_entry(&pool, entry, manual).await.unwrap_err().starts_with("EDIT_NOT_MANUAL"));
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert_eq!(sales_tax_return(&pool, d(1), d(31), Basis::Accrual).await.unwrap().agencies[0].tax_cents, -200);
}

#[tokio::test]
async fn an_adjustment_is_refused_on_a_combined_rate_or_a_balance_sheet_account() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let a = create_tax_rate(&pool, TaxRateInput { name: "State".into(), rate_ppm: 60_000, agency_name: Some("State".into()), liability_account_id: None }).await.unwrap();
    let b = create_tax_rate(&pool, TaxRateInput { name: "City".into(), rate_ppm: 20_000, agency_name: Some("City".into()), liability_account_id: None }).await.unwrap();
    let both = save_combined_rate(&pool, None, &CombinedRateInput { name: "Both".into(), component_ids: vec![a.id, b.id] }).await.unwrap();
    let input = |rate: i64, account: i64, cents: i64| SalesTaxAdjustmentInput { tax_rate_id: rate, adjustment_date: d(1), amount_cents: cents, account_id: account, memo: None };
    assert!(adjust_sales_tax(&pool, &input(both.id, services, 100)).await.unwrap_err().starts_with("TAX_ADJUST_COMBINED"));
    assert!(adjust_sales_tax(&pool, &input(a.id, checking, 100)).await.unwrap_err().starts_with("TAX_ADJUST_ACCOUNT"));
    assert!(adjust_sales_tax(&pool, &input(a.id, services, 0)).await.unwrap_err().starts_with("TAX_BAD_AMOUNT"));
    assert!(list_sales_tax_adjustments(&pool).await.unwrap().is_empty());
}
