use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn new_account(pool: &SqlitePool, number: &str, name: &str, account_type: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type) VALUES (?1, ?2, ?3) RETURNING id")
        .bind(number)
        .bind(name)
        .bind(account_type)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Two payrolls in a service's journal layout: debits and credits, a payroll id, a totals row.
const EXPORT: &str = "Payroll Journal Report\n\
Payroll ID,Check Date,GL Account,Description,Debit,Credit\n\
PR-101,01/15/2026,Wages Expense,Gross wages,\"4,000.00\",\n\
PR-101,01/15/2026,Payroll Tax Expense,Employer taxes,306.00,\n\
PR-101,01/15/2026,Payroll Liabilities,Withholding and employer taxes,,918.00\n\
PR-101,01/15/2026,Checking,Net pay,,\"3,388.00\"\n\
PR-102,01/31/2026,Wages Expense,Gross wages,\"4,000.00\",\n\
PR-102,01/31/2026,Payroll Tax Expense,Employer taxes,306.00,\n\
PR-102,01/31/2026,Payroll Liabilities,Withholding and employer taxes,,918.00\n\
PR-102,01/31/2026,Checking,Net pay,,\"3,388.00\"\n\
,,Total,,\"8,612.00\",\"8,612.00\"\n";

#[test]
fn headings_suggest_the_columns() {
    let headers: Vec<String> = ["Payroll ID", "Check Date", "GL Account", "Description", "Debit", "Credit"].iter().map(|s| s.to_string()).collect();
    let m = suggest(&headers);
    assert_eq!((m["entry"], m["date"], m["account"], m["memo"], m["debit"], m["credit"]), (0, 1, 2, 3, 4, 5));
    assert!(!m.contains_key("amount"));
}

#[tokio::test]
async fn an_export_posts_balanced_entries_once_its_accounts_are_matched() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let wages = new_account(&pool, "6871", "Wages Expense", "EXPENSE").await;
    let taxes = new_account(&pool, "6872", "Payroll Taxes", "EXPENSE").await;
    let liabilities = new_account(&pool, "2871", "Payroll Liabilities", "OTHER_CURRENT_LIABILITY").await;

    let first = preview(&pool, "january.csv", EXPORT.as_bytes(), &PayrollChoices::default()).await.unwrap();
    assert_eq!(first.entries.len(), 2, "the totals row is skipped");
    assert_eq!(first.total_cents, 861_200);
    let how: Vec<(&str, Option<i64>, &str)> = first.source_accounts.iter().map(|s| (s.name.as_str(), s.account_id, s.how)).collect();
    assert_eq!(how, vec![("Checking", Some(checking), "matched"), ("Payroll Liabilities", Some(liabilities), "matched"), ("Payroll Tax Expense", None, "none"), ("Wages Expense", Some(wages), "matched")]);
    assert_eq!(first.problems.len(), 1, "{:?}", first.problems);
    assert!(post(&pool, "january.csv", EXPORT.as_bytes(), &PayrollChoices::default()).await.unwrap_err().starts_with("PAYROLL_PROBLEMS"));

    let choices = PayrollChoices { accounts: HashMap::from([("Payroll Tax Expense".to_string(), taxes)]), ..Default::default() };
    let ready = preview(&pool, "january.csv", EXPORT.as_bytes(), &choices).await.unwrap();
    assert!(ready.problems.is_empty(), "{:?}", ready.problems);
    let e = &ready.entries[0];
    assert_eq!((e.key.as_str(), e.date, e.description.as_str(), e.debit_cents, e.credit_cents), ("PR-101", NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(), "Payroll PR-101 (2026-01-15)", 430_600, 430_600));

    let posted = post(&pool, "january.csv", EXPORT.as_bytes(), &choices).await.unwrap();
    assert_eq!(posted.journal_entry_ids.len(), 2);
    let balance = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM journal_lines WHERE account_id = ?1").bind(id).fetch_one(&pool).await.unwrap()
        }
    };
    assert_eq!(balance(wages).await, 800_000);
    assert_eq!(balance(taxes).await, 61_200);
    assert_eq!(balance(liabilities).await, -183_600);
    assert_eq!(balance(checking).await, -677_600);
    let memo: String = sqlx::query_scalar("SELECT memo FROM journal_lines WHERE account_id = ?1 LIMIT 1").bind(wages).fetch_one(&pool).await.unwrap();
    assert_eq!(memo, "Gross wages");

    // The same file again is refused; the account it was told about is remembered.
    let again = preview(&pool, "copy.csv", EXPORT.as_bytes(), &PayrollChoices::default()).await.unwrap();
    assert!(again.already_imported.is_some());
    assert!(again.source_accounts.iter().any(|s| s.name == "Payroll Tax Expense" && s.account_id == Some(taxes) && s.how == "saved"));
    assert!(post(&pool, "copy.csv", EXPORT.as_bytes(), &PayrollChoices::default()).await.unwrap_err().starts_with("PAYROLL_ALREADY_IMPORTED"));

    // Voiding every entry lets it be imported again.
    for id in &posted.journal_entry_ids {
        crate::ledger::void_transaction(&pool, *id).await.unwrap();
    }
    let h = history(&pool).await.unwrap();
    assert_eq!((h.len(), h[0].entry_count, h[0].voided, h[0].total_cents), (1, 2, 2, 861_200));
    assert!(preview(&pool, "copy.csv", EXPORT.as_bytes(), &PayrollChoices::default()).await.unwrap().already_imported.is_none());
}

#[tokio::test]
async fn an_entry_out_of_balance_stops_the_import() {
    let pool = test_pool().await;
    new_account(&pool, "6871", "Wages", "EXPENSE").await;
    let file = "Date,Account,Amount,Memo\n2026-02-13,Wages,1000.00,Gross\n2026-02-13,Checking,-990.00,Net\n2026-02-27,Wages,500,\n2026-02-27,Checking,-500,\n";
    let p = preview(&pool, "feb.csv", file.as_bytes(), &PayrollChoices::default()).await.unwrap();
    assert_eq!(p.entries.len(), 2, "entries go by date without an Entry column");
    assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
    assert_eq!(p.problems[0].row, 2);
    assert!(p.problems[0].message.contains("out of balance") && p.problems[0].message.contains("10.00"), "{}", p.problems[0].message);
    assert!(post(&pool, "feb.csv", file.as_bytes(), &PayrollChoices::default()).await.unwrap_err().starts_with("PAYROLL_PROBLEMS"));
    let posted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(posted, 0, "nothing posts");
}

#[tokio::test]
async fn missing_columns_and_bad_rows_are_listed() {
    let pool = test_pool().await;
    let p = preview(&pool, "x.csv", b"When,What\n1/1/2026,Wages\n", &PayrollChoices::default()).await.unwrap();
    let messages: Vec<&str> = p.problems.iter().map(|x| x.message.as_str()).collect();
    assert_eq!(messages, vec!["Match a column to Date", "Match a column to Account", "Match a column to Debit and Credit, or to Amount"]);

    let file = "Date,Account,Debit,Credit\nnot a date,Checking,1.00,\n01/02/2026,,1.00,\n01/02/2026,Checking,abc,\n";
    let p = preview(&pool, "y.csv", file.as_bytes(), &PayrollChoices::default()).await.unwrap();
    let rows: Vec<usize> = p.problems.iter().map(|x| x.row).collect();
    assert_eq!(rows, vec![2, 3, 4], "{:?}", p.problems);
}

#[tokio::test]
async fn a_closed_period_refuses_the_whole_import() {
    let pool = test_pool().await;
    new_account(&pool, "6871", "Wages", "EXPENSE").await;
    let lock = crate::closing::ClosingInput { lock_date: NaiveDate::from_ymd_opt(2026, 1, 31), current_passcode: None, new_passcode: None };
    crate::closing::set_closing_date(&pool, &lock).await.unwrap();
    let file = "Date,Account,Amount\n2026-03-13,Wages,100\n2026-03-13,Checking,-100\n2026-01-15,Wages,100\n2026-01-15,Checking,-100\n";
    assert!(post(&pool, "c.csv", file.as_bytes(), &PayrollChoices::default()).await.is_err());
    let posted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(posted, 0, "the March entry is rolled back with the January one");
}
