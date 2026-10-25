use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

#[test]
fn plain_amounts_are_spreadsheet_numbers() {
    assert_eq!(plain_amount(123_456), "1234.56");
    assert_eq!(plain_amount(-5), "-0.05");
    assert_eq!(plain_amount(0), "0.00");
}

#[tokio::test]
async fn exports_statements_as_quoted_indented_csv() {
    let pool = test_pool().await;
    let id = |n: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap()
        }
    };
    let (checking, services) = (id("1010").await, id("4100").await);
    let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines: vec![JournalLine::debit(checking, 123_456), JournalLine::credit(services, 123_456)], currency: None })
        .await
        .unwrap();
    crate::preferences::set(&pool, "company_name", "Acme, \"Widgets\" & Co").await.unwrap();

    let csv = render(&pool, "profit_and_loss", Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()), Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()), None, None).await.unwrap();
    assert!(csv.starts_with('\u{feff}'));
    let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(csv.trim_start_matches('\u{feff}').as_bytes());
    let rows: Vec<Vec<String>> = reader.records().map(|r| r.unwrap().iter().map(String::from).collect()).collect();
    assert_eq!(rows[0], vec!["Acme, \"Widgets\" & Co"], "commas and quotes survive a round trip");
    assert_eq!(rows[2], vec!["January 1, 2026 - December 31, 2026"]);
    assert!(rows.contains(&vec!["  4100 Services".to_string(), "Account".to_string(), "1234.56".to_string()]));
    assert_eq!(rows.last().unwrap(), &vec!["Net Income".to_string(), "Net".to_string(), "1234.56".to_string()]);

    let by_class = render(&pool, "profit_and_loss_by_class", Some(date), Some(date), None, None).await.unwrap();
    assert!(by_class.contains("Account,Line type,Unclassified,Total"), "{by_class}");
    assert!(by_class.contains("Net Income,Net,1234.56,1234.56"), "{by_class}");

    let bs = render(&pool, "balance_sheet", None, None, Some(date), None).await.unwrap();
    assert!(bs.contains("TOTAL ASSETS,Net,1234.56"));
    assert!(render(&pool, "cash_flow", None, None, Some(date), None).await.unwrap_err().starts_with("EXPORT_UNKNOWN_REPORT"));

    let dir = std::env::temp_dir().join(format!("localbooks-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("pl.csv");
    write_atomically(&target, csv.as_bytes()).unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), csv);
    std::fs::remove_dir_all(&dir).unwrap();
}
