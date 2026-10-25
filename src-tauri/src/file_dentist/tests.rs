use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};
use chrono::NaiveDate;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn post_sale(pool: &SqlitePool, cents: i64) -> i64 {
    let (checking, sales) = (account(pool, "1010").await, account(pool, "4000").await);
    post_journal_entry(
        pool,
        JournalEntry {
            id: None,
            date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: "Sale".into(),
            lines: vec![JournalLine::debit(checking, cents), JournalLine::credit(sales, cents)],
            currency: None,
        },
    )
    .await
    .unwrap()
}

fn result<'a>(report: &'a CheckupReport, id: &str) -> &'a CheckResult {
    report.checks.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no {id} check"))
}

#[tokio::test]
async fn a_fresh_company_file_passes_every_check() {
    let pool = test_pool().await;
    post_sale(&pool, 12_500).await;
    let report = checkup(&pool, 0).await.unwrap();
    let failing: Vec<_> = report.checks.iter().filter(|c| c.status != CheckStatus::Pass).collect();
    assert!(failing.is_empty(), "{failing:#?}");
    assert_eq!(report.status, CheckStatus::Pass);
    assert!(report.can_rebuild);
    assert_eq!(report.checks.len(), 8);
}

#[tokio::test]
async fn rebuild_restores_a_dropped_safeguard_trigger_and_a_changed_index() {
    let pool = test_pool().await;
    let trigger: String = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name LIMIT 1").fetch_one(&pool).await.unwrap();
    let (index, table): (String, String) =
        sqlx::query_as("SELECT name, tbl_name FROM sqlite_schema WHERE type = 'index' AND sql IS NOT NULL ORDER BY name LIMIT 1").fetch_one(&pool).await.unwrap();
    sqlx::query(&format!("DROP TRIGGER {}", quote_ident(&trigger))).execute(&pool).await.unwrap();
    sqlx::query(&format!("DROP INDEX {}", quote_ident(&index))).execute(&pool).await.unwrap();
    sqlx::query(&format!("CREATE INDEX {} ON {} (id)", quote_ident(&index), quote_ident(&table))).execute(&pool).await.unwrap();
    sqlx::query("CREATE TRIGGER sneaky AFTER INSERT ON journal_entries BEGIN SELECT 1; END").execute(&pool).await.unwrap();

    let before = checkup(&pool, 0).await.unwrap();
    let structure = result(&before, "STRUCTURE");
    assert_eq!(structure.status, CheckStatus::Fail);
    assert!(structure.fixed_by_rebuild);
    assert!(structure.details.contains(&format!("Missing trigger {trigger}")), "{:?}", structure.details);
    assert!(structure.details.contains(&format!("Changed {index}")), "{:?}", structure.details);
    assert!(structure.details.contains(&"Unknown trigger sneaky".to_string()), "{:?}", structure.details);

    let actions = rebuild_pool(&pool).await.unwrap();
    assert!(actions.contains(&format!("Recreated missing trigger {trigger}")), "{actions:?}");
    assert!(actions.contains(&format!("Restored changed index {index}")), "{actions:?}");
    assert!(actions.contains(&"Removed unknown trigger sneaky".to_string()), "{actions:?}");
    assert!(actions.contains(&"Compacted the file".to_string()));

    let after = checkup(&pool, 0).await.unwrap();
    assert_eq!(after.status, CheckStatus::Pass, "{:#?}", after.checks);
}

#[tokio::test]
async fn rebuilding_a_healthy_file_changes_no_data() {
    let pool = test_pool().await;
    post_sale(&pool, 4_200).await;
    let count = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, i64>("SELECT (SELECT COUNT(*) FROM journal_lines) * 1000 + (SELECT COUNT(*) FROM ledger_audit_log)").fetch_one(&pool).await.unwrap()
    };
    let before = count(pool.clone()).await;
    let actions = rebuild_pool(&pool).await.unwrap();
    assert_eq!(actions, ["Rebuilt every index", "Refreshed query statistics", "Optimized queries", "Compacted the file"]);
    assert_eq!(count(pool.clone()).await, before);
}

/// Runs `sql` with `table`'s triggers removed, the way a tool editing the file outside
/// LocalBooks would, then puts them back.
async fn edit_behind_safeguards(pool: &SqlitePool, table: &str, sql: &str, bind: i64) {
    let triggers: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ?1").bind(table).fetch_all(pool).await.unwrap();
    for (name, _) in &triggers {
        sqlx::query(&format!("DROP TRIGGER {}", quote_ident(name))).execute(pool).await.unwrap();
    }
    sqlx::query(sql).bind(bind).execute(pool).await.unwrap();
    for (_, sql) in &triggers {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
}

#[tokio::test]
async fn an_entry_edited_outside_localbooks_fails_balance_and_history_but_is_not_rewritten() {
    let pool = test_pool().await;
    let entry = post_sale(&pool, 10_000).await;
    edit_behind_safeguards(&pool, "journal_lines", "UPDATE journal_lines SET debit_cents = 9000 WHERE journal_entry_id = ?1 AND debit_cents > 0", entry).await;

    let report = checkup(&pool, 0).await.unwrap();
    assert_eq!(result(&report, "BALANCE").status, CheckStatus::Fail);
    assert!(!result(&report, "BALANCE").fixed_by_rebuild);
    assert_eq!(result(&report, "AUDIT").status, CheckStatus::Fail);
    assert_eq!(result(&report, "STRUCTURE").status, CheckStatus::Pass);

    rebuild_pool(&pool).await.unwrap();
    let debit: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = ?1").bind(entry).fetch_one(&pool).await.unwrap();
    assert_eq!(debit, 9000, "rebuild never rewrites ledger data");
}

#[tokio::test]
async fn an_invoice_paid_total_out_of_step_with_its_payments_is_reported() {
    use crate::invoices::{self, InvoiceInput, InvoiceLineInput, PaymentInput};
    let pool = test_pool().await;
    let day = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    let invoice = invoices::create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Acme".into(),
            issue_date: day,
            due_date: day,
            lines: vec![InvoiceLineInput {
                description: "Work".into(),
                quantity_e4: 10_000,
                rate_cents: 5_000,
                income_account_id: account(&pool, "4000").await,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    invoices::record_payment(
        &pool,
        PaymentInput {
            invoice_id: invoice.id,
            deposit_account_id: account(&pool, "1010").await,
            payment_date: day,
            amount_cents: 2_000,
            payment_method: "CHECK".into(),
            reference: None,
            exchange_rate_e6: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(result(&checkup(&pool, 0).await.unwrap(), "TOTALS").status, CheckStatus::Pass);

    edit_behind_safeguards(&pool, "invoice_payments", "UPDATE invoice_payments SET amount_cents = 1500 WHERE invoice_id = ?1", invoice.id).await;
    let report = checkup(&pool, 0).await.unwrap();
    let totals = result(&report, "TOTALS");
    assert_eq!(totals.status, CheckStatus::Fail);
    assert!(!totals.fixed_by_rebuild);
    assert_eq!(totals.details, [format!("Invoice {}: stored 2000, records add up to 1500", invoice.invoice_number)]);
}
