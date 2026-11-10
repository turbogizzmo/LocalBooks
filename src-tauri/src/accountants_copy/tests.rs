use std::path::PathBuf;

use super::*;
use crate::db::{self, Db};
use crate::ledger::post_journal_entry;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-acopy-{tag}-{}-{}", std::process::id(), crate::mcp_server::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn entry(date: NaiveDate, memo: &str, lines: Vec<JournalLine>) -> JournalEntry {
    JournalEntry { id: None, date, description: memo.into(), lines, currency: None }
}

async fn balance(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn the_accountants_changes_come_back_with_an_audit_record() {
    let tmp = TempDir::new("flow");
    let live = tmp.0.join("live.db");
    let client = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    let pool = client.pool().await;
    let (checking, sales, office) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6250").await);
    post_journal_entry(&pool, entry(d(3, 15), "March sales", vec![JournalLine::debit(checking, 50_000), JournalLine::credit(sales, 50_000)])).await.unwrap();

    // The client makes the copy, dividing at March 31.
    let copy_path = tmp.0.join("for-accountant.localbooks");
    let rec = create(&pool, &live, &copy_path, d(3, 31), "Dana").await.unwrap();
    assert_eq!((rec.status.as_str(), rec.dividing_date), ("OUTSTANDING", d(3, 31)));
    assert!(create(&pool, &live, &tmp.0.join("again.localbooks"), d(3, 31), "Dana").await.unwrap_err().starts_with("ACCOUNTANT_COPY_OUT"));
    // The client can't post in the accountant's period, and keeps working after it.
    let early = post_journal_entry(&pool, entry(d(3, 20), "Late March", vec![JournalLine::debit(office, 100), JournalLine::credit(checking, 100)])).await.unwrap_err();
    assert!(early.starts_with("ACCOUNTANT_COPY_LOCKED"), "{early}");
    post_journal_entry(&pool, entry(d(4, 2), "April supplies", vec![JournalLine::debit(office, 2_500), JournalLine::credit(checking, 2_500)])).await.unwrap();

    // The accountant works in the copy: a new account, an adjusting entry, and a voided mistake.
    let copy = db::open(&copy_path).await.unwrap();
    assert_eq!(copy_dividing_date(&copy).await.unwrap(), Some(d(3, 31)));
    let prepaid: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type) VALUES ('1450', 'Prepaid Insurance', 'OTHER_CURRENT_ASSET') RETURNING id").fetch_one(&copy).await.unwrap();
    let adjusting = entry(d(3, 31), "Prepaid insurance", vec![JournalLine::debit(prepaid, 12_000), JournalLine::credit(account(&copy, "6250").await, 12_000)]);
    let mut tx = crate::ledger::begin_write(&copy).await.unwrap();
    crate::ledger::post_entry_in_tx_flagged(&mut tx, &adjusting, EntrySource::Journal, true).await.unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    let mistake = post_journal_entry(&copy, entry(d(3, 30), "Oops", vec![JournalLine::debit(account(&copy, "6250").await, 99), JournalLine::credit(account(&copy, "1010").await, 99)])).await.unwrap();
    crate::ledger::void_transaction(&copy, mistake).await.unwrap();
    let after = post_journal_entry(&copy, entry(d(4, 1), "Too late", vec![JournalLine::debit(account(&copy, "6250").await, 1), JournalLine::credit(account(&copy, "1010").await, 1)])).await.unwrap_err();
    assert!(after.starts_with("ACCOUNTANT_COPY_LIMIT"), "{after}");
    let changes_path = tmp.0.join("changes.json");
    let file = export(&copy, &changes_path).await.unwrap();
    db::close_pool(&copy).await;
    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.entries.len(), 1, "the voided mistake and its reversal stay behind");
    assert_eq!(file.left_out, vec![format!("journal entry \"Oops\" dated {}, voided in the copy (it and its void stay behind)", d(3, 30))]);
    assert!(file.entries[0].adjusting);

    // The client previews and imports.
    let bytes = std::fs::read(&changes_path).unwrap();
    let p = preview(&pool, &bytes).await.unwrap();
    assert_eq!((p.accounts.len(), p.accounts_existing.len(), p.entries.len()), (1, 0, 1));
    let done = import(&pool, &bytes, "Dana").await.unwrap();
    assert_eq!((done.status.as_str(), done.entries_posted, done.accounts_created, done.closed_by.as_deref()), ("RETURNED", 1, 1, Some("Dana")));
    assert_eq!(done.changes_sha256.as_deref().map(str::len), Some(64));
    let new_prepaid = account(&pool, "1450").await;
    assert_eq!(balance(&pool, new_prepaid).await, 12_000);
    let (memo, adj): (String, bool) = sqlx::query_as("SELECT memo, is_adjusting = 1 FROM journal_entries ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!((memo.as_str(), adj), ("Prepaid insurance (from the accountant)", true));
    // The lock is lifted, and the same changes can't come in twice.
    post_journal_entry(&pool, entry(d(3, 20), "Late March", vec![JournalLine::debit(office, 100), JournalLine::credit(checking, 100)])).await.unwrap();
    assert!(import(&pool, &bytes, "Dana").await.unwrap_err().starts_with("ACCOUNTANT_CHANGES_DONE"));
    assert_eq!(copies(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn changes_from_another_or_a_cancelled_copy_are_refused() {
    let tmp = TempDir::new("refuse");
    let live = tmp.0.join("live.db");
    let client = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    let pool = client.pool().await;
    let copy_path = tmp.0.join("copy.localbooks");
    create(&pool, &live, &copy_path, d(6, 30), "Dana").await.unwrap();
    let copy = db::open(&copy_path).await.unwrap();
    let file = changes(&copy).await.unwrap();
    db::close_pool(&copy).await;

    let mut other = file.clone();
    other.token = "0".repeat(32);
    assert!(preview(&pool, serde_json::to_string(&other).unwrap().as_bytes()).await.unwrap_err().starts_with("ACCOUNTANT_CHANGES_OTHER"));
    assert!(preview(&pool, b"{\"not\": \"changes\"}").await.unwrap_err().starts_with("ACCOUNTANT_CHANGES_BAD"));
    let mut late = file.clone();
    late.entries.push(ChangedEntry { date: d(7, 1), memo: "x".into(), adjusting: false, lines: vec![] });
    assert!(import(&pool, serde_json::to_string(&late).unwrap().as_bytes(), "Dana").await.unwrap_err().starts_with("ACCOUNTANT_CHANGES_BAD"));
    assert_eq!(copies(&pool).await.unwrap()[0].status, "OUTSTANDING", "a refused import changes nothing");

    cancel(&pool, "Dana").await.unwrap();
    assert!(cancel(&pool, "Dana").await.unwrap_err().starts_with("ACCOUNTANT_COPY_NONE"));
    assert!(import(&pool, serde_json::to_string(&file).unwrap().as_bytes(), "Dana").await.unwrap_err().starts_with("ACCOUNTANT_CHANGES_CANCELLED"));
    // A copy can't make a copy of its own.
    let copy = db::open(&copy_path).await.unwrap();
    assert!(create(&copy, &copy_path, &tmp.0.join("nested.localbooks"), d(6, 1), "Acc").await.unwrap_err().starts_with("ACCOUNTANT_COPY_IN_COPY"));
    db::close_pool(&copy).await;
}
