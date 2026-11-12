use super::*;
use crate::accountant_reports::set_account_tax_line;
use crate::db::test_pool;
use crate::ledger::{JournalEntry, JournalLine};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[test]
fn amounts_have_two_decimals_and_a_leading_minus() {
    assert_eq!(amount_text(0), "0.00");
    assert_eq!(amount_text(5), "0.05");
    assert_eq!(amount_text(-62_500), "-625.00");
    assert_eq!(amount_text(123_456_789), "1234567.89");
    assert_eq!(amount_text(i64::MIN), "-92233720368547758.08");
}

#[test]
fn every_schedule_c_line_has_a_reference_number_or_a_reason() {
    for line in tax_lines().iter().filter(|t| t.form == TXF_FORM) {
        assert!(reference_for(line.line).is_some() || ["13", "30"].contains(&line.line), "Schedule C {} has no TXF reference", line.line);
    }
    let mut refs: Vec<u16> = SCHEDULE_C_REFS.iter().map(|(_, n, _)| *n).collect();
    refs.sort_unstable();
    refs.dedup();
    assert_eq!(refs.len(), SCHEDULE_C_REFS.len(), "each line has its own reference number");
}

#[tokio::test]
async fn the_file_carries_each_schedule_c_line_with_txf_signs() {
    let pool = test_pool().await;
    let (checking, sales, fuel, meals, rent, contractors, utilities) = (
        account(&pool, "1010").await,
        account(&pool, "4000").await,
        account(&pool, "6100").await,
        account(&pool, "6200").await,
        account(&pool, "6300").await,
        account(&pool, "6400").await,
        account(&pool, "6500").await,
    );
    let post = |date, lines| crate::ledger::post_journal_entry(&pool, JournalEntry { id: None, date, description: "Activity".into(), lines, currency: None });
    post(d(2025, 3, 1), vec![JournalLine::debit(checking, 100_000), JournalLine::credit(sales, 100_000)]).await.unwrap();
    post(d(2025, 3, 2), vec![JournalLine::debit(sales, 5_000), JournalLine::credit(checking, 5_000)]).await.unwrap();
    post(d(2025, 4, 1), vec![JournalLine::debit(fuel, 12_345), JournalLine::credit(checking, 12_345)]).await.unwrap();
    post(d(2025, 4, 2), vec![JournalLine::debit(rent, 30_000), JournalLine::credit(checking, 30_000)]).await.unwrap();
    post(d(2025, 4, 3), vec![JournalLine::debit(contractors, 7_000), JournalLine::credit(checking, 7_000)]).await.unwrap();
    post(d(2025, 4, 4), vec![JournalLine::debit(utilities, 4_000), JournalLine::credit(checking, 4_000)]).await.unwrap();
    post(d(2025, 4, 5), vec![JournalLine::debit(meals, 2_500), JournalLine::credit(checking, 2_500)]).await.unwrap();
    post(d(2024, 12, 31), vec![JournalLine::debit(fuel, 99_000), JournalLine::credit(checking, 99_000)]).await.unwrap();
    set_account_tax_line(&pool, sales, Some("SCHEDULE_C:1")).await.unwrap();
    set_account_tax_line(&pool, fuel, Some("SCHEDULE_C:9")).await.unwrap();
    set_account_tax_line(&pool, contractors, Some("SCHEDULE_C:27a")).await.unwrap();
    set_account_tax_line(&pool, utilities, Some("SCHEDULE_C:30")).await.unwrap();
    set_account_tax_line(&pool, meals, Some("1120S:16")).await.unwrap();

    let out = export(&pool, d(2025, 1, 1), d(2025, 12, 31), d(2026, 2, 14)).await.unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        out.text,
        format!(
            "V042\r\nATillhouse {version}\r\nD02/14/2026\r\n^\r\n\
             TS\r\nN293\r\nC1\r\nL1\r\n$950.00\r\n^\r\n\
             TS\r\nN306\r\nC1\r\nL1\r\n$-123.45\r\n^\r\n\
             TS\r\nN302\r\nC1\r\nL1\r\n$-70.00\r\nPContractors\r\n^\r\n"
        ),
        "income positive, expenses negative, other expenses described, last year left out"
    );
    let left: Vec<(&str, i64)> = out.left_out.iter().map(|l| (l.account_name.as_str(), l.amount_cents)).collect();
    assert_eq!(left, vec![("Meals & Entertainment", -2_500), ("Rent & Lease", -30_000), ("Utilities", -4_000)]);
    assert!(out.left_out[0].reason.contains("not a Schedule C line"));
    assert!(out.left_out[1].reason.starts_with("No tax line"));
    assert!(out.left_out[2].reason.contains("no TXF reference number"));

    assert!(export(&pool, d(2025, 2, 1), d(2025, 1, 1), d(2026, 1, 1)).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
}

#[tokio::test]
async fn accounts_on_one_line_are_added_together() {
    let pool = test_pool().await;
    let (checking, fuel, rent) = (account(&pool, "1010").await, account(&pool, "6100").await, account(&pool, "6300").await);
    crate::ledger::post_journal_entry(
        &pool,
        JournalEntry {
            id: None,
            date: d(2025, 5, 1),
            description: "Two".into(),
            lines: vec![JournalLine::debit(fuel, 1_000), JournalLine::debit(rent, 2_000), JournalLine::credit(checking, 3_000)],
            currency: None,
        },
    )
    .await
    .unwrap();
    set_account_tax_line(&pool, fuel, Some("SCHEDULE_C:22")).await.unwrap();
    set_account_tax_line(&pool, rent, Some("SCHEDULE_C:22")).await.unwrap();
    let out = export(&pool, d(2025, 1, 1), d(2025, 12, 31), d(2026, 1, 2)).await.unwrap();
    assert_eq!(out.records, vec![TxfRecord { reference: 301, tax_line: "22 · Supplies".into(), description: None, amount_cents: -3_000 }]);
    assert!(out.left_out.is_empty());
}
