use super::*;
use crate::db::test_pool;

struct Fixture {
    pool: SqlitePool,
    checking: i64,
    sales: i64,
}

async fn setup() -> Fixture {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let sales = account_id(&pool, "4000").await;
    Fixture { pool, checking, sales }
}

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 13).expect("valid date")
}

async fn row_counts(pool: &SqlitePool) -> (i64, i64) {
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries")
        .fetch_one(pool)
        .await
        .expect("count entries");
    let lines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_lines")
        .fetch_one(pool)
        .await
        .expect("count lines");
    (entries, lines)
}

#[tokio::test]
async fn posts_balanced_100_dollar_entry() {
    let fx = setup().await;
    let entry = JournalEntry {
        id: None,
        date: date(),
        description: "Cash sale".into(),
        lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
        currency: None,
    };

    post_transaction(&fx.pool, entry).await.expect("balanced entry posts");

    let (status, number, memo, entry_date): (String, i64, Option<String>, String) = sqlx::query_as(
        "SELECT status, entry_number, memo, entry_date FROM journal_entries",
    )
    .fetch_one(&fx.pool)
    .await
    .expect("read header");
    assert_eq!(status, "POSTED");
    assert_eq!(number, 1);
    assert_eq!(memo.as_deref(), Some("Cash sale"));
    assert_eq!(entry_date, "2026-10-13");

    let (debits, credits): (i64, i64) =
        sqlx::query_as("SELECT SUM(debit_cents), SUM(credit_cents) FROM journal_lines")
            .fetch_one(&fx.pool)
            .await
            .expect("sum lines");
    assert_eq!((debits, credits), (10_000, 10_000));

    let checking_balance: i64 =
        sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(fx.checking)
            .fetch_one(&fx.pool)
            .await
            .expect("checking balance");
    let sales_balance: i64 =
        sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(fx.sales)
            .fetch_one(&fx.pool)
            .await
            .expect("sales balance");
    assert_eq!(checking_balance, 10_000);
    assert_eq!(sales_balance, 10_000);
}

#[tokio::test]
async fn rejects_unbalanced_entry() {
    let fx = setup().await;
    let entry = JournalEntry {
        id: None,
        date: date(),
        description: "Unbalanced".into(),
        lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 9_900)],
        currency: None,
    };

    let err = post_transaction(&fx.pool, entry).await.expect_err("unbalanced entry must be rejected");

    assert!(err.starts_with("JE_UNBALANCED"), "unexpected error: {err}");
    assert!(err.contains("$100.00") && err.contains("$99.00") && err.contains("$1.00"), "{err}");
    assert_eq!(row_counts(&fx.pool).await, (0, 0));
}

#[tokio::test]
async fn rejects_line_with_both_debit_and_credit() {
    let fx = setup().await;
    let entry = JournalEntry {
        id: None,
        date: date(),
        description: "Two-sided line".into(),
        lines: vec![
            JournalLine { account_id: fx.checking, debit: 10_000, credit: 10_000, memo: None, class_id: None },
            JournalLine::debit(fx.sales, 5_000),
            JournalLine::credit(fx.sales, 5_000),
        ],
        currency: None,
    };

    let err = post_transaction(&fx.pool, entry).await.expect_err("two-sided line must be rejected");

    assert!(err.starts_with("JE_LINE_BOTH_SIDES: line 1"), "unexpected error: {err}");
    assert_eq!(row_counts(&fx.pool).await, (0, 0));
}

#[tokio::test]
async fn rolls_back_header_when_a_later_step_fails() {
    let fx = setup().await;
    sqlx::query("UPDATE chart_of_accounts SET is_active = 0 WHERE id = ?1")
        .bind(fx.sales)
        .execute(&fx.pool)
        .await
        .expect("deactivate sales");
    let entry = JournalEntry {
        id: None,
        date: date(),
        description: "Posts to inactive account".into(),
        lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
        currency: None,
    };

    let err = post_transaction(&fx.pool, entry).await.expect_err("inactive account must be rejected");

    assert!(err.starts_with("JE_INACTIVE_ACCOUNT: line 2"), "unexpected error: {err}");
    // The DRAFT header and line 1 were written before the failure; rollback must remove both.
    assert_eq!(row_counts(&fx.pool).await, (0, 0));
}

#[test]
fn formats_cents() {
    assert_eq!(format_cents(0), "$0.00");
    assert_eq!(format_cents(1_050), "$10.50");
    assert_eq!(format_cents(-100), "-$1.00");
    assert_eq!(format_cents(123_456_789), "$1,234,567.89");
}

#[tokio::test]
async fn voiding_reverses_unwinds_documents_and_audits() {
    use crate::invoices::{create_and_post_invoice, list_invoices, record_payment, InvoiceInput, InvoiceLineInput, PaymentInput};
    let fx = setup().await;
    let services = account_id(&fx.pool, "4100").await;
    let ar = account_id(&fx.pool, "1200").await;
    let bal = |account: i64| {
        let pool = fx.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
                .bind(account)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };

    let invoice = create_and_post_invoice(
        &fx.pool,
        InvoiceInput {
            customer_name: "Acme".into(),
            issue_date: date(),
            due_date: date(),
            lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: 50_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    let paid = record_payment(
        &fx.pool,
        PaymentInput { invoice_id: invoice.id, deposit_account_id: fx.checking, payment_date: date(), amount_cents: 50_000, payment_method: "ACH".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .unwrap();
    assert_eq!(paid.status, "PAID");

    // An invoice with payments cannot be voided until the payment is.
    let err = void_transaction(&fx.pool, invoice.journal_entry_id.unwrap()).await.expect_err("has payments");
    assert!(err.starts_with("VOID_HAS_PAYMENTS"), "{err}");

    let payment_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoice_payments").fetch_one(&fx.pool).await.unwrap();
    void_transaction(&fx.pool, payment_entry).await.expect("payment voids");
    let reopened = list_invoices(&fx.pool).await.unwrap().remove(0);
    assert_eq!((reopened.status.as_str(), reopened.balance_cents), ("UNPAID", 50_000));
    assert_eq!((bal(fx.checking).await, bal(ar).await), (0, 50_000));

    void_transaction(&fx.pool, invoice.journal_entry_id.unwrap()).await.expect("invoice voids");
    assert_eq!((bal(ar).await, bal(services).await), (0, 0));
    assert!(list_invoices(&fx.pool).await.unwrap()[0].is_voided);

    // Nothing was deleted: originals and reversals are all still posted.
    let (posted, voided, reversals): (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), SUM(is_voided), COUNT(reverses_entry_id) FROM journal_entries WHERE status = 'POSTED'",
    )
    .fetch_one(&fx.pool)
    .await
    .unwrap();
    assert_eq!((posted, voided, reversals), (4, 2, 2));

    let err = void_transaction(&fx.pool, payment_entry).await.expect_err("double void");
    assert!(err.starts_with("VOID_ALREADY"), "{err}");
    let reversal: i64 = sqlx::query_scalar("SELECT id FROM journal_entries WHERE reverses_entry_id = ?1")
        .bind(payment_entry)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    assert!(void_transaction(&fx.pool, reversal).await.unwrap_err().starts_with("VOID_REVERSAL"));

    let log = list_audit_log(&fx.pool, 50).await.unwrap();
    assert_eq!(log.iter().filter(|a| a.action == "VOID").count(), 2);
    assert_eq!(log.iter().filter(|a| a.action == "POST").count(), 4);
    assert!(sqlx::query("DELETE FROM audit_log").execute(&fx.pool).await.unwrap_err().to_string().contains("AUDIT_IMMUTABLE"));

    let register = list_journal_entries(&fx.pool, 10).await.unwrap();
    let original = register.iter().find(|e| e.id == payment_entry).unwrap();
    assert!(original.is_voided && original.reversed_by_entry_number.is_some());
    assert_eq!(journal_entry_lines(&fx.pool, reversal).await.unwrap()[0].credit_cents, 50_000);
}

#[tokio::test]
async fn reconciled_entries_cannot_be_voided() {
    let fx = setup().await;
    let entry = post_journal_entry(
        &fx.pool,
        JournalEntry { id: None, date: date(), description: "Sale".into(), lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)], currency: None },
    )
    .await
    .unwrap();
    let rec = crate::reconcile::start(&fx.pool, fx.checking, date(), 10_000).await.unwrap();
    let lines = crate::reconcile::unreconciled_lines(&fx.pool, fx.checking).await.unwrap();
    crate::reconcile::set_lines_cleared(&fx.pool, rec.id, &[lines[0].line_id], true).await.unwrap();
    assert!(void_transaction(&fx.pool, entry).await.unwrap_err().starts_with("VOID_CLEARED"));

    crate::reconcile::lock(&fx.pool, rec.id).await.unwrap();
    assert!(void_transaction(&fx.pool, entry).await.unwrap_err().starts_with("VOID_RECONCILED"));
    let still_posted: bool = sqlx::query_scalar("SELECT is_voided FROM journal_entries WHERE id = ?1").bind(entry).fetch_one(&fx.pool).await.unwrap();
    assert!(!still_posted);
}

#[test]
fn conversion_balances_rounding_on_the_short_side() {
    // 3 × 33.33 EUR = 99.99 EUR at 1.085: each line rounds separately (36.16 × 3 = 108.48),
    // the total rounds to 108.49; the 0.01 lands on the largest line of the short side.
    let lines = vec![
        JournalLine::debit(1, 3_333),
        JournalLine::debit(2, 3_333),
        JournalLine::debit(3, 3_333),
        JournalLine::credit(4, 9_999),
    ];
    let home = convert_lines(&lines, 1_085_000).unwrap();
    let debits: i64 = home.iter().map(|l| l.0).sum();
    let credits: i64 = home.iter().map(|l| l.1).sum();
    assert_eq!(debits, credits);
    assert_eq!(home[3].1, 10_849);
    assert_eq!(home.iter().map(|l| l.0).collect::<Vec<_>>(), vec![3_617, 3_616, 3_616, 0]);
    assert!(convert_lines(&[JournalLine::debit(1, 1), JournalLine::credit(2, 1)], 100_000).unwrap_err().starts_with("FX_ROUNDS_TO_ZERO"));
}

#[tokio::test]
async fn foreign_journal_entry_posts_home_amounts_and_keeps_foreign_detail() {
    let fx = setup().await;
    let eur = |rate| Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: rate });
    let entry = |currency| JournalEntry {
        id: None,
        date: date(),
        description: "Euro sale".into(),
        lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
        currency,
    };
    let id = post_journal_entry(&fx.pool, entry(eur(1_087_500))).await.expect("posts");
    let rows: Vec<(i64, i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT debit_cents, credit_cents, foreign_debit_cents, foreign_credit_cents FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
    )
    .bind(id)
    .fetch_all(&fx.pool)
    .await
    .unwrap();
    assert_eq!(rows, vec![(10_875, 0, Some(10_000), Some(0)), (0, 10_875, Some(0), Some(10_000))]);
    let (code, rate): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT foreign_currency_code, exchange_rate_e6 FROM journal_entries WHERE id = ?1").bind(id).fetch_one(&fx.pool).await.unwrap();
    assert_eq!((code.as_deref(), rate), (Some("EUR"), Some(1_087_500)));

    let usd = Some(ForeignCurrency { code: "USD".into(), exchange_rate_e6: 1_000_000 });
    assert!(post_journal_entry(&fx.pool, entry(usd)).await.unwrap_err().starts_with("FX_IS_HOME"));
    assert!(post_journal_entry(&fx.pool, entry(Some(ForeignCurrency { code: "eur".into(), exchange_rate_e6: 1 }))).await.unwrap_err().starts_with("FX_BAD_CODE"));
    assert!(post_journal_entry(&fx.pool, entry(eur(0))).await.unwrap_err().starts_with("FX_BAD_RATE"));

    // The home currency is locked once foreign transactions exist.
    assert!(crate::preferences::set(&fx.pool, "home_currency_code", "CAD").await.unwrap_err().starts_with("PREF_HOME_CURRENCY_LOCKED"));
    assert_eq!(crate::preferences::set(&fx.pool, "home_currency_code", "usd").await.unwrap(), "USD");
}

async fn post_sale(fx: &Fixture, cents: i64, memo: &str) -> i64 {
    post_journal_entry(
        &fx.pool,
        JournalEntry {
            id: None,
            date: date(),
            description: memo.into(),
            lines: vec![JournalLine::debit(fx.checking, cents).with_memo(Some("deposit")), JournalLine::credit(fx.sales, cents)],
            currency: None,
        },
    )
    .await
    .unwrap()
}

async fn sql_err(pool: &SqlitePool, sql: &str) -> String {
    sqlx::query(sql).execute(pool).await.map(|_| String::new()).unwrap_or_else(db_err)
}

#[tokio::test]
async fn posting_and_voiding_write_immutable_snapshots() {
    let fx = setup().await;
    let kept = post_sale(&fx, 12_345, "Cash sale").await;
    let voided = post_sale(&fx, 500, "Wrong amount").await;
    void_transaction(&fx.pool, voided).await.unwrap();

    let snapshot: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1")
        .bind(kept)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(snapshot["memo"], "Cash sale");
    assert_eq!(snapshot["totalDebitCents"], 12_345);
    assert_eq!(snapshot["lines"][0]["accountId"], fx.checking);
    assert_eq!(snapshot["lines"][0]["memo"], "deposit");
    assert_eq!(snapshot["lines"][1]["creditCents"], 12_345);

    let void: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'VOID' AND target_entity_id = ?1")
        .bind(voided)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let void: serde_json::Value = serde_json::from_str(&void).unwrap();
    let reversal: i64 = sqlx::query_scalar("SELECT id FROM journal_entries WHERE reverses_entry_id = ?1").bind(voided).fetch_one(&fx.pool).await.unwrap();
    assert_eq!(void["reversalEntryId"], reversal, "the void links to the correcting entry");
    assert_eq!(void["amountCents"], 500);

    let verification = verify_ledger_audit(&fx.pool).await.unwrap();
    assert!(verification.intact, "{:?}", verification.findings);
    assert_eq!((verification.entries_checked, verification.voids_checked), (3, 1));

    assert!(sql_err(&fx.pool, "UPDATE ledger_audit_log SET payload_snapshot = '{}'").await.starts_with("AUDIT_IMMUTABLE"));
    assert!(sql_err(&fx.pool, "DELETE FROM ledger_audit_log").await.starts_with("AUDIT_IMMUTABLE"));
}

#[tokio::test]
async fn posted_memos_and_account_types_with_history_are_locked() {
    let fx = setup().await;
    let entry = post_sale(&fx, 1_000, "Original memo").await;
    let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET memo = 'Edited' WHERE id = {entry}")).await;
    assert!(err.starts_with("JE_IMMUTABLE"), "{err}");

    let err = sql_err(&fx.pool, &format!("UPDATE chart_of_accounts SET account_type = 'EXPENSE' WHERE id = {}", fx.checking)).await;
    assert!(err.starts_with("COA_TYPE_LOCKED"), "{err}");

    // Renaming is allowed, and recorded with before/after values.
    sqlx::query("UPDATE chart_of_accounts SET name = 'Operating Checking' WHERE id = ?1").bind(fx.checking).execute(&fx.pool).await.unwrap();
    let change: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'ACCOUNT_CHANGE' AND target_entity_id = ?1")
        .bind(fx.checking)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let change: serde_json::Value = serde_json::from_str(&change).unwrap();
    assert_eq!(change["before"]["name"], "Checking");
    assert_eq!(change["after"]["name"], "Operating Checking");

    // An account with no posted history can still change type.
    let unused = account_id(&fx.pool, "4100").await;
    sqlx::query("UPDATE chart_of_accounts SET account_type = 'OTHER_INCOME' WHERE id = ?1").bind(unused).execute(&fx.pool).await.unwrap();
}

#[tokio::test]
async fn verification_detects_tampering_that_bypasses_the_triggers() {
    let fx = setup().await;
    let altered = post_sale(&fx, 1_000, "Altered").await;
    let deleted = post_sale(&fx, 2_000, "Deleted").await;
    let untouched = post_sale(&fx, 3_000, "Untouched").await;
    assert!(verify_ledger_audit(&fx.pool).await.unwrap().intact);

    // Simulate someone editing the file with an outside tool (the sqlite3 shell runs with
    // foreign keys off) after removing the protecting triggers.
    for sql in [
        "PRAGMA foreign_keys = OFF",
        "DROP TRIGGER trg_jl_update_draft_only",
        "DROP TRIGGER trg_jl_delete_draft_only",
        "DROP TRIGGER trg_je_no_delete_posted",
        "DROP TRIGGER trg_ledger_audit_no_delete",
    ] {
        sqlx::query(sql).execute(&fx.pool).await.unwrap();
    }
    sqlx::query("UPDATE journal_lines SET debit_cents = 9000 WHERE journal_entry_id = ?1 AND debit_cents > 0").bind(altered).execute(&fx.pool).await.unwrap();
    sqlx::query("DELETE FROM journal_lines WHERE journal_entry_id = ?1").bind(deleted).execute(&fx.pool).await.unwrap();
    sqlx::query("DELETE FROM journal_entries WHERE id = ?1").bind(deleted).execute(&fx.pool).await.unwrap();

    let verification = verify_ledger_audit(&fx.pool).await.unwrap();
    let problems: Vec<(i64, &str)> = verification.findings.iter().map(|f| (f.entry_id, f.problem.as_str())).collect();
    assert!(problems.contains(&(altered, "Differs from its posting snapshot (changed after posting)")), "{problems:?}");
    assert!(problems.contains(&(altered, "Debits do not equal credits")), "{problems:?}");
    assert!(problems.contains(&(deleted, "Snapshotted at posting but no longer in the ledger (deleted)")), "{problems:?}");
    assert!(!problems.iter().any(|(id, _)| *id == untouched));
    let deleted_number = verification.findings.iter().find(|f| f.entry_id == deleted).unwrap().entry_number;
    assert_eq!(deleted_number, Some(2), "the snapshot still knows the deleted entry's number");

    // Erasing the snapshot itself is caught as an entry posted without one.
    sqlx::query("DELETE FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1").bind(untouched).execute(&fx.pool).await.unwrap();
    let verification = verify_ledger_audit(&fx.pool).await.unwrap();
    assert!(verification.findings.iter().any(|f| f.entry_id == untouched && f.problem == "Posted without a posting snapshot"));
}

#[tokio::test]
async fn posted_entries_cannot_leave_the_ledger_by_status_change() {
    let fx = setup().await;
    let id = post_sale(&fx, 50_000, "Sale").await;

    let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET status = 'VOID', voided_at = 'x' WHERE id = {id}")).await;
    assert!(err.starts_with("JE_INVALID_TRANSITION"), "{err}");
    let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET status = 'DRAFT', posted_at = NULL WHERE id = {id}")).await;
    assert!(err.starts_with("JE_INVALID_TRANSITION"), "{err}");
    let err = sql_err(&fx.pool, &format!("INSERT INTO legacy_status_voids (entry_id) VALUES ({id})")).await;
    assert!(err.starts_with("AUDIT_IMMUTABLE"), "{err}");

    // Forced through with the trigger removed, the verifier now catches it.
    sqlx::query("DROP TRIGGER trg_je_status_transition").execute(&fx.pool).await.unwrap();
    sqlx::query("UPDATE journal_entries SET status = 'VOID', voided_at = 'x' WHERE id = ?1").bind(id).execute(&fx.pool).await.unwrap();
    let verification = verify_ledger_audit(&fx.pool).await.unwrap();
    assert!(!verification.intact);
    assert!(verification
        .findings
        .iter()
        .any(|f| f.entry_id == id && f.problem == "Removed from balances by a status change (no reversing entry)"));
}

#[tokio::test]
async fn schema_rejects_unbalanced_or_missing_foreign_amounts() {
    let fx = setup().await;
    let draft = |n: i64, currency: bool| {
        format!(
            "INSERT INTO journal_entries (entry_number, entry_date, foreign_currency_code, exchange_rate_e6) VALUES ({n}, '2026-10-13', {}, {}) RETURNING id",
            if currency { "'EUR'" } else { "NULL" },
            if currency { "1100000" } else { "NULL" }
        )
    };
    let lines = |id: i64, fd: &str, fc: &str| {
        format!(
            "INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents, foreign_debit_cents, foreign_credit_cents) VALUES ({id}, 1, {}, 1100, {fd}, 0);
                 INSERT INTO journal_lines (journal_entry_id, line_number, account_id, credit_cents, foreign_debit_cents, foreign_credit_cents) VALUES ({id}, 2, {}, 1100, 0, {fc});",
            fx.checking, fx.sales
        )
    };
    let post = |id: i64| format!("UPDATE journal_entries SET status = 'POSTED', posted_at = 'now' WHERE id = {id}");

    let unbalanced: i64 = sqlx::query_scalar(&draft(901, true)).fetch_one(&fx.pool).await.unwrap();
    sqlx::raw_sql(&lines(unbalanced, "1000", "999")).execute(&fx.pool).await.unwrap();
    assert!(sql_err(&fx.pool, &post(unbalanced)).await.starts_with("JE_FOREIGN_UNBALANCED"));

    let missing: i64 = sqlx::query_scalar(&draft(902, true)).fetch_one(&fx.pool).await.unwrap();
    sqlx::raw_sql(&lines(missing, "NULL", "1000")).execute(&fx.pool).await.unwrap();
    assert!(sql_err(&fx.pool, &post(missing)).await.starts_with("JE_FOREIGN_MISSING"));

    let home: i64 = sqlx::query_scalar(&draft(903, false)).fetch_one(&fx.pool).await.unwrap();
    sqlx::raw_sql(&lines(home, "1000", "1000")).execute(&fx.pool).await.unwrap();
    assert!(sql_err(&fx.pool, &post(home)).await.starts_with("JE_FOREIGN_ON_HOME"));

    let good: i64 = sqlx::query_scalar(&draft(904, true)).fetch_one(&fx.pool).await.unwrap();
    sqlx::raw_sql(&lines(good, "1000", "1000")).execute(&fx.pool).await.unwrap();
    assert_eq!(sql_err(&fx.pool, &post(good)).await, "");
}

/// The snapshot format from before classes existed (migration 0013), kept to prove that entries
/// without a class still snapshot byte-for-byte as they did when they were posted.
const PRE_CLASS_LINES_SQL: &str = "SELECT json_group_array(json_object(
            'lineNumber', l.line_number, 'accountId', l.account_id, 'debitCents', l.debit_cents,
            'creditCents', l.credit_cents, 'memo', l.memo, 'foreignDebitCents', l.foreign_debit_cents,
            'foreignCreditCents', l.foreign_credit_cents) ORDER BY l.line_number)
        FROM journal_lines l WHERE l.journal_entry_id = ?1";

#[tokio::test]
async fn classes_join_the_snapshot_without_changing_unclassified_ones() {
    let fx = setup().await;
    let rent = account_id(&fx.pool, "6300").await;
    let east = crate::classes::create_class(&fx.pool, crate::classes::ClassInput { name: "East".into(), parent_id: None }).await.unwrap().id;
    let post = |lines: Vec<JournalLine>| {
        post_journal_entry(&fx.pool, JournalEntry { id: None, date: date(), description: "Caf\u{e9} \"rent\" \u{1F680}".into(), lines, currency: None })
    };

    let plain = post(vec![JournalLine::debit(rent, 1_250).with_memo(Some("tab\there")), JournalLine::credit(fx.checking, 1_250)]).await.unwrap();
    let (live, old): (String, String) = sqlx::query_as(&format!(
        "SELECT json_extract(snapshot, '$.lines'), ({PRE_CLASS_LINES_SQL}) FROM v_entry_snapshot WHERE entry_id = ?1"
    ))
    .bind(plain)
    .fetch_one(&fx.pool)
    .await
    .unwrap();
    assert_eq!(live, old);

    let classed = post(vec![JournalLine::debit(rent, 700).with_class(Some(east)), JournalLine::credit(fx.checking, 700)]).await.unwrap();
    let snapshot: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1")
        .bind(classed)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(snapshot["lines"][0]["classId"], east);
    assert!(snapshot["lines"][1].get("classId").is_none());

    // The void's reversing entry carries the class, so the class nets to zero.
    void_transaction(&fx.pool, classed).await.unwrap();
    let east_net: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) - SUM(credit_cents) FROM v_posted_lines WHERE class_id = ?1")
        .bind(east)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    assert_eq!(east_net, 0);
    assert!(verify_ledger_audit(&fx.pool).await.unwrap().intact);

    // Moving a posted line to another class (bypassing the triggers) is detected.
    let west = crate::classes::create_class(&fx.pool, crate::classes::ClassInput { name: "West".into(), parent_id: None }).await.unwrap().id;
    sqlx::query("DROP TRIGGER trg_jl_update_draft_only").execute(&fx.pool).await.unwrap();
    sqlx::query("UPDATE journal_lines SET class_id = ?1 WHERE journal_entry_id = ?2 AND class_id IS NOT NULL").bind(west).bind(classed).execute(&fx.pool).await.unwrap();
    let verification = verify_ledger_audit(&fx.pool).await.unwrap();
    assert!(verification.findings.iter().any(|f| f.entry_id == classed && f.problem.starts_with("Differs from its posting snapshot")));
}
