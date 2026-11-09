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

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn post(pool: &SqlitePool, date: NaiveDate, debit: i64, credit: i64, cents: i64) -> i64 {
    post_journal_entry(
        pool,
        JournalEntry {
            id: None,
            date,
            description: String::new(),
            lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)],
            currency: None,
        },
    )
    .await
    .expect("posts")
}

#[tokio::test]
async fn reconcile_balances_locks_and_freezes() {
    let pool = test_pool().await;
    let (checking, owner, rent) = (
        account_id(&pool, "1010").await,
        account_id(&pool, "3100").await,
        account_id(&pool, "6300").await,
    );
    let deposit = post(&pool, d(9, 2), checking, owner, 500_000).await;
    post(&pool, d(9, 10), rent, checking, 120_000).await;
    post(&pool, d(9, 28), rent, checking, 5_000).await; // not yet on the statement
    post(&pool, d(10, 3), rent, checking, 7_500).await; // after statement date

    let rec = start(&pool, checking, d(9, 30), 380_000).await.expect("starts");
    assert_eq!(rec.beginning_balance_cents, 0);

    let lines = unreconciled_lines(&pool, checking).await.unwrap();
    assert_eq!(lines.iter().map(|l| l.amount_cents).collect::<Vec<_>>(), vec![500_000, -120_000, -5_000, -7_500]);

    let progress = set_lines_cleared(&pool, rec.id, &[lines[0].line_id, lines[1].line_id], true).await.unwrap();
    assert_eq!(progress, ReconcileProgress { cleared_balance_cents: 380_000, difference_cents: 0 });

    let err = set_lines_cleared(&pool, rec.id, &[lines[3].line_id], true).await.expect_err("after statement");
    assert!(err.starts_with("REC_AFTER_STATEMENT"), "{err}");

    // Ticking the $50 line puts it out of balance, so locking must fail.
    set_lines_cleared(&pool, rec.id, &[lines[2].line_id], true).await.unwrap();
    let err = lock(&pool, rec.id).await.expect_err("out of balance");
    assert!(err.starts_with("REC_NOT_BALANCED: difference is $50.00"), "{err}");
    let progress = set_lines_cleared(&pool, rec.id, &[lines[2].line_id], false).await.unwrap();
    assert_eq!(progress.difference_cents, 0);

    let locked = lock(&pool, rec.id).await.expect("locks");
    assert!(locked.is_locked);

    let remaining = unreconciled_lines(&pool, checking).await.unwrap();
    assert_eq!(remaining.len(), 2);
    let state = account_state(&pool, checking).await.unwrap();
    assert_eq!(state.reconciled_balance_cents, 380_000);
    assert_eq!(state.last_statement_end_date.as_deref(), Some("2026-09-30"));
    assert!(state.in_progress.is_none());

    // Frozen: reconciled lines cannot be unticked, their entries cannot be voided,
    // and a new statement must end after the last one.
    let err = set_lines_cleared(&pool, rec.id, &[lines[0].line_id], false).await.expect_err("frozen");
    assert!(err.starts_with("REC_LOCKED") || err.starts_with("REC_LINE_UNAVAILABLE"), "{err}");
    let void = sqlx::query("UPDATE journal_entries SET status = 'VOID', voided_at = 'now' WHERE id = ?1")
        .bind(deposit)
        .execute(&pool)
        .await
        .expect_err("void blocked");
    assert!(void.to_string().contains("JE_INVALID_TRANSITION") || void.to_string().contains("REC_LOCKED"), "{void}");
    let err = start(&pool, checking, d(9, 15), 0).await.expect_err("date must advance");
    assert!(err.starts_with("REC_DATE_NOT_AFTER_LAST"), "{err}");

    let next = start(&pool, checking, d(10, 31), 367_500).await.expect("next statement");
    assert_eq!(next.beginning_balance_cents, 380_000);
    let ids: Vec<i64> = remaining.iter().map(|l| l.line_id).collect();
    let progress = set_lines_cleared(&pool, next.id, &ids, true).await.unwrap();
    assert_eq!(progress.difference_cents, 0);

    cancel(&pool, next.id).await.expect("cancels");
    assert!(unreconciled_lines(&pool, checking).await.unwrap().iter().all(|l| l.reconciliation_id.is_none()));
}

#[tokio::test]
async fn credit_card_balances_on_the_credit_side() {
    let pool = test_pool().await;
    let (card, fuel, checking) = (
        account_id(&pool, "2100").await,
        account_id(&pool, "6100").await,
        account_id(&pool, "1010").await,
    );
    post(&pool, d(9, 5), fuel, card, 6_000).await; // charge raises the amount owed
    post(&pool, d(9, 20), card, checking, 2_500).await; // payment lowers it

    let rec = start(&pool, card, d(9, 30), 3_500).await.unwrap();
    let ids: Vec<i64> = unreconciled_lines(&pool, card).await.unwrap().iter().map(|l| l.line_id).collect();
    let progress = set_lines_cleared(&pool, rec.id, &ids, true).await.unwrap();
    assert_eq!(progress, ReconcileProgress { cleared_balance_cents: 3_500, difference_cents: 0 });
    lock(&pool, rec.id).await.expect("locks");
}

#[tokio::test]
async fn rejects_accounts_that_cannot_be_reconciled() {
    let pool = test_pool().await;
    let ar = account_id(&pool, "1200").await;
    let err = start(&pool, ar, d(9, 30), 0).await.expect_err("A/R is not reconcilable");
    assert!(err.starts_with("REC_ACCOUNT_TYPE"), "{err}");
}

#[tokio::test]
async fn service_charge_and_interest_post_with_the_reconciliation_and_balance() {
    let pool = test_pool().await;
    let (checking, owner, rent, sales) = (
        account_id(&pool, "1010").await,
        account_id(&pool, "3100").await,
        account_id(&pool, "6300").await,
        account_id(&pool, "4000").await,
    );
    post(&pool, d(9, 2), checking, owner, 500_000).await;
    // The statement: 5,000.00 in, a 12.50 fee and 1.25 interest, so it ends at 4,988.75.
    let charges = [
        ReconcileCharge { kind: ChargeKind::ServiceCharge, amount_cents: 1_250, date: d(9, 30), account_id: rent, class_id: None },
        ReconcileCharge { kind: ChargeKind::InterestEarned, amount_cents: 125, date: d(9, 30), account_id: sales, class_id: None },
    ];
    let rec = start_with_charges(&pool, checking, d(9, 30), 498_875, &charges).await.expect("starts with charges");

    // Both posted through the ledger, their bank lines already cleared into this reconciliation.
    let lines = unreconciled_lines(&pool, checking).await.unwrap();
    let cleared: Vec<i64> = lines.iter().filter(|l| l.reconciliation_id == Some(rec.id)).map(|l| l.amount_cents).collect();
    assert_eq!(cleared, vec![-1_250, 125]);
    assert!(lines.iter().filter(|l| l.amount_cents != 500_000).all(|l| l.memo.as_deref().is_some_and(|m| m == "Service charge" || m == "Interest earned")));
    let fee_on_rent: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE account_id = ?1").bind(rent).fetch_one(&pool).await.unwrap();
    assert_eq!(fee_on_rent, 1_250);

    // Tick the deposit and it balances.
    let deposit = lines.iter().find(|l| l.amount_cents == 500_000).unwrap().line_id;
    let progress = set_lines_cleared(&pool, rec.id, &[deposit], true).await.unwrap();
    assert_eq!(progress.difference_cents, 0);
    lock(&pool, rec.id).await.expect("locks");
}

#[tokio::test]
async fn a_bad_charge_posts_nothing() {
    let pool = test_pool().await;
    let (checking, rent) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await);
    let entries = || async { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap() };
    let before = entries().await;
    let late = ReconcileCharge { kind: ChargeKind::ServiceCharge, amount_cents: 500, date: d(10, 1), account_id: rent, class_id: None };
    let err = start_with_charges(&pool, checking, d(9, 30), 0, &[late]).await.expect_err("after the statement");
    assert!(err.starts_with("REC_CHARGE_DATE"), "{err}");
    let zero = ReconcileCharge { kind: ChargeKind::InterestEarned, amount_cents: 0, date: d(9, 30), account_id: rent, class_id: None };
    assert!(start_with_charges(&pool, checking, d(9, 30), 0, &[zero]).await.expect_err("zero").starts_with("REC_CHARGE_AMOUNT"));
    let same = ReconcileCharge { kind: ChargeKind::ServiceCharge, amount_cents: 500, date: d(9, 30), account_id: checking, class_id: None };
    assert!(start_with_charges(&pool, checking, d(9, 30), 0, &[same]).await.expect_err("same account").starts_with("REC_CHARGE_ACCOUNT"));
    // Nothing posted and no reconciliation was left started.
    assert_eq!(entries().await, before);
    assert!(account_state(&pool, checking).await.unwrap().in_progress.is_none());
}

async fn reconcile_through(pool: &SqlitePool, account: i64, end: NaiveDate, ending: i64) -> Reconciliation {
    let rec = start(pool, account, end, ending).await.expect("starts");
    let ids: Vec<i64> = unreconciled_lines(pool, account).await.unwrap().into_iter().filter(|l| l.entry_date <= end.to_string()).map(|l| l.line_id).collect();
    set_lines_cleared(pool, rec.id, &ids, true).await.unwrap();
    lock(pool, rec.id).await.expect("locks")
}

#[tokio::test]
async fn undo_last_reconciliation_releases_its_lines_with_an_audit_row() {
    let pool = test_pool().await;
    let (checking, owner, rent) = (account_id(&pool, "1010").await, account_id(&pool, "3100").await, account_id(&pool, "6300").await);
    post(&pool, d(8, 2), checking, owner, 100_000).await;
    let august = reconcile_through(&pool, checking, d(8, 31), 100_000).await;
    let fee = post(&pool, d(9, 10), rent, checking, 2_000).await;
    let september = reconcile_through(&pool, checking, d(9, 30), 98_000).await;
    let report_before = crate::accountant_reports::previous_reconciliation(&pool, august.id, true).await.unwrap();

    assert!(undo_last(&pool, 9_999, None).await.unwrap_err().starts_with("REC_NOT_FOUND"));
    let direct = sqlx::query("INSERT INTO reconciliation_undos (reconciliation_id, released_count, released_total_cents) VALUES (9999, 0, 0)").execute(&pool).await.unwrap_err();
    assert!(direct.to_string().contains("REC_NOT_FOUND"), "{direct}");
    // Only the last one can be undone.
    let err = undo_last(&pool, august.id, None).await.expect_err("not the last");
    assert!(err.starts_with("REC_UNDO_NOT_LAST"), "{err}");

    assert_eq!(undo_last(&pool, september.id, Some("wrong statement")).await.unwrap(), 1);
    // Its line is uncleared and its entry can be voided again; the locked row itself is unchanged.
    let lines = unreconciled_lines(&pool, checking).await.unwrap();
    assert_eq!(lines.iter().map(|l| (l.journal_entry_id, l.reconciliation_id)).collect::<Vec<_>>(), vec![(fee, None)]);
    let still_locked: bool = sqlx::query_scalar("SELECT is_locked FROM reconciliations WHERE id = ?1").bind(september.id).fetch_one(&pool).await.unwrap();
    assert!(still_locked);
    let state = account_state(&pool, checking).await.unwrap();
    assert_eq!((state.reconciled_balance_cents, state.last_statement_end_date.as_deref()), (100_000, Some("2026-08-31")));
    assert_eq!(list_reconciliations(&pool, checking).await.unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), vec![august.id]);

    // Audited in the Audit Log and sealed on the ledger audit trail.
    let detail: String = sqlx::query_scalar("SELECT detail FROM audit_log WHERE action = 'UNDO_RECONCILIATION' AND reconciliation_id = ?1")
        .bind(september.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(detail.contains("through 2026-09-30") && detail.contains("1 transaction uncleared") && detail.ends_with(": wrong statement"), "{detail}");
    let sealed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'EDIT' AND json_extract(payload_snapshot, '$.kind') = 'UNDO_RECONCILIATION'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sealed, 1);
    assert!(crate::audit_chain::verify(&pool).await.unwrap().problems.is_empty());

    // The undo is final, and the earlier reconciliation's report still ties out as before.
    assert!(undo_last(&pool, september.id, None).await.is_err());
    assert!(sqlx::query("DELETE FROM reconciliation_undos").execute(&pool).await.is_err());
    let report_after = crate::accountant_reports::previous_reconciliation(&pool, august.id, true).await.unwrap();
    assert_eq!(serde_json::to_value(&report_after).unwrap(), serde_json::to_value(&report_before).unwrap());

    // The same statement can be reconciled again.
    let again = reconcile_through(&pool, checking, d(9, 30), 98_000).await;
    assert!(again.is_locked);
}

#[tokio::test]
async fn undo_waits_for_a_reconciliation_in_progress() {
    let pool = test_pool().await;
    let (checking, owner) = (account_id(&pool, "1010").await, account_id(&pool, "3100").await);
    post(&pool, d(8, 2), checking, owner, 100_000).await;
    let august = reconcile_through(&pool, checking, d(8, 31), 100_000).await;
    let open = start(&pool, checking, d(9, 30), 100_000).await.unwrap();
    let err = undo_last(&pool, august.id, None).await.expect_err("one in progress");
    assert!(err.starts_with("REC_UNDO_IN_PROGRESS"), "{err}");
    cancel(&pool, open.id).await.unwrap();
    assert_eq!(undo_last(&pool, august.id, None).await.unwrap(), 1);
    // A line of a reconciliation that wasn't undone still can't be released.
    let other = reconcile_through(&pool, checking, d(8, 31), 100_000).await;
    let line: i64 = sqlx::query_scalar("SELECT id FROM journal_lines WHERE reconciliation_id = ?1").bind(other.id).fetch_one(&pool).await.unwrap();
    let err = sqlx::query("UPDATE journal_lines SET reconciliation_id = NULL, reconciled_at = NULL WHERE id = ?1").bind(line).execute(&pool).await.expect_err("locked");
    assert!(err.to_string().contains("REC_LOCKED"), "{err}");
}

#[tokio::test]
async fn a_statement_file_attaches_to_the_reconciliation() {
    let pool = test_pool().await;
    let (checking, owner) = (account_id(&pool, "1010").await, account_id(&pool, "3100").await);
    post(&pool, d(8, 2), checking, owner, 100_000).await;
    let open = start(&pool, checking, d(8, 31), 100_000).await.unwrap();
    let file = attach_statement(&pool, open.id, "/tmp/statements/August 2026.pdf", b"%PDF-1.7 synthetic").await.unwrap();
    assert_eq!((file.file_name.as_str(), file.media_type.as_str(), file.size_bytes), ("August 2026.pdf", "application/pdf", 18));
    assert_eq!(statement_content(&pool, file.id).await.unwrap(), ("August 2026.pdf".into(), b"%PDF-1.7 synthetic".to_vec()));
    assert!(attach_statement(&pool, open.id, "empty.pdf", b"").await.unwrap_err().starts_with("ATTACH_EMPTY"));

    // It stays with the reconciliation once finished.
    let ids: Vec<i64> = unreconciled_lines(&pool, checking).await.unwrap().into_iter().map(|l| l.line_id).collect();
    set_lines_cleared(&pool, open.id, &ids, true).await.unwrap();
    lock(&pool, open.id).await.unwrap();
    assert_eq!(statement_files(&pool, open.id).await.unwrap().len(), 1);
    remove_statement(&pool, file.id).await.unwrap();
    assert!(statement_files(&pool, open.id).await.unwrap().is_empty());

    // Cancelling one in progress takes its files with it; an undone one takes no new ones.
    let next = start(&pool, checking, d(9, 30), 100_000).await.unwrap();
    attach_statement(&pool, next.id, "september.png", b"png").await.unwrap();
    cancel(&pool, next.id).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reconciliation_statements").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0);
    undo_last(&pool, open.id, None).await.unwrap();
    assert!(attach_statement(&pool, open.id, "late.pdf", b"x").await.unwrap_err().starts_with("REC_UNDONE"));
}
