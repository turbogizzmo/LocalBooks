//! The list reports and the other reports outside the engine (`list_reports.rs`) on the fixture
//! company: a golden snapshot of each (except the Audit Trail, whose rows carry the time the test
//! ran), and checks that each ties out to the report or records it summarizes.
//!
//! On top of the presets' company, `company()` adds contact details, a job under a customer, an
//! employee's time, an open purchase order, a tracked item and a part-paid bill split across jobs.

use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::SqlitePool;

use super::account;
use super::fixture::d;
use super::report_presets::{self, excel, render, today};
use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{ReportRow, RowKind};
use crate::expenses::{BillInput, BillLineInput, BillPaymentInput};
use crate::list_reports::{self, Dates};

async fn company() -> SqlitePool {
    let pool = report_presets::company().await;
    let p = &pool;
    let id = |table: &'static str, name: &'static str| {
        let p = p.clone();
        async move { sqlx::query_scalar::<_, i64>(&format!("SELECT id FROM {table} WHERE name = ?1")).bind(name).fetch_one(&p).await.unwrap_or_else(|_| panic!("{name}")) }
    };
    let cafe = id("customers", "Brightwater Cafe").await;
    sqlx::query("UPDATE customers SET phone = '555-0142', company_name = 'Brightwater Cafe Inc.', billing_address = '8 Pier Road\nHarbor Lane, OR 97001', terms_days = 30 WHERE id = ?1")
        .bind(cafe)
        .execute(p)
        .await
        .unwrap();
    let patio: i64 = sqlx::query_scalar("INSERT INTO customers (name, parent_id, phone) VALUES ('Patio Menu', ?1, '555-0143') RETURNING id").bind(cafe).fetch_one(p).await.unwrap();
    sqlx::query("UPDATE vendors SET phone = '555-0190', email = 'orders@riverside.example', terms_days = 30, is_1099 = 1 WHERE name = 'Riverside Print Shop'").execute(p).await.unwrap();

    // Time in November: billable design for the cafe, unbilled work on its patio menu job, and an
    // hour of office time for no one.
    let logo = id("products_services", "Logo Package").await;
    let dana = crate::timesheet::create_employee(p, "Dana Ortiz", None).await.unwrap().id;
    let time = |day: u32, job: Option<i64>, item: Option<i64>, minutes: i64, billable: bool, description: &str| crate::timesheet::TimeActivityInput {
        employee_id: dana,
        activity_date: d(2025, 11, day),
        customer_id: job,
        product_service_id: item,
        minutes,
        billable,
        hourly_rate_cents: billable.then_some(9_500),
        description: Some(description.into()),
        class_id: None,
    };
    crate::timesheet::create_time_activities(
        p,
        vec![
            time(3, Some(cafe), Some(logo), 90, true, "Logo concepts"),
            time(4, Some(patio), Some(logo), 140, false, "Menu layout"),
            time(5, None, None, 50, false, "Studio admin"),
        ],
    )
    .await
    .unwrap();

    // An open purchase order for print stock.
    let office = account(p, "6250").await;
    let prints = id("products_services", "Poster Prints").await;
    let po_line = |item, description: &str, qty: Option<i64>, rate: Option<i64>, amount| crate::workflows::PurchaseOrderLineInput {
        account_id: office,
        product_service_id: item,
        description: Some(description.into()),
        quantity_e4: qty,
        rate_cents: rate,
        amount_cents: amount,
        class_id: None,
    };
    crate::workflows::create_purchase_order(
        p,
        crate::workflows::PurchaseOrderInput {
            vendor_name: "Riverside Print Shop".into(),
            po_date: d(2025, 12, 5),
            expected_date: Some(d(2025, 12, 19)),
            memo: Some("January poster run".into()),
            lines: vec![po_line(Some(prints), "Poster stock", Some(1_000_000), Some(80), 8_000), po_line(None, "Rush delivery", None, None, 1_500)],
        },
    )
    .await
    .unwrap();

    // Twelve mugs into stock on a bill that also carries job costs for the cafe and its patio job;
    // $50.00 of it paid, so its open balance splits unevenly.
    let sales = account(p, "4000").await;
    let mugs = crate::sales::create_product(p, crate::sales::ProductInput { name: "Gift Mugs".into(), description: Some("Logo mugs".into()), rate_cents: 1_800, income_account_id: sales, is_taxable: true })
        .await
        .unwrap()
        .id;
    let tracked = crate::inventory::track_product(p, &crate::inventory::TrackInput { product_id: mugs, sku: Some("MUG-12".into()), asset_account_id: None, cogs_account_id: None }).await.unwrap();
    let bill_line = |account_id, amount_cents, memo: &str, qty: Option<i64>, rate: Option<i64>, item, job| BillLineInput {
        account_id,
        amount_cents,
        memo: Some(memo.into()),
        quantity_e4: qty,
        rate_cents: rate,
        product_service_id: item,
        class_id: None,
        customer_id: job,
        billable: None,
    };
    let bill = crate::expenses::post_vendor_bill(
        p,
        BillInput {
            terms_id: None,
            vendor_name: "Lakeside Ceramics".into(),
            bill_number: Some("LC-310".into()),
            bill_date: d(2025, 12, 2),
            due_date: d(2025, 12, 17),
            memo: None,
            total_cents: 15_800,
            lines: vec![
                bill_line(tracked.asset_account_id, 10_800, "Mugs", Some(120_000), Some(900), Some(mugs), None),
                bill_line(office, 3_000, "Menu card stock", None, None, None, Some(cafe)),
                bill_line(office, 2_000, "Patio sign", None, None, None, Some(patio)),
            ],
        },
    )
    .await
    .unwrap()
    .id;
    crate::expenses::pay_vendor_bill(p, BillPaymentInput { bill_id: bill, payment_account_id: account(p, "1010").await, payment_date: d(2025, 12, 10), amount_cents: 5_000, reference: None, to_be_printed: false })
        .await
        .unwrap();
    pool
}

/// Runs `id` with the dates the screen opens it with, pinned to the fixture's calendar.
async fn run(pool: &SqlitePool, id: &str) -> AccountantReport {
    let info = list_reports::catalog().into_iter().find(|r| r.id == id).unwrap_or_else(|| panic!("report {id}"));
    let (start, end) = match (info.dates, info.period) {
        (Dates::Range, "thisMonth") => (Some(d(2025, 11, 1)), Some(d(2025, 11, 30))),
        (Dates::Range, _) => (Some(d(2025, 1, 1)), Some(d(2025, 12, 31))),
        _ => (None, Some(today())),
    };
    list_reports::run(pool, id, start, end, today()).await.unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn cents(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn last(r: &ReportRow) -> i64 {
    cents(r.cells.last().unwrap())
}

fn total(report: &AccountantReport) -> &ReportRow {
    report.report.rows.iter().rev().find(|r| r.kind == RowKind::Total).expect("a total row")
}

fn details(report: &AccountantReport) -> impl Iterator<Item = &ReportRow> {
    report.report.rows.iter().filter(|r| r.kind == RowKind::Detail)
}

#[tokio::test]
async fn list_reports() {
    let pool = company().await;
    for info in list_reports::catalog() {
        let report = run(&pool, info.id).await;
        assert!(excel(info.title, &report.report).starts_with(b"PK"), "{} exports to Excel", info.id);
        if info.id == "audit-trail" {
            continue;
        }
        assert!(report.report.line_count > 0, "{} has lines", info.id);
        let shown = format!("{}\n{}", render(info.title, &report.report), report.notes.join("\n"));
        insta::assert_snapshot!(format!("list_{}", info.id.replace('-', "_")), shown);
    }
}

#[tokio::test]
async fn statement_summaries_tie_out() {
    let pool = company().await;
    // Balance Sheet Summary: the same totals as the Balance Sheet, and its lines add up to them.
    let bs = crate::statements::balance_sheet(&pool, today()).await.unwrap();
    let summary = run(&pool, "bs-summary").await;
    let totals: BTreeMap<String, i64> = summary.report.rows.iter().filter(|r| r.kind != RowKind::Detail && r.kind != RowKind::Section).map(|r| (text(&r.cells[0]), last(r))).collect();
    // A group folded into one line shows its total beside the group's name.
    let folded: BTreeMap<String, i64> = details(&summary).map(|r| (text(&r.cells[0]), last(r))).collect();
    for line in bs.lines.iter().filter(|l| matches!(l.kind, crate::statements::LineKind::Total | crate::statements::LineKind::Net)) {
        let shown = totals.get(&line.label).or_else(|| folded.get(line.label.trim_start_matches("Total ")));
        assert_eq!(shown, line.amount_cents.as_ref(), "{}", line.label);
    }
    let lines: i64 = details(&summary).map(last).sum();
    assert_eq!(lines, bs.total_assets_cents + bs.total_liabilities_cents + bs.total_equity_cents);
    assert!(details(&summary).count() < bs.lines.iter().filter(|l| l.account_id.is_some()).count(), "accounts fold into groups");

    // Profit & Loss YTD Comparison: each column's net income is a Profit & Loss for its dates.
    let ytd = run(&pool, "pl-ytd").await;
    let net = ytd.report.rows.last().unwrap();
    let month = crate::statements::profit_and_loss(&pool, d(2025, 11, 1), d(2025, 11, 30)).await.unwrap();
    let year = crate::statements::profit_and_loss(&pool, d(2025, 1, 1), d(2025, 11, 30)).await.unwrap();
    assert_eq!((cents(&net.cells[1]), cents(&net.cells[2])), (month.net_income_cents, year.net_income_cents));
    assert_ne!(month.net_income_cents, year.net_income_cents);
}

#[tokio::test]
async fn customer_and_vendor_reports_tie_out() {
    let pool = company().await;
    // Contact list balances: the receivables and payables on the balance sheet.
    let balance = |t: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(SUM(CASE WHEN a.normal_balance = 'DEBIT' THEN l.debit_cents - l.credit_cents ELSE l.credit_cents - l.debit_cents END), 0)
                 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id JOIN v_chart_of_accounts a ON a.id = l.account_id
                 WHERE a.account_type = ?1 AND e.status = 'POSTED' AND e.entry_date <= '2025-12-31'",
            )
            .bind(t)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let customers = run(&pool, "customer-contact").await;
    assert_eq!(last(total(&customers)), balance("ACCOUNTS_RECEIVABLE").await);
    let vendors = run(&pool, "vendor-contact").await;
    assert_eq!(last(total(&vendors)), balance("ACCOUNTS_PAYABLE").await);
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM customers WHERE is_active = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(run(&pool, "customer-phone").await.report.line_count as i64, active);
    let job = details(&customers).find(|r| text(&r.cells[0]) == "Brightwater Cafe:Patio Menu").expect("jobs under their customer");
    assert_eq!(text(&job.cells[3]), "555-0143");

    // Average Days to Pay: every invoice the fixture pays off is paid 27 days after it is issued,
    // except June's, half paid in July and the rest on September 25 (107 days).
    let days = run(&pool, "average-days-to-pay").await;
    let t = total(&days);
    assert_eq!((cents(&t.cells[1]), cents(&t.cells[2])), (11, (10 * 27 + 107 + 5) / 11));
    let paid_late = details(&days).find(|r| cents(&r.cells[2]) > 27).expect("the customer that paid late");
    let count = cents(&paid_late.cells[1]);
    assert_eq!(cents(&paid_late.cells[2]), ((count - 1) * 27 + 107 + count / 2) / count);

    // Unpaid Bills by Job: the bills on the A/P aging, with the part-paid bill's $108.00 left split
    // by its lines ($108.00, $30.00 and $20.00 of $158.00), the last job taking the rounding.
    let by_job = run(&pool, "unpaid-bills-job").await;
    let aging = crate::report_center::aging(&pool, true, today()).await.unwrap();
    let bills: i64 = aging.items.iter().filter(|i| i.kind == "Bill").map(|i| i.open_cents).sum();
    assert_eq!(last(total(&by_job)), bills);
    let shares: Vec<i64> = details(&by_job).filter(|r| text(&r.cells[3]) == "LC-310").map(last).collect();
    assert_eq!(shares, vec![2_051, 1_367, 7_382], "cafe, patio menu, then no job");

    // Open Purchase Orders Detail: the lines of the orders on Open Purchase Orders.
    let detail = run(&pool, "open-po-detail").await;
    let orders = crate::accountant_reports::open_purchase_orders(&pool, today()).await.unwrap();
    assert_eq!(last(total(&detail)), last(orders.report.rows.last().unwrap()));
    assert_eq!(detail.report.line_count, 2);
}

#[tokio::test]
async fn time_inventory_and_lists_tie_out() {
    let pool = company().await;
    let entries = crate::timesheet::list_time_activities(&pool, d(2025, 11, 1), d(2025, 11, 30)).await.unwrap();
    let minutes: i64 = entries.iter().map(|t| t.minutes).sum();
    let billable: i64 = entries.iter().filter(|t| t.billable).map(|t| t.minutes).sum();
    for id in ["time-job-summary", "time-name", "time-item"] {
        let r = run(&pool, id).await;
        let t = total(&r);
        let hours = |m: i64| (m * 10_000 + 30) / 60;
        assert_eq!((cents(&t.cells[1]), cents(&t.cells[2])), (hours(billable), hours(minutes)), "{id}");
        // Each row is its own minutes in hours, so rows add up to the total within a rounding.
        let rows: i64 = details(&r).map(|x| cents(&x.cells[2])).sum();
        assert!((rows - cents(&t.cells[2])).abs() <= details(&r).count() as i64, "{id}");
    }
    let detail = run(&pool, "time-job-detail").await;
    assert_eq!(detail.report.line_count, entries.len());
    assert_eq!(last(total(&detail)), (minutes * 10_000 + 30) / 60);

    // Physical Inventory Worksheet: today's quantity of every tracked item.
    let worksheet = run(&pool, "physical-inventory").await;
    let on_hand: Vec<(String, i64)> = sqlx::query_as("SELECT p.name, i.qty_on_hand_e4 FROM inventory_items i JOIN products_services p ON p.id = i.product_id ORDER BY p.name").fetch_all(&pool).await.unwrap();
    let shown: Vec<(String, i64)> = details(&worksheet).map(|r| (text(&r.cells[0]), cents(&r.cells[3]))).collect();
    assert_eq!(shown, on_hand);
    assert!(details(&worksheet).all(|r| r.cells[4].is_null()), "the count column is blank");

    // Account Listing: each balance sheet account's balance as the Balance Sheet shows it.
    let listing = run(&pool, "account-listing").await;
    let bs = crate::statements::balance_sheet(&pool, today()).await.unwrap();
    let listed: BTreeMap<i64, Option<i64>> = details(&listing).map(|r| (r.account_id.unwrap(), r.cells[2].as_i64())).collect();
    let mut checked = 0;
    for line in bs.lines.iter().filter(|l| l.kind == crate::statements::LineKind::Account) {
        if let (Some(id), Some(amount)) = (line.account_id, line.amount_cents) {
            if let Some(Some(shown)) = listed.get(&id) {
                assert_eq!(*shown, amount, "{}", line.label);
                checked += 1;
            }
        }
    }
    assert!(checked > 5, "checked {checked} accounts");
    let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products_services WHERE is_active = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(run(&pool, "item-listing").await.report.line_count as i64, items);
}

#[tokio::test]
async fn audit_trail_lists_the_chain() {
    let pool = company().await;
    let day: chrono::NaiveDate = sqlx::query_scalar("SELECT date(MAX(timestamp), 'localtime') FROM ledger_audit_log").fetch_one(&pool).await.unwrap();
    let report = list_reports::run(&pool, "audit-trail", Some(day), Some(day), day).await.unwrap();
    let written: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE date(timestamp, 'localtime') = ?1").bind(day).fetch_one(&pool).await.unwrap();
    assert_eq!(report.report.line_count as i64, written);
    let action = report.report.columns.iter().position(|c| c.label == "Action").unwrap();
    let posted: Vec<&ReportRow> = details(&report).filter(|r| text(&r.cells[action]) == "Posted").collect();
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries WHERE status = 'POSTED'").fetch_one(&pool).await.unwrap();
    assert_eq!(posted.len() as i64, entries, "one record per posted entry");
    assert!(posted.iter().all(|r| r.journal_entry_id.is_some()), "posted rows open their entry");
    assert!(details(&report).any(|r| text(&r.cells[action]) == "Voided"));
    assert!(report.notes.iter().any(|n| n.contains("checks out")), "{:?}", report.notes);
    // A day with nothing written has no rows.
    let quiet = list_reports::run(&pool, "audit-trail", Some(d(2001, 1, 1)), Some(d(2001, 1, 2)), day).await.unwrap();
    assert_eq!(quiet.report.line_count, 0);
}

#[tokio::test]
async fn several_reports_make_one_workbook() {
    let pool = company().await;
    let a = run(&pool, "customer-phone").await;
    let b = run(&pool, "vendor-phone").await;
    let sheet = |title: &str, r: &AccountantReport| -> crate::xlsx_export::Sheet {
        let rows: Vec<Value> = r.report.rows.iter().map(|x| serde_json::json!({ "kind": x.kind, "cells": x.cells })).collect();
        let columns: Vec<Value> = r.report.columns.iter().map(|c| serde_json::json!({ "label": c.label })).collect();
        serde_json::from_value(serde_json::json!({ "company": "Harbor Lane", "title": title, "columns": columns, "rows": rows })).unwrap()
    };
    // Two reports with the same title still get worksheets of their own.
    let book = crate::xlsx_export::to_xlsx_book(&[sheet("Phone List", &a), sheet("Phone List", &b)], "$").unwrap();
    assert!(book.starts_with(b"PK"));
    let long = "A report title much longer than thirty-one characters";
    assert_eq!(crate::xlsx_export::sheet_names(["Phone List", "phone list", "Phone List", long, long].into_iter()), vec![
        "Phone List".to_string(),
        "phone list (2)".into(),
        "Phone List (3)".into(),
        "A report title much longer than".into(),
        "A report title much longer (2)".into(),
    ]);
    assert!(crate::xlsx_export::to_xlsx_book(&[], "$").is_err());
}
