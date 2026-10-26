use super::*;
use crate::db::test_pool;
use crate::ledger::post_journal_entry;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn post(pool: &SqlitePool, lines: Vec<JournalLine>) -> i64 {
    post_journal_entry(pool, JournalEntry { id: None, date: "2026-05-01".parse().unwrap(), description: "original".into(), lines, currency: None }).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

async fn tamper(pool: &SqlitePool, sql: &str) {
    for s in ["PRAGMA foreign_keys = OFF", "DROP TRIGGER IF EXISTS trg_jl_update_draft_only", "DROP TRIGGER IF EXISTS trg_jl_delete_draft_only", "DROP TRIGGER IF EXISTS trg_je_no_delete_posted", "DROP TRIGGER IF EXISTS trg_je_posted_header_immutable"] {
        sqlx::query(s).execute(pool).await.unwrap();
    }
    sqlx::query(sql).execute(pool).await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON").execute(pool).await.unwrap();
}

fn today() -> NaiveDate {
    "2026-10-13".parse().unwrap()
}

#[tokio::test]
async fn reviewed_corrections_restore_balanced_changes_and_refuse_the_rest() {
    let pool = test_pool().await;
    let (checking, sales, services, savings) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "4100").await, account(&pool, "1020").await);
    let moved = post(&pool, vec![JournalLine::debit(checking, 50_000), JournalLine::credit(sales, 50_000)]).await;
    let deleted = post(&pool, vec![JournalLine::debit(savings, 7_000), JournalLine::credit(services, 7_000)]).await;
    let skewed = post(&pool, vec![JournalLine::debit(checking, 3_000), JournalLine::credit(sales, 3_000)]).await;
    let redated = post(&pool, vec![JournalLine::debit(checking, 1_000), JournalLine::credit(sales, 1_000)]).await;
    assert!(propose_corrections(&pool).await.unwrap().is_empty(), "an intact ledger needs no corrections");

    // Outside edits: income moved to another account, an entry deleted, an amount skewed, a date changed.
    tamper(&pool, &format!("UPDATE journal_lines SET account_id = {services} WHERE journal_entry_id = {moved} AND account_id = {sales}")).await;
    tamper(&pool, &format!("DELETE FROM journal_lines WHERE journal_entry_id = {deleted}")).await;
    tamper(&pool, &format!("DELETE FROM journal_entries WHERE id = {deleted}")).await;
    tamper(&pool, &format!("UPDATE journal_lines SET debit_cents = 3500 WHERE journal_entry_id = {skewed} AND debit_cents > 0")).await;
    tamper(&pool, &format!("UPDATE journal_entries SET entry_date = '2026-01-01' WHERE id = {redated}")).await;

    let proposals = propose_corrections(&pool).await.unwrap();
    let find = |id: i64| proposals.iter().find(|p| p.entry_id == id).unwrap().clone();

    let p = find(moved);
    assert_eq!(p.status, CorrectionStatus::Fixable);
    let lines: Vec<(i64, i64, i64)> = p.lines.iter().map(|l| (l.account_id, l.debit_cents, l.credit_cents)).collect();
    assert!(lines.contains(&(sales, 0, 50_000)) && lines.contains(&(services, 50_000, 0)), "{lines:?}");

    let d = find(deleted);
    assert_eq!((d.status, d.entry_deleted, d.entry_number), (CorrectionStatus::Fixable, true, Some(2)));

    let s = find(skewed);
    assert_eq!((s.status, s.imbalance_cents), (CorrectionStatus::OutOfBalance, 500));
    assert!(s.lines.is_empty());
    assert_eq!(find(redated).status, CorrectionStatus::NoBalanceEffect);

    // A correction posts only exactly as reviewed, and only once.
    let stale = ApplyCorrectionInput { entry_id: moved, lines: vec![], date: today() };
    assert!(apply_correction(&pool, &stale).await.unwrap_err().starts_with("HEAL_STALE"));
    let (sales_before, services_before) = (balance(&pool, sales).await, balance(&pool, services).await);
    let applied = apply_correction(&pool, &ApplyCorrectionInput { entry_id: moved, lines: p.lines.clone(), date: today() }).await.unwrap();
    assert_eq!(balance(&pool, sales).await, sales_before - 50_000, "sales gets its income back");
    assert_eq!(balance(&pool, services).await, services_before + 50_000);
    assert!(apply_correction(&pool, &ApplyCorrectionInput { entry_id: moved, lines: p.lines.clone(), date: today() }).await.unwrap_err().starts_with("HEAL_NOT_FIXABLE"));
    let after = propose_corrections(&pool).await.unwrap();
    let corrected = after.iter().find(|p| p.entry_id == moved).unwrap();
    assert_eq!((corrected.status, corrected.corrected_by_entry_number), (CorrectionStatus::Corrected, Some(applied.correction_entry_number)));

    let heal: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'FORENSIC_HEAL' AND target_entity_id = ?1").bind(moved).fetch_one(&pool).await.unwrap();
    let heal: Value = serde_json::from_str(&heal).unwrap();
    assert_eq!(heal["correctionEntryId"], applied.correction_entry_id);

    // Corrections respect the closing date like every other entry, and nothing is recorded.
    crate::closing::set_closing_date(&pool, &crate::closing::ClosingInput { lock_date: Some(today()), current_passcode: None, new_passcode: None }).await.unwrap();
    let err = apply_correction(&pool, &ApplyCorrectionInput { entry_id: deleted, lines: d.lines.clone(), date: today() }).await.unwrap_err();
    assert_eq!(err, crate::closing::CLOSED_MESSAGE);
    let heals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'FORENSIC_HEAL'").fetch_one(&pool).await.unwrap();
    assert_eq!(heals, 1, "the refused correction left no FORENSIC_HEAL event");
    crate::closing::set_closing_date(&pool, &crate::closing::ClosingInput { lock_date: None, current_passcode: None, new_passcode: None }).await.unwrap();

    // The deleted entry's effect is re-recorded; savings returns to its posted amount.
    apply_correction(&pool, &ApplyCorrectionInput { entry_id: deleted, lines: d.lines.clone(), date: today() }).await.unwrap();
    assert_eq!(balance(&pool, savings).await, 7_000);

    // Out-of-balance and date-only changes cannot be "healed" with an entry.
    assert!(apply_correction(&pool, &ApplyCorrectionInput { entry_id: skewed, lines: vec![], date: today() }).await.unwrap_err().starts_with("HEAL_NOT_FIXABLE"));
    assert!(apply_correction(&pool, &ApplyCorrectionInput { entry_id: redated, lines: vec![], date: today() }).await.unwrap_err().starts_with("HEAL_NOT_FIXABLE"));
}

#[tokio::test]
async fn concurrent_corrections_post_only_once() {
    let pool = test_pool().await;
    let (checking, sales, services) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "4100").await);
    let id = post(&pool, vec![JournalLine::debit(checking, 100), JournalLine::credit(sales, 100)]).await;
    tamper(&pool, &format!("UPDATE journal_lines SET account_id = {services} WHERE journal_entry_id = {id} AND account_id = {sales}")).await;
    let proposal = propose_corrections(&pool).await.unwrap().remove(0);
    let input = ApplyCorrectionInput { entry_id: id, lines: proposal.lines, date: today() };
    let (first, second) = tokio::join!(apply_correction(&pool, &input), apply_correction(&pool, &input));
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1, "{first:?}, {second:?}");
    let error = first.err().or_else(|| second.err()).unwrap();
    assert!(error.starts_with("HEAL_NOT_FIXABLE"), "{error}");
    assert_eq!(balance(&pool, sales).await, -100);
    assert_eq!(balance(&pool, services).await, 0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'FORENSIC_HEAL'").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn voided_correction_can_be_reviewed_and_reapplied() {
    let pool = test_pool().await;
    let (checking, sales, services) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "4100").await);
    let id = post(&pool, vec![JournalLine::debit(checking, 100), JournalLine::credit(sales, 100)]).await;
    tamper(&pool, &format!("UPDATE journal_lines SET account_id = {services} WHERE journal_entry_id = {id} AND account_id = {sales}")).await;
    let proposal = propose_corrections(&pool).await.unwrap().remove(0);
    let input = ApplyCorrectionInput { entry_id: id, lines: proposal.lines, date: today() };
    let first = apply_correction(&pool, &input).await.unwrap();
    ledger::void_transaction(&pool, first.correction_entry_id).await.unwrap();
    assert_eq!(balance(&pool, sales).await, 0);
    let proposal = propose_corrections(&pool).await.unwrap().remove(0);
    assert_eq!(proposal.status, CorrectionStatus::Fixable);
    assert_eq!(proposal.lines, input.lines);
    let second = apply_correction(&pool, &input).await.unwrap();
    assert_ne!(first.correction_entry_id, second.correction_entry_id);
    assert_eq!(balance(&pool, sales).await, -100);
    assert_eq!(balance(&pool, services).await, 0);
    let proposal = propose_corrections(&pool).await.unwrap().remove(0);
    assert_eq!(proposal.status, CorrectionStatus::Corrected);
    assert_eq!(proposal.corrected_by_entry_number, Some(second.correction_entry_number));
}

#[tokio::test]
async fn rebuilding_the_audit_table_kept_every_row_and_trigger() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let sales = account(&pool, "4000").await;
    let id = post(&pool, vec![JournalLine::debit(checking, 100), JournalLine::credit(sales, 100)]).await;
    ledger::void_transaction(&pool, id).await.unwrap();
    let kinds: Vec<String> = sqlx::query_scalar("SELECT DISTINCT action_type FROM ledger_audit_log ORDER BY 1").fetch_all(&pool).await.unwrap();
    assert_eq!(kinds, ["MIGRATION", "POST", "VOID"]);
    assert!(sqlx::query("DELETE FROM ledger_audit_log").execute(&pool).await.map_err(db_err).unwrap_err().starts_with("AUDIT_IMMUTABLE"));
    assert!(ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
