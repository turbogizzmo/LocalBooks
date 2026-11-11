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
use crate::money::Cents;

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
    let patio: i64 = sqlx::query_scalar("INSERT INTO customers (name, parent_id, phone) VALUES ('Brightwater Cafe:Patio Menu', ?1, '555-0143') RETURNING id").bind(cafe).fetch_one(p).await.unwrap();
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
        customer_id: None,
    };
    crate::workflows::create_purchase_order(
        p,
        crate::workflows::PurchaseOrderInput { form_lines: Vec::new(),
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
    let mugs = crate::sales::create_product(p, crate::sales::ProductInput { name: "Gift Mugs".into(), description: Some("Logo mugs".into()), rate_cents: Cents(1_800), income_account_id: sales, is_taxable: true })
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
        BillInput { form_lines: Vec::new(),
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

    // For the T36 reports: a budget, two to-dos, a memorized transaction and a ship-to address.
    let budget = crate::statements::create_budget(p, "Plan 2025", d(2025, 6, 1)).await.unwrap().id;
    let rent = account(p, "6300").await;
    let amounts: Vec<crate::statements::BudgetAmount> = (1..=12)
        .map(|period| crate::statements::BudgetAmount { account_id: sales, period, amount_cents: 1_000_000 })
        .chain((1..=12).map(|period| crate::statements::BudgetAmount { account_id: rent, period, amount_cents: 250_000 }))
        .collect();
    crate::statements::set_budget_amounts(p, budget, &amounts).await.unwrap();
    sqlx::query("INSERT INTO todos (title, note, due_date, customer_id) VALUES ('Call about the patio menu', 'Ask for the final copy', '2025-12-20', ?1)").bind(cafe).execute(p).await.unwrap();
    sqlx::query("INSERT INTO todos (title, is_done, done_at) VALUES ('Order mug stock', 1, '2025-12-01T10:00:00.000Z')").execute(p).await.unwrap();
    sqlx::query("INSERT INTO recurring_templates (name, kind, payload, frequency, every, next_date) VALUES ('Monthly rent', 'EXPENSE', '{}', 'MONTHLY', 1, '2026-01-01')").execute(p).await.unwrap();
    sqlx::query("UPDATE invoices SET ship_to = '8 Pier Road
Harbor Lane, OR 97001' WHERE id = (SELECT id FROM invoices WHERE customer_id = ?1 AND is_voided = 0 ORDER BY id LIMIT 1)")
        .bind(cafe)
        .execute(p)
        .await
        .unwrap();
    // A sales order for twenty mugs, more than are on hand, and a logo package: open for the sales
    // order reports, with part of it backordered.
    let so_line = |item, description: &str, qty: i64, rate: i64| crate::sales_orders::SalesOrderLineInput {
        id: None,
        line: crate::sales::SalesLineInput { product_service_id: Some(item), description: description.into(), quantity_e4: qty, rate_cents: rate, income_account_id: sales, class_id: None, is_taxable: false, service_date: None },
    };
    crate::sales_orders::create(
        p,
        &crate::sales_orders::SalesOrderInput {
            order_number: None,
            customer_name: "Brightwater Cafe".into(),
            order_date: d(2025, 12, 8),
            expected_date: Some(d(2026, 1, 15)),
            memo: Some("Holiday mugs".into()),
            lines: vec![so_line(mugs, "Logo mugs", 200_000, 1_800), so_line(logo, "Logo refresh", 10_000, 45_000)],
            tax_rate_id: None,
            sales_rep_id: None,
            ship_method_id: None,
            customer_message_id: None,
            header: Default::default(),
        },
    )
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
    // The fixture's books have no other names; one gives the Other Names Phone List a line.
    let owner = crate::other_names::OtherNameInput { name: "Dana Hart".into(), phone: Some("555-0142".into()), ..Default::default() };
    crate::other_names::create(&pool, &owner).await.unwrap();
    for info in list_reports::catalog() {
        let report = run(&pool, info.id).await;
        assert!(excel(info.title, &report.report).starts_with(b"PK"), "{} exports to Excel", info.id);
        if info.id == "audit-trail" {
            continue;
        }
        // The fixture closes no period, marks no cost billable and has no home currency adjustment
        // (one would change every other report here); `t9_…` and `t36_reports_tie_out` fill those.
        if !matches!(info.id, "closing-date-exceptions" | "unbilled-costs-job" | "unrealized-gains") {
            assert!(report.report.line_count > 0, "{} has lines", info.id);
        }
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

#[tokio::test]
async fn t36_reports_tie_out() {
    let pool = company().await;
    let p = &pool;
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    let run_on = |id: &'static str| {
        let p = p.clone();
        async move { list_reports::run(&p, id, Some(start), Some(end), today()).await.unwrap_or_else(|e| panic!("{id}: {e}")) }
    };

    // Sales Tax Revenue Summary: the Sales Tax Liability report's sales, taxable plus non-taxable.
    let revenue = run_on("sales-tax-revenue").await;
    let liability = crate::sales_tax::tax_liability(p, start, end).await.unwrap();
    let taxable: i64 = liability.rows.iter().map(|r| r.taxable_sales_cents).sum();
    let other: i64 = liability.rows.iter().map(|r| r.non_taxable_sales_cents).sum();
    let totals: Vec<i64> = total(&revenue).cells[1..].iter().map(cents).collect();
    assert_eq!(totals, vec![taxable + other, other, taxable]);

    // Sales by Ship To Address: every invoice and sales receipt's income, grouped; the one with an
    // address is under it.
    let ship = run_on("sales-ship-to").await;
    let income: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0) FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id JOIN v_chart_of_accounts a ON a.id = l.account_id
         WHERE a.classification = 'INCOME' AND e.status = 'POSTED' AND e.entry_date BETWEEN '2025-01-01' AND '2025-12-31'
           AND e.id IN (SELECT journal_entry_id FROM invoices WHERE is_voided = 0 UNION SELECT journal_entry_id FROM sales_receipts WHERE is_voided = 0)",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(last(total(&ship)), income);
    assert!(ship.report.rows.iter().any(|r| r.kind == RowKind::Section && text(&r.cells[0]) == "8 Pier Road, Harbor Lane, OR 97001"));

    // A home currency adjustment at the end of November on the euro invoice still open.
    let rates: std::collections::BTreeMap<String, i64> = [("EUR".to_string(), 1_120_000)].into();
    crate::revaluation::post(p, d(2025, 11, 30), &rates).await.unwrap();
    let adjustments = run_on("unrealized-gains").await;

    // Each ties out to what its account shows in the ledger for the range.
    let posted = |name: &'static str| {
        let p = p.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0) FROM journal_lines l
                 JOIN journal_entries e ON e.id = l.journal_entry_id JOIN chart_of_accounts a ON a.id = l.account_id
                 WHERE a.name = ?1 AND a.parent_id IS NULL AND e.status = 'POSTED' AND e.entry_date BETWEEN '2025-01-01' AND '2025-12-31'",
            )
            .bind(name)
            .fetch_one(&p)
            .await
            .unwrap()
        }
    };
    assert_eq!(last(total(&run_on("realized-gains").await)), posted("Exchange Gain or Loss").await);
    assert_eq!(last(total(&adjustments)), posted("Unrealized Exchange Gain or Loss").await);
    assert_eq!(adjustments.report.line_count, 2, "the November 30 adjustment and its December 1 reversal, which net to zero over the year");

    // Budget Overview: the amounts as entered, and net income = income less expenses.
    let budget = list_reports::run(p, "budget-overview", None, Some(d(2025, 6, 30)), today()).await.unwrap();
    let net = total(&budget);
    assert_eq!(last(net), 12 * (1_000_000 - 250_000));
    assert_eq!(details(&budget).count(), 2);

    // To Do Notes and the memorized transaction listing list what was entered.
    let todos = run_on("todo-notes").await;
    assert_eq!(text(&todos.report.rows[0].cells[2]), "Call about the patio menu", "open to-dos first");
    assert_eq!(text(&todos.report.rows[0].cells[4]), "Brightwater Cafe");
    let memorized = run_on("memorized-transactions").await;
    assert!(memorized.report.rows.iter().any(|r| text(&r.cells[0]) == "Monthly rent" && text(&r.cells[2]) == "Every month"));
}

#[tokio::test]
async fn t49_reports_tie_out_to_the_ledger() {
    let pool = company().await;
    let p = &pool;
    // Four of the twelve mugs sold to the cafe in December, so an invoice carries cost of goods.
    let mugs: i64 = sqlx::query_scalar("SELECT id FROM products_services WHERE name = 'Gift Mugs'").fetch_one(p).await.unwrap();
    let sales = account(p, "4000").await;
    crate::invoices::create_and_post_invoice(
        p,
        crate::invoices::InvoiceInput {
            invoice_number: None,
            customer_name: "Brightwater Cafe".into(),
            issue_date: d(2025, 12, 15),
            due_date: d(2025, 12, 30),
            lines: vec![crate::invoices::InvoiceLineInput {
                description: "Gift mugs".into(),
                quantity_e4: 40_000,
                rate_cents: Cents(1_800),
                income_account_id: sales,
                product_service_id: Some(mugs),
                class_id: None,
                is_taxable: false,
                service_date: None,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: Cents::ZERO,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    let run_on = |id: &'static str| {
        let p = p.clone();
        async move { list_reports::run(&p, id, Some(start), Some(end), today()).await.unwrap_or_else(|e| panic!("{id}: {e}")) }
    };
    let ledger = |classification_or_type: &'static str, column: &'static str| {
        let p = p.clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COALESCE(SUM({}), 0) FROM journal_lines l
                 JOIN journal_entries e ON e.id = l.journal_entry_id JOIN v_chart_of_accounts a ON a.id = l.account_id
                 WHERE a.{column} = ?1 AND e.status = 'POSTED' AND e.entry_date BETWEEN '2025-01-01' AND '2025-12-31'
                   AND e.id IN (SELECT journal_entry_id FROM invoices WHERE is_voided = 0)",
                if column == "classification" { "l.credit_cents - l.debit_cents" } else { "l.debit_cents - l.credit_cents" }
            ))
            .bind(classification_or_type)
            .fetch_one(&p)
            .await
            .unwrap()
        }
    };
    let income = ledger("INCOME", "classification").await;
    let cogs = ledger("COST_OF_GOODS_SOLD", "account_type").await;
    assert_eq!(cogs, 3_600, "four mugs at their $9.00 average cost");

    // Invoice Profitability by Customer and Detail: the invoices' income and cost of goods, as posted.
    let by_customer = run_on("invoice-profit-customer").await;
    let t = total(&by_customer);
    assert_eq!((cents(&t.cells[2]), cents(&t.cells[3]), cents(&t.cells[4])), (income, cogs, income - cogs));
    let detail = run_on("invoice-profit-detail").await;
    let t = total(&detail);
    assert_eq!((cents(&t.cells[2]), cents(&t.cells[3])), (income, cogs));
    assert_eq!(details(&detail).count() as i64, sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM invoices i JOIN journal_entries e ON e.id = i.journal_entry_id WHERE i.is_voided = 0 AND e.entry_date BETWEEN '2025-01-01' AND '2025-12-31'").fetch_one(p).await.unwrap());

    // Inventory Turnover: ending values are Inventory Valuation's; its cost of goods sold is what the
    // stock ledger took out for sales.
    let turnover = run_on("inventory-turnover").await;
    let valuation = crate::report_center::inventory_valuation(p, end).await.unwrap();
    assert_eq!(cents(&total(&turnover).cells[2]), valuation.total_asset_value_cents);
    let sold: i64 = sqlx::query_scalar(
        "SELECT -COALESCE(SUM(s.value_change_cents), 0) FROM stock_ledger_entries s LEFT JOIN stock_ledger_entries r ON r.id = s.reverses_id
         WHERE s.movement_date BETWEEN '2025-01-01' AND '2025-12-31' AND (s.kind = 'SALE' OR (s.kind = 'VOID' AND r.kind = 'SALE'))",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!(cents(&total(&turnover).cells[4]), sold);
    assert_eq!(sold, cogs, "the stock ledger and the invoices' cost of goods agree");
    // Mugs: nothing at the start, $72.00 at the end, $36.00 on average, $36.00 sold: turned over once.
    let mug_row = details(&turnover).find(|r| text(&r.cells[0]) == "Gift Mugs").unwrap();
    assert_eq!((text(&mug_row.cells[5]), text(&mug_row.cells[6])), ("1.00".to_string(), "365".to_string()));
}

/// A job on purchase order lines (gap T9): Open Purchase Orders by Job groups the same open lines
/// Open Purchase Orders Detail lists, the job goes onto the bill, and a customer merge moves it.
#[tokio::test]
async fn open_purchase_orders_by_job_tie_out_and_the_job_reaches_the_bill() {
    let pool = company().await;
    let p = &pool;
    let office = account(p, "6250").await;
    let job = |name: &'static str| {
        let p = p.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM customers WHERE name = ?1").bind(name).fetch_one(&p).await.unwrap() }
    };
    let (cafe, patio) = (job("Brightwater Cafe").await, job("Brightwater Cafe:Patio Menu").await);
    let line = |amount_cents, description: &str, customer_id| crate::workflows::PurchaseOrderLineInput {
        account_id: office,
        product_service_id: None,
        description: Some(description.into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents,
        class_id: None,
        customer_id,
    };
    let po = crate::workflows::create_purchase_order(
        p,
        crate::workflows::PurchaseOrderInput {
            form_lines: Vec::new(),
            vendor_name: "Riverside Print Shop".into(),
            po_date: d(2025, 12, 9),
            expected_date: None,
            memo: None,
            lines: vec![line(4_000, "Menu boards", Some(cafe)), line(1_500, "Patio banner", Some(patio)), line(700, "Shop supplies", None)],
        },
    )
    .await
    .unwrap();

    let by_job = run(p, "open-po-job").await;
    let detail = run(p, "open-po-detail").await;
    assert_eq!(last(total(&by_job)), last(total(&detail)), "the same open lines, grouped by job");
    let jobs: Vec<String> = by_job.report.rows.iter().filter(|r| r.kind == RowKind::Section).map(|r| text(&r.cells[0])).collect();
    assert_eq!(jobs, ["Brightwater Cafe", "Brightwater Cafe:Patio Menu", "No customer or job"]);
    let subtotals: Vec<i64> = by_job.report.rows.iter().filter(|r| r.kind == RowKind::Subtotal).map(last).collect();
    assert_eq!(subtotals[..2], [4_000, 1_500]);

    // The bill made from the order keeps each line's job.
    let bill = crate::workflows::bill_purchase_order(p, po, d(2025, 12, 12), d(2025, 12, 26), None).await.unwrap();
    let on_bill: Vec<Option<i64>> = sqlx::query_scalar("SELECT customer_id FROM bill_lines WHERE bill_id = ?1 ORDER BY line_number").bind(bill).fetch_all(p).await.unwrap();
    assert_eq!(on_bill, [Some(cafe), Some(patio), None]);

    // A customer entered twice: merging it into the cafe moves its purchase order lines too.
    let twice: i64 = sqlx::query_scalar("INSERT INTO customers (name) VALUES ('Brightwater Cafe Co') RETURNING id").fetch_one(p).await.unwrap();
    crate::workflows::create_purchase_order(
        p,
        crate::workflows::PurchaseOrderInput { form_lines: Vec::new(), vendor_name: "Riverside Print Shop".into(), po_date: d(2025, 12, 10), expected_date: None, memo: None, lines: vec![line(900, "Table cards", Some(twice))] },
    )
    .await
    .unwrap();
    crate::merges::merge(p, crate::merges::MergeKind::Customer, twice, cafe, d(2025, 12, 31)).await.unwrap_or_else(|e| panic!("merge: {e}"));
    let moved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM purchase_order_lines WHERE customer_id = ?1 AND description = 'Table cards'").bind(cafe).fetch_one(p).await.unwrap();
    assert_eq!(moved, 1);
}

/// Gap T9's last reports: each total or quantity is the one in the records it reads.
#[tokio::test]
async fn t9_terms_unbilled_stock_and_closing_exceptions_tie_out() {
    let pool = company().await;
    let p = &pool;

    // Terms Listing: every term in the Terms List, one row each.
    let terms = run(p, "terms-listing").await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM terms").fetch_one(p).await.unwrap();
    assert_eq!(details(&terms).count() as i64, count);

    // Unbilled Costs by Job: the billable bill and expense lines not yet invoiced, by job, with a
    // total equal to those lines as posted to their expense accounts.
    // A bill for the cafe and its patio job, its lines marked billable.
    let office = account(p, "6250").await;
    let job = |name: &'static str| {
        let p = p.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM customers WHERE name = ?1").bind(name).fetch_one(&p).await.unwrap() }
    };
    let (cafe, patio) = (job("Brightwater Cafe").await, job("Brightwater Cafe:Patio Menu").await);
    let billable = |amount_cents, memo: &str, customer_id| BillLineInput {
        account_id: office,
        amount_cents,
        memo: Some(memo.into()),
        quantity_e4: None,
        rate_cents: None,
        product_service_id: None,
        class_id: None,
        customer_id: Some(customer_id),
        billable: Some(true),
    };
    crate::expenses::post_vendor_bill(
        p,
        BillInput {
            form_lines: Vec::new(),
            terms_id: None,
            vendor_name: "Riverside Print Shop".into(),
            bill_number: Some("RP-77".into()),
            bill_date: d(2025, 12, 8),
            due_date: d(2025, 12, 22),
            memo: None,
            total_cents: 5_000,
            lines: vec![billable(3_000, "Menu card stock", cafe), billable(2_000, "Patio sign", patio)],
        },
    )
    .await
    .unwrap();
    let unbilled = run(p, "unbilled-costs-job").await;
    let posted: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(amount_cents), 0) FROM bill_lines WHERE is_billable = 1").fetch_one(p).await.unwrap();
    assert_eq!(posted, 5_000, "the cafe's menu card stock and its patio job's sign");
    assert_eq!(last(total(&unbilled)), posted);
    let jobs: Vec<String> = unbilled.report.rows.iter().filter(|r| r.kind == RowKind::Section).map(|r| text(&r.cells[0])).collect();
    assert_eq!(jobs, ["Brightwater Cafe", "Brightwater Cafe:Patio Menu"]);
    let subtotals: Vec<i64> = unbilled.report.rows.iter().filter(|r| r.kind == RowKind::Subtotal).map(last).collect();
    assert_eq!(subtotals, [3_000, 2_000]);

    // Inventory Stock Status by Vendor: on hand as the stock ledger has it, under the preferred
    // vendor, flagged to order at or below the reorder point.
    sqlx::query("UPDATE products_services SET preferred_vendor_id = (SELECT id FROM vendors WHERE name = 'Lakeside Ceramics'), reorder_point_e4 = 150000 WHERE name = 'Gift Mugs'")
        .execute(p)
        .await
        .unwrap();
    let stock = run(p, "stock-status-vendor").await;
    let mugs = details(&stock).find(|r| text(&r.cells[1]) == "Gift Mugs").expect("the mugs");
    let on_hand: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(s.qty_change_e4), 0) FROM stock_ledger_entries s JOIN inventory_items i ON i.id = s.item_id JOIN products_services p ON p.id = i.product_id WHERE p.name = 'Gift Mugs'",
    )
    .fetch_one(p)
    .await
    .unwrap();
    assert_eq!((cents(&mugs.cells[4]), on_hand), (120_000, 120_000), "twelve mugs");
    assert_eq!(text(&mugs.cells[5]), "Order", "12 on hand is at or below the reorder point of 15");
    let vendor = stock.report.rows.iter().find(|r| r.kind == RowKind::Section).map(|r| text(&r.cells[0]));
    assert_eq!(vendor.as_deref(), Some("Lakeside Ceramics"));

    // Closing Date Exceptions: none while the books stay closed; an entry made while the closing
    // date was moved back is one, for the amount it posted.
    let checking = account(p, "1010").await;
    let close = |date: Option<chrono::NaiveDate>| {
        let p = p.clone();
        async move { crate::closing::set_closing_date(&p, &crate::closing::ClosingInput { lock_date: date, current_passcode: None, new_passcode: None }).await.unwrap() }
    };
    close(Some(d(2025, 11, 30))).await;
    let none = list_reports::run(p, "closing-date-exceptions", Some(d(2025, 1, 1)), Some(d(2025, 12, 31)), today()).await.unwrap();
    assert_eq!(none.report.line_count, 0);
    close(None).await;
    let line = |account_id, debit, credit| crate::ledger::JournalLine { account_id, debit: crate::money::Cents(debit), credit: crate::money::Cents(credit), memo: None, class_id: None };
    let late = crate::ledger::post_journal_entry(
        p,
        crate::ledger::JournalEntry { id: None, date: d(2025, 11, 20), description: "Late office supplies".into(), lines: vec![line(office, 4_200, 0), line(checking, 0, 4_200)], currency: None },
    )
    .await
    .unwrap();
    close(Some(d(2025, 11, 30))).await;
    let exceptions = list_reports::run(p, "closing-date-exceptions", Some(d(2025, 1, 1)), Some(d(2025, 12, 31)), today()).await.unwrap();
    let rows: Vec<&ReportRow> = details(&exceptions).collect();
    assert_eq!(rows.len(), 1, "{:?}", exceptions.notes);
    assert_eq!(rows[0].journal_entry_id, Some(late));
    let debits: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = ?1").bind(late).fetch_one(p).await.unwrap();
    assert_eq!((last(rows[0]), last(total(&exceptions))), (debits, 4_200));
}
