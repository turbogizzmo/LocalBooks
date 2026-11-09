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
