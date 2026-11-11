use super::*;
use crate::bank::suggest::Suggester;
use crate::bank::{approve_many, list_pending, Approval};
use crate::db::test_pool;
use crate::expenses::{ExpenseInput, SplitLineInput};
use crate::money::Cents;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

/// Downloads one line, as an import does; returns its id and whether it matched.
async fn download(pool: &SqlitePool, bank: i64, date: &str, description: &str, amount: i64) -> (i64, bool) {
    let mut tx = pool.begin().await.unwrap();
    let matched = crate::bank::insert_line(&mut tx, bank, day(date), description, amount, &format!("{date}|{description}|{amount}")).await.unwrap().unwrap();
    tx.commit().await.unwrap();
    let id = sqlx::query_scalar("SELECT MAX(id) FROM bank_transactions").fetch_one(pool).await.unwrap();
    (id, matched)
}

async fn check(pool: &SqlitePool, number: Option<&str>, date: &str, cents: i64) -> i64 {
    let (checking, rent) = (account(pool, "1010").await, account(pool, "6300").await);
    let input = ExpenseInput { form_lines: Vec::new(),
        payment_method_id: None,
        kind: "CHECK".into(),
        payee_name: Some("Landlord".into()),
        payment_account_id: checking,
        expense_date: day(date),
        check_number: number.map(Into::into),
        payment_method: None,
        memo: None,
        lines: vec![SplitLineInput { account_id: rent, amount_cents: cents, memo: None, class_id: None, customer_id: None, billable: None }],
        currency: None,
    };
    crate::expenses::post_expense(pool, input).await.unwrap().journal_entry_id.unwrap()
}

async fn pending(pool: &SqlitePool) -> Vec<crate::bank::BankTransactionView> {
    list_pending(pool, &Suggester::default(), "test").await.unwrap()
}

#[test]
fn reads_check_numbers_from_bank_descriptions() {
    assert_eq!(check_number("CHECK 1042").as_deref(), Some("1042"));
    assert_eq!(check_number("Chk #001042 cleared").as_deref(), Some("1042"));
    assert_eq!(check_number("CHECK NO. 77").as_deref(), Some("77"));
    assert_eq!(check_number("CHECKCARD 0412 COFFEE"), None, "a card purchase is not a check");
    assert_eq!(check_number("POS PURCHASE 1042"), None);
}

#[tokio::test]
async fn a_downloaded_check_is_matched_to_the_check_already_written() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let written = check(&pool, Some("1042"), "2026-03-01", 120_000).await;
    check(&pool, Some("1043"), "2026-03-02", 120_000).await;

    // Two checks for 1,200.00: the number says which; the other line, with no check entered, waits.
    let (line, matched) = download(&pool, checking, "2026-03-06", "CHECK 1042", -120_000).await;
    assert!(matched);
    let (other, matched) = download(&pool, checking, "2026-03-06", "COFFEE SHOP", -450).await;
    assert!(!matched);
    assert_eq!(pending(&pool).await.iter().map(|t| t.id).collect::<Vec<_>>(), [other], "the matched line posts nothing and leaves the queue");
    let matched = list_matched(&pool).await.unwrap();
    assert_eq!((matched[0].id, matched[0].matched.journal_entry_id, matched[0].matched.num.as_deref(), matched[0].origin.as_str()), (line, written, Some("1042"), "AUTO"));
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(entries, 2, "nothing new posted");

    // Unmatch puts it back; matching by hand works; voiding the check puts it back again.
    unmatch(&pool, line).await.unwrap();
    assert_eq!(pending(&pool).await.len(), 2);
    match_line(&pool, line, written).await.unwrap();
    assert!(match_line(&pool, other, written).await.unwrap_err().starts_with("BANK_MATCH"), "a check is matched once");
    crate::ledger::void_transaction(&pool, written).await.unwrap();
    assert!(list_matched(&pool).await.unwrap().is_empty());
    assert_eq!(pending(&pool).await.len(), 2);
}

#[tokio::test]
async fn two_that_agree_are_offered_not_matched_and_lines_entered_later_are_suggested() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let first = check(&pool, None, "2026-03-01", 5_000).await;
    let second = check(&pool, None, "2026-03-03", 5_000).await;
    let (line, matched) = download(&pool, checking, "2026-03-04", "ACME SUPPLY", -5_000).await;
    assert!(!matched, "two transactions agree, so the import leaves it to you");
    assert_eq!(pending(&pool).await[0].suggested_match, None);
    let offered = candidates(&pool, line).await.unwrap();
    assert_eq!(offered.iter().map(|c| c.journal_entry_id).collect::<Vec<_>>(), [second, first], "nearest date first");

    // A line downloaded before its transaction was entered is suggested once it is.
    let (later, _) = download(&pool, checking, "2026-03-10", "TRANSFER IN", 25_000).await;
    assert_eq!(pending(&pool).await.iter().find(|t| t.id == later).unwrap().suggested_match, None);
    let savings: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Reserve Savings', 'BANK') RETURNING id").fetch_one(&pool).await.unwrap();
    crate::banking::post_transfer(&pool, crate::banking::TransferInput { from_account_id: savings, to_account_id: checking, transfer_date: day("2026-03-09"), amount_cents: Cents(25_000), memo: None })
        .await
        .unwrap();
    let suggested = pending(&pool).await.into_iter().find(|t| t.id == later).unwrap().suggested_match.unwrap();
    assert_eq!(suggested.source_type, "TRANSFER");
}

#[tokio::test]
async fn add_several_at_once_posts_them_all_or_none() {
    let pool = test_pool().await;
    let (checking, supplies) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let (a, _) = download(&pool, checking, "2026-03-04", "PAPER", -1_000).await;
    let (b, _) = download(&pool, checking, "2026-03-05", "TONER", -2_000).await;
    let approval = || Approval { category_account_id: supplies, class_id: None, payee_name: None };
    let err = approve_many(&pool, &Suggester::default(), "test", vec![(a, approval()), (b, Approval { category_account_id: checking, ..approval() })]).await.unwrap_err();
    assert!(err.starts_with("BANK_SAME_ACCOUNT"), "{err}");
    assert_eq!(pending(&pool).await.len(), 2, "none posted");
    let entries = approve_many(&pool, &Suggester::default(), "test", vec![(a, approval()), (b, approval())]).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert!(pending(&pool).await.is_empty());
}
