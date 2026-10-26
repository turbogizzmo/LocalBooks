//! Golden snapshots of the Accountant & Taxes reports on the fixture company. Harbor Lane
//! reconciles checking for the first quarter; `company()` adds April and May statements (each
//! leaving its last transaction outstanding), tax lines on its accounts and two purchase orders.

use std::fmt::Write;

use sqlx::SqlitePool;

use super::account;
use super::fixture::{d, harbor_lane};
use super::report_presets::render;
use crate::accountant_reports::{self as ar, AccountantReport};

fn text(title: &str, report: &AccountantReport) -> String {
    let mut out = render(title, &report.report);
    for note in &report.notes {
        writeln!(out, "Note: {note}").unwrap();
    }
    out
}

/// Reconciles every open checking line dated on or before `end`, except the last one.
async fn reconcile(pool: &SqlitePool, end: chrono::NaiveDate) -> i64 {
    let checking = account(pool, "1010").await;
    let lines: Vec<_> = crate::reconcile::unreconciled_lines(pool, checking).await.unwrap().into_iter().filter(|l| l.entry_date <= end.to_string()).collect();
    let clear = &lines[..lines.len() - 1];
    let state = crate::reconcile::account_state(pool, checking).await.unwrap();
    let ending = state.reconciled_balance_cents + clear.iter().map(|l| l.amount_cents).sum::<i64>();
    let rec = crate::reconcile::start(pool, checking, end, ending).await.expect("start");
    crate::reconcile::set_lines_cleared(pool, rec.id, &clear.iter().map(|l| l.line_id).collect::<Vec<_>>(), true).await.expect("tick");
    crate::reconcile::lock(pool, rec.id).await.expect("lock");
    rec.id
}

async fn company() -> (SqlitePool, i64, i64) {
    let pool = harbor_lane().await;
    let april = reconcile(&pool, d(2025, 4, 30)).await;
    let may = reconcile(&pool, d(2025, 5, 31)).await;
    let office = account(&pool, "6250").await;
    let po = |vendor: &str, date, amount| crate::workflows::PurchaseOrderInput {
        vendor_name: vendor.into(),
        po_date: date,
        expected_date: Some(d(2026, 1, 15)),
        memo: Some("Paper and toner".into()),
        lines: vec![crate::workflows::PurchaseOrderLineInput { account_id: office, product_service_id: None, description: None, quantity_e4: None, rate_cents: None, amount_cents: amount, class_id: None }],
    };
    crate::workflows::create_purchase_order(&pool, po("Pacific Office Outfitters", d(2025, 12, 18), 64_000)).await.expect("po");
    crate::workflows::create_purchase_order(&pool, po("Riverside Print Shop", d(2025, 12, 22), 23_500)).await.expect("po");
    (pool, april, may)
}

async fn assign(pool: &SqlitePool, lines: &[(&str, &str)]) {
    for (number, line) in lines {
        ar::set_account_tax_line(pool, account(pool, number).await, Some(line)).await.unwrap_or_else(|e| panic!("{number}: {e}"));
    }
}

#[tokio::test]
async fn previous_reconciliations() {
    let (pool, april, may) = company().await;
    let mut out = String::new();
    for (title, id) in [("April", april), ("May", may)] {
        out += &text(&format!("Reconciliation Detail, {title}"), &ar::previous_reconciliation(&pool, id, true).await.unwrap());
        out += "\n";
        out += &text(&format!("Reconciliation Summary, {title}"), &ar::previous_reconciliation(&pool, id, false).await.unwrap());
        out += "\n";
    }
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn reconciliation_discrepancy() {
    let (pool, april, _) = company().await;
    let clean = ar::discrepancy(&pool, None, d(2025, 12, 31)).await.unwrap();
    assert_eq!(clean.report.line_count, 0, "nothing changed after reconciling");
    // The company file is edited outside LocalBooks, past the schema's guards.
    let triggers: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name IN ('journal_lines', 'journal_entries')")
        .fetch_all(&pool)
        .await
        .unwrap();
    for t in triggers {
        sqlx::query(&format!("DROP TRIGGER {t}")).execute(&pool).await.unwrap();
    }
    let cleared: Vec<(i64, i64)> = sqlx::query_as("SELECT id, journal_entry_id FROM journal_lines WHERE reconciliation_id = ?1 ORDER BY id LIMIT 3").bind(april).fetch_all(&pool).await.unwrap();
    sqlx::query("UPDATE journal_lines SET debit_cents = CASE WHEN debit_cents > 0 THEN debit_cents + 10000 ELSE 0 END, credit_cents = CASE WHEN credit_cents > 0 THEN credit_cents + 10000 ELSE 0 END WHERE id = ?1").bind(cleared[0].0).execute(&pool).await.unwrap();
    sqlx::query("UPDATE journal_entries SET is_voided = 1 WHERE id = ?1").bind(cleared[1].1).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM journal_lines WHERE id = ?1").bind(cleared[2].0).execute(&pool).await.unwrap();
    let report = ar::discrepancy(&pool, None, d(2025, 12, 31)).await.unwrap();
    assert_eq!(report.report.line_count, 3);
    insta::assert_snapshot!(text("Reconciliation Discrepancy", &report));
}

#[tokio::test]
async fn income_tax_schedule_c() {
    let (pool, _, _) = company().await;
    assign(
        &pool,
        &[
            ("4000", "SCHEDULE_C:1"),
            ("4100", "SCHEDULE_C:1"),
            ("6000", "SCHEDULE_C:8"),
            ("6050", "SCHEDULE_C:27a"),
            ("6250", "SCHEDULE_C:18"),
            ("6300", "SCHEDULE_C:20b"),
            ("6350", "SCHEDULE_C:27a"),
            ("6500", "SCHEDULE_C:25"),
            ("6700", "SCHEDULE_C:26"),
            ("6710", "SCHEDULE_C:23"),
            ("8000", "SCHEDULE_C:16b"),
        ],
    )
    .await;
    // An import that kept QuickBooks' own tax line number.
    sqlx::query("UPDATE chart_of_accounts SET tax_line = ?1 WHERE account_number = '6400'").bind(ar::quickbooks_tax_line(1462)).execute(&pool).await.unwrap();
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    let mut out = text("Income Tax Summary", &ar::income_tax(&pool, "SCHEDULE_C", start, end, false, None).await.unwrap());
    out += "\n";
    out += &text("Income Tax Detail, line 8", &ar::income_tax(&pool, "SCHEDULE_C", start, end, true, Some("SCHEDULE_C:8")).await.unwrap());
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn income_tax_1120s() {
    let (pool, _, _) = company().await;
    assign(
        &pool,
        &[
            ("4000", "1120S:1a"),
            ("4100", "1120S:1a"),
            ("6000", "1120S:16"),
            ("6300", "1120S:11"),
            ("6700", "1120S:8"),
            ("6710", "1120S:12"),
            ("8000", "1120S:13"),
            ("1010", "1120S:L-cash"),
            ("1020", "1120S:L-cash"),
            ("1500", "1120S:L-depreciable"),
            ("2100", "1120S:L-other-current-liabilities"),
            ("2500", "1120S:L-notes-long"),
        ],
    )
    .await;
    let report = ar::income_tax(&pool, "1120S", d(2025, 1, 1), d(2025, 12, 31), false, None).await.unwrap();
    insta::assert_snapshot!(text("Income Tax Summary, Form 1120-S", &report));
}

#[tokio::test]
async fn collections() {
    let (pool, _, _) = company().await;
    insta::assert_snapshot!(text("Collections Report", &ar::collections(&pool, d(2025, 12, 31)).await.unwrap()));
}

#[tokio::test]
async fn open_purchase_orders() {
    let (pool, _, _) = company().await;
    insta::assert_snapshot!(text("Open Purchase Orders", &ar::open_purchase_orders(&pool, d(2025, 12, 31)).await.unwrap()));
}
