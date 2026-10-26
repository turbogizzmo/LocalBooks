//! The custom report engine on the fixture company: every preset as a golden snapshot, and checks
//! that the engine agrees exactly with the existing reports where they overlap (General Ledger,
//! A/R and A/P aging, the sales and expense summaries, the 1099 summary).
//!
//! Harbor Lane has no products, classes or reps, so `company()` adds a little of each in 2025:
//! product sales with a class and a rep, a credit memo and a vendor credit (part used), and a bill
//! for products.

use std::collections::BTreeMap;
use std::fmt::Write;

use chrono::NaiveDate;
use serde_json::Value;
use sqlx::SqlitePool;

use super::account;
use super::fixture::{d, harbor_lane};
use crate::custom_reports::{self, Column, ColumnKind, CustomReport, Filters, GroupBy, Mode, ReportRow, ReportSettings, RowKind, Source};
use crate::credits::{ApplyCreditInput, CreditMemoInput, VendorCreditInput, VendorCreditLineInput};
use crate::expenses::{BillInput, BillLineInput};
use crate::invoices::{InvoiceInput, InvoiceLineInput};
use crate::sales::SalesLineInput;

/// The date the reports run as of (stands in for "today").
const TODAY: (i32, u32, u32) = (2025, 12, 31);

fn today() -> NaiveDate {
    d(TODAY.0, TODAY.1, TODAY.2)
}

async fn company() -> SqlitePool {
    let pool = harbor_lane().await;
    let p = &pool;
    let sales = account(p, "4000").await;
    let office = account(p, "6250").await;
    let studio = crate::classes::create_class(p, crate::classes::ClassInput { name: "Studio".into(), parent_id: None }).await.expect("class").id;
    let rep = crate::sales_lists::create_sales_rep(p, crate::sales_lists::SalesRepInput { initials: "MR".into(), name: Some("Mara Reyes".into()) }).await.expect("rep").id;
    let product = |name: &str, rate: i64| crate::sales::ProductInput { name: name.into(), description: None, rate_cents: rate, income_account_id: sales, is_taxable: false };
    let logo = crate::sales::create_product(p, product("Logo Package", 120_000)).await.expect("product").id;
    let prints = crate::sales::create_product(p, product("Poster Prints", 2_500)).await.expect("product").id;

    let line = |description: &str, item: i64, qty: i64, rate: i64, class: Option<i64>| InvoiceLineInput {
        description: description.into(),
        quantity_e4: qty * 10_000,
        rate_cents: rate,
        income_account_id: sales,
        product_service_id: Some(item),
        class_id: class,
        is_taxable: false,
    };
    let details = crate::sales_lists::SalesDetails { sales_rep_id: Some(rep), ..Default::default() };
    let invoice = crate::invoices::create_and_post_invoice(
        p,
        InvoiceInput {
            customer_name: "Brightwater Cafe".into(),
            issue_date: d(2025, 11, 14),
            due_date: d(2025, 12, 14),
            lines: vec![line("Logo refresh", logo, 1, 120_000, Some(studio)), line("Menu posters", prints, 40, 2_500, Some(studio))],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details,
        },
    )
    .await
    .expect("product invoice")
    .id;
    let memo = crate::credits::create_credit_memo(
        p,
        CreditMemoInput {
            customer_name: "Brightwater Cafe".into(),
            credit_date: d(2025, 11, 20),
            memo: Some("Misprinted posters".into()),
            refund_account_id: None,
            lines: vec![SalesLineInput { product_service_id: Some(prints), description: "Misprinted posters".into(), quantity_e4: 60_000, rate_cents: 2_500, income_account_id: sales, class_id: Some(studio), is_taxable: false }],
            tax_rate_id: None,
        },
    )
    .await
    .expect("credit memo")
    .id;
    crate::credits::apply_customer_credit(p, ApplyCreditInput { credit_id: memo, document_id: invoice, amount_cents: 10_000, applied_date: d(2025, 11, 21) }).await.expect("apply credit");

    let bill = crate::expenses::post_vendor_bill(
        p,
        BillInput {
            vendor_name: "Riverside Print Shop".into(),
            bill_number: Some("RP-77".into()),
            bill_date: d(2025, 11, 10),
            due_date: d(2025, 12, 10),
            memo: Some("Poster stock".into()),
            total_cents: 45_000,
            lines: vec![
                BillLineInput { account_id: office, amount_cents: 40_000, memo: Some("Poster stock".into()), quantity_e4: Some(5_000_000), rate_cents: Some(80), product_service_id: Some(prints), class_id: Some(studio) },
                BillLineInput { account_id: office, amount_cents: 5_000, memo: Some("Delivery".into()), quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None },
            ],
        },
    )
    .await
    .expect("product bill")
    .id;
    let credit = crate::credits::create_vendor_credit(
        p,
        VendorCreditInput {
            vendor_name: "Riverside Print Shop".into(),
            reference: Some("RP-C1".into()),
            credit_date: d(2025, 11, 25),
            memo: Some("Damaged stock".into()),
            lines: vec![VendorCreditLineInput { account_id: office, amount_cents: 6_000, memo: Some("Damaged stock".into()), class_id: Some(studio) }],
        },
    )
    .await
    .expect("vendor credit")
    .id;
    crate::credits::apply_vendor_credit(p, ApplyCreditInput { credit_id: credit, document_id: bill, amount_cents: 2_500, applied_date: d(2025, 11, 26) }).await.expect("apply vendor credit");
    pool
}

async fn run(pool: &SqlitePool, settings: &ReportSettings) -> CustomReport {
    custom_reports::run(pool, settings, today()).await.expect("report runs")
}

fn year(mut settings: ReportSettings) -> ReportSettings {
    settings.start_date = Some(d(2025, 1, 1));
    settings.end_date = Some(d(2025, 12, 31));
    settings
}

fn cents(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn text(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_default()
}

/// Detail rows with their journal entry and named cells.
fn details(report: &CustomReport) -> impl Iterator<Item = &ReportRow> {
    report.rows.iter().filter(|r| r.kind == RowKind::Detail && r.journal_entry_id.is_some())
}

fn column(report: &CustomReport, label: &str) -> usize {
    report.columns.iter().position(|c| c.label == label).unwrap_or_else(|| panic!("column {label}"))
}

// ------------------------------------------------------------------------------------------------
// Agreement with the existing reports
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn general_ledger_agrees() {
    let pool = company().await;
    for (start, end) in [(d(2025, 1, 1), d(2025, 12, 31)), (d(2025, 4, 1), d(2025, 6, 30)), (d(2026, 1, 1), d(2026, 1, 31))] {
        let gl = crate::report_center::general_ledger(&pool, start, end).await.unwrap();
        let settings = ReportSettings {
            start_date: Some(start),
            end_date: Some(end),
            columns: vec![Column::Debit, Column::Credit, Column::Balance],
            group_by: Some(GroupBy::Account),
            opening_balances: true,
            ..ReportSettings::default()
        };
        let report = run(&pool, &settings).await;
        // Walk the engine's rows account by account.
        let mut accounts = Vec::new();
        let mut rows = report.rows.iter().peekable();
        while let Some(heading) = rows.next() {
            if heading.kind == RowKind::Total {
                break;
            }
            assert_eq!(heading.kind, RowKind::Section);
            let opening = rows.next().expect("beginning balance");
            assert_eq!(text(&opening.cells[0]), "Beginning balance");
            let mut lines = Vec::new();
            while rows.peek().is_some_and(|r| r.kind == RowKind::Detail) {
                let r = rows.next().unwrap();
                lines.push((r.journal_entry_id.unwrap(), cents(&r.cells[1]), cents(&r.cells[2]), cents(&r.cells[3])));
            }
            let total = rows.next().expect("account total");
            accounts.push((heading.account_id.unwrap(), cents(&opening.cells[3]), lines, cents(&total.cells[1]), cents(&total.cells[2]), cents(&total.cells[3])));
        }
        let expected: Vec<_> = gl
            .accounts
            .iter()
            .map(|a| {
                let lines = a.rows.iter().map(|r| (r.journal_entry_id, r.debit_cents, r.credit_cents, r.balance_cents)).collect::<Vec<_>>();
                (a.account_id, a.opening_balance_cents, lines, a.total_debit_cents, a.total_credit_cents, a.ending_balance_cents)
            })
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(accounts, expected, "General Ledger {start} to {end}");
    }
}

#[tokio::test]
async fn aging_detail_agrees() {
    let pool = company().await;
    let presets = custom_reports::presets();
    for (payable, preset) in [(false, "ar-aging-detail"), (true, "ap-aging-detail"), (false, "open-invoices"), (true, "unpaid-bills")] {
        for as_of in [d(2025, 6, 30), d(2025, 11, 22), today()] {
            let aging = crate::report_center::aging(&pool, payable, as_of).await.unwrap();
            let mut settings = presets.iter().find(|p| p.id == preset).unwrap().settings.clone();
            settings.end_date = Some(as_of);
            let report = run(&pool, &settings).await;
            let (num, open, aging_col) = (column(&report, "Num"), column(&report, "Open Balance"), column(&report, "Aging"));
            let mut got: Vec<(String, i64, i64)> = details(&report).map(|r| (text(&r.cells[num]), cents(&r.cells[open]), cents(&r.cells[aging_col]))).collect();
            let mut want: Vec<(String, i64, i64)> = aging.items.iter().map(|i| (i.number.clone().unwrap_or_default(), i.open_cents, i.days_past_due.max(0))).collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "{preset} as of {as_of}");
            let total = report.rows.last().unwrap();
            assert_eq!(cents(&total.cells[open]), aging.total_cents, "{preset} total as of {as_of}");
        }
    }
}

/// Label → amount of a summary report (and quantity, when it has one).
fn summary_rows(report: &CustomReport) -> BTreeMap<String, (i64, Option<i64>)> {
    let qty = report.columns.iter().position(|c| c.kind == ColumnKind::Quantity);
    report
        .rows
        .iter()
        .filter(|r| r.kind == RowKind::Detail)
        .map(|r| (text(&r.cells[0]), (cents(r.cells.last().unwrap()), qty.map(|q| cents(&r.cells[q])))))
        .collect()
}

#[tokio::test]
async fn sales_and_expense_summaries_agree() {
    let pool = company().await;
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    let summary = |source, group_by, filters, show_qty| ReportSettings {
        source,
        mode: Mode::Summary,
        group_by: Some(group_by),
        filters,
        show_qty,
        start_date: Some(start),
        end_date: Some(end),
        ..ReportSettings::default()
    };
    let classes = |c: &str| Filters { classifications: vec![c.into()], ..Filters::default() };
    let types = |t: &[&str]| Filters { transaction_types: t.iter().map(|s| s.to_string()).collect(), ..Filters::default() };
    let cases = [
        (crate::report_center::by_party(&pool, false, start, end).await.unwrap(), summary(Source::Ledger, GroupBy::Name, classes("INCOME"), false), "No customer"),
        (crate::report_center::by_party(&pool, true, start, end).await.unwrap(), summary(Source::Ledger, GroupBy::Name, classes("EXPENSE"), false), "No vendor"),
        (crate::report_center::sales_by_product(&pool, start, end).await.unwrap(), summary(Source::Items, GroupBy::Item, types(&["INVOICE", "SALES_RECEIPT", "CREDIT_MEMO"]), true), "No product/service"),
        (crate::report_center::sales_by_rep(&pool, start, end).await.unwrap(), summary(Source::Items, GroupBy::Rep, types(&["INVOICE", "SALES_RECEIPT"]), true), "No rep"),
    ];
    for (existing, settings, none) in cases {
        let report = run(&pool, &settings).await;
        let engine_none = match settings.group_by {
            Some(GroupBy::Name) => "No name",
            Some(GroupBy::Item) => "No product/service",
            _ => "No rep",
        };
        let got: Vec<(String, i64, Option<i64>)> = report
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Detail)
            .map(|r| {
                let label = text(&r.cells[0]);
                let label = if label == engine_none { none.to_string() } else { label };
                let qty = settings.show_qty.then(|| cents(&r.cells[1]));
                (label, cents(r.cells.last().unwrap()), qty)
            })
            .collect();
        let want: Vec<(String, i64, Option<i64>)> = existing.rows.iter().map(|r| (r.label.clone(), r.amount_cents, r.quantity_e4)).collect();
        assert!(!want.is_empty());
        assert_eq!(got, want, "{:?} by {:?}", settings.source, settings.group_by);
        assert_eq!(cents(report.rows.last().unwrap().cells.last().unwrap()), existing.total_cents);
    }
}

#[tokio::test]
async fn vendor_1099_detail_agrees() {
    let pool = company().await;
    let rows = crate::workflows::vendors_1099(&pool, 2025, crate::tax_ids::Access::OPEN).await.unwrap();
    let preset = custom_reports::presets().into_iter().find(|p| p.id == "1099-detail").unwrap();
    let mut settings = year(preset.settings);
    settings.mode = Mode::Summary;
    let report = run(&pool, &settings).await;
    let got = summary_rows(&report);
    let want: BTreeMap<String, (i64, Option<i64>)> = rows.iter().filter(|r| r.paid_cents != 0).map(|r| (r.vendor_name.clone(), (r.paid_cents, None))).collect();
    assert!(!want.is_empty());
    assert_eq!(got, want);
}

#[tokio::test]
async fn customer_balances_agree_with_receivables() {
    let pool = company().await;
    let preset = custom_reports::presets().into_iter().find(|p| p.id == "customer-balance-summary").unwrap();
    let mut settings = preset.settings;
    settings.end_date = Some(today());
    let report = run(&pool, &settings).await;
    let aging = crate::report_center::aging(&pool, false, today()).await.unwrap();
    let got = summary_rows(&report);
    for row in &aging.rows {
        assert_eq!(got.get(&row.party_name).map(|v| v.0), Some(row.total_cents), "{}", row.party_name);
    }
    let receivable: i64 = sqlx::query_scalar("SELECT SUM(balance_cents) FROM v_account_balances WHERE account_type = 'ACCOUNTS_RECEIVABLE'").fetch_one(&pool).await.unwrap();
    assert_eq!(cents(report.rows.last().unwrap().cells.last().unwrap()), receivable);
}

// ------------------------------------------------------------------------------------------------
// Golden snapshots of every preset
// ------------------------------------------------------------------------------------------------

fn render_cell(kind: ColumnKind, v: &Value) -> String {
    match (kind, v) {
        (_, Value::Null) => String::new(),
        (ColumnKind::Money, v) => crate::ledger::format_money(cents(v), ""),
        (ColumnKind::Quantity, v) => {
            let q = cents(v);
            let (whole, frac) = (q / 10_000, (q % 10_000).abs());
            if frac == 0 { whole.to_string() } else { format!("{whole}.{:04}", frac).trim_end_matches('0').to_string() }
        }
        (_, Value::String(s)) => s.chars().take(24).collect(),
        (_, v) => v.to_string(),
    }
}

fn render(title: &str, report: &CustomReport) -> String {
    let mut out = format!("{title}, {} to {}\n\n", report.start_date.map_or("all dates".into(), |s| s.to_string()), report.end_date);
    let widths: Vec<usize> = report.columns.iter().enumerate().map(|(i, c)| if i == 0 { 30 } else if matches!(c.kind, ColumnKind::Text) { 25 } else { 13 }).collect();
    let pad = |s: &str, w: usize, right: bool| if right { format!("{s:>w$} ") } else { format!("{s:<w$} ") };
    let right = |k: ColumnKind| !matches!(k, ColumnKind::Text | ColumnKind::Date);
    let mut head = String::new();
    for (c, w) in report.columns.iter().zip(&widths) {
        head += &pad(&c.label, *w, right(c.kind));
    }
    writeln!(out, "{}", head.trim_end()).unwrap();
    for row in &report.rows {
        let mut line = String::new();
        for (i, ((c, w), v)) in report.columns.iter().zip(&widths).zip(&row.cells).enumerate() {
            let mut s = render_cell(c.kind, v);
            if i == 0 {
                s = format!("{}{s}", "  ".repeat(row.depth as usize));
            }
            line += &pad(&s, *w, i > 0 && right(c.kind));
        }
        writeln!(out, "{}", line.trim_end()).unwrap();
    }
    writeln!(out, "\n{} lines", report.line_count).unwrap();
    out
}

#[tokio::test]
async fn presets() {
    let pool = company().await;
    for preset in custom_reports::presets() {
        let mut settings = preset.settings.clone();
        match (preset.date_mode, preset.period) {
            (custom_reports::DateMode::AsOf, _) => settings.end_date = Some(today()),
            (_, "thisMonth") => (settings.start_date, settings.end_date) = (Some(d(2025, 11, 1)), Some(d(2025, 11, 30))),
            _ => settings = year(settings),
        }
        let report = run(&pool, &settings).await;
        assert!(report.line_count > 0, "{} has lines", preset.id);
        insta::assert_snapshot!(format!("preset_{}", preset.id.replace('-', "_")), render(preset.title, &report));
    }
}

// ------------------------------------------------------------------------------------------------
// Filters, layouts and drill-down on the fixture
// ------------------------------------------------------------------------------------------------

fn lines_report(filters: Filters) -> ReportSettings {
    year(ReportSettings { columns: vec![Column::Type, Column::Num, Column::Name, Column::Memo, Column::Account, Column::Class, Column::Item, Column::Amount], filters, ..ReportSettings::default() })
}

/// The given column of every detail row.
fn values(report: &CustomReport, label: &str) -> Vec<Value> {
    let c = column(report, label);
    details(report).map(|r| r.cells[c].clone()).collect()
}

#[tokio::test]
async fn filters_narrow_lines() {
    let pool = company().await;
    let all = run(&pool, &lines_report(Filters::default())).await;

    // Memo text, ignoring case.
    let rent = run(&pool, &lines_report(Filters { memo: Some("STUDIO RENT".into()), ..Filters::default() })).await;
    assert!(rent.line_count > 0 && rent.line_count < all.line_count);
    assert!(values(&rent, "Memo").iter().all(|m| text(m).to_lowercase().contains("studio rent")));

    // Amount range on the line's size, either side.
    let big = run(&pool, &lines_report(Filters { amount_min_cents: Some(1_000_000), ..Filters::default() })).await;
    assert!(values(&big, "Amount").iter().all(|a| cents(a).abs() >= 1_000_000));
    let band = run(&pool, &lines_report(Filters { amount_min_cents: Some(95_000), amount_max_cents: Some(95_000), ..Filters::default() })).await;
    assert_eq!(band.line_count, 12, "six $950 contractor checks, two lines each");

    // Class (with subclasses) and unclassified.
    let studio: i64 = sqlx::query_scalar("SELECT id FROM classes WHERE name = 'Studio'").fetch_one(&pool).await.unwrap();
    let classed = run(&pool, &lines_report(Filters { class_ids: vec![studio], ..Filters::default() })).await;
    assert!(classed.line_count > 0);
    assert!(values(&classed, "Class").iter().all(|c| text(c) == "Studio"));
    let unclassified = run(&pool, &lines_report(Filters { unclassified: true, ..Filters::default() })).await;
    assert_eq!(classed.line_count + unclassified.line_count, all.line_count);

    // A name, and every vendor.
    let cafe: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Brightwater Cafe'").fetch_one(&pool).await.unwrap();
    let named = run(&pool, &lines_report(Filters { names: vec![custom_reports::NameRef { kind: custom_reports::NameKind::Customer, id: cafe }], ..Filters::default() })).await;
    assert!(named.line_count > 0);
    assert!(values(&named, "Name").iter().all(|n| text(n) == "Brightwater Cafe"));
    let vendors = run(&pool, &lines_report(Filters { name_kind: Some(custom_reports::NameKind::Vendor), ..Filters::default() })).await;
    assert!(values(&vendors, "Type").iter().all(|t| ["Bill", "Bill Payment", "Check", "Expense", "Vendor Credit", "Credit Card Credit"].contains(&text(t).as_str())));

    // An item matches the ledger lines it was posted on.
    let prints: i64 = sqlx::query_scalar("SELECT id FROM products_services WHERE name = 'Poster Prints'").fetch_one(&pool).await.unwrap();
    let item = run(&pool, &lines_report(Filters { item_ids: vec![prints], ..Filters::default() })).await;
    let mut accounts: Vec<String> = values(&item, "Account").iter().map(text).collect();
    accounts.dedup();
    assert_eq!(accounts, ["Office Supplies", "Sales"], "the bill line and the invoice and credit memo lines");

    // Transaction types, and leaving voids out.
    let checks = run(&pool, &lines_report(Filters { transaction_types: vec!["CHECK".into()], ..Filters::default() })).await;
    assert!(values(&checks, "Type").iter().all(|t| text(t) == "Check"));
    let voided: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id WHERE e.status = 'POSTED' AND (e.is_voided = 1 OR e.reverses_entry_id IS NOT NULL)")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(voided > 0);
    let live = run(&pool, &lines_report(Filters { exclude_voided: true, ..Filters::default() })).await;
    assert_eq!(live.line_count as i64, all.line_count as i64 - voided);

    // Cleared and reconciled: the fixture reconciles checking for the first quarter.
    let checking = account(&pool, "1010").await;
    let reconciled = run(&pool, &lines_report(Filters { account_ids: vec![checking], cleared: Some(custom_reports::Cleared::Reconciled), ..Filters::default() })).await;
    let uncleared = run(&pool, &lines_report(Filters { account_ids: vec![checking], cleared: Some(custom_reports::Cleared::Uncleared), ..Filters::default() })).await;
    let checking_all = run(&pool, &lines_report(Filters { account_ids: vec![checking], ..Filters::default() })).await;
    assert!(reconciled.line_count > 0);
    assert_eq!(reconciled.line_count + uncleared.line_count, checking_all.line_count);

    // Paid invoices: fully paid by year end.
    let paid = run(&pool, &lines_report(Filters { account_types: vec!["ACCOUNTS_RECEIVABLE".into()], transaction_types: vec!["INVOICE".into()], paid_status: Some(custom_reports::PaidStatus::Paid), ..Filters::default() })).await;
    let open = run(&pool, &lines_report(Filters { account_types: vec!["ACCOUNTS_RECEIVABLE".into()], transaction_types: vec!["INVOICE".into()], paid_status: Some(custom_reports::PaidStatus::Open), ..Filters::default() })).await;
    let invoices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices WHERE is_voided = 0").fetch_one(&pool).await.unwrap();
    assert_eq!((paid.line_count + open.line_count) as i64, invoices);
    assert_eq!(open.line_count, 3);
}

#[tokio::test]
async fn summary_columns_by_period() {
    let pool = company().await;
    let settings = year(ReportSettings {
        mode: Mode::Summary,
        group_by: Some(GroupBy::Account),
        column_by: Some(custom_reports::Period::Quarter),
        filters: Filters { classifications: vec!["INCOME".into()], ..Filters::default() },
        ..ReportSettings::default()
    });
    let report = run(&pool, &settings).await;
    let labels: Vec<&str> = report.columns.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Account", "Q1 2025", "Q2 2025", "Q3 2025", "Q4 2025", "Total"]);
    let pl = crate::statements::profit_and_loss(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    for row in report.rows.iter().filter(|r| r.kind != RowKind::Section) {
        let quarters: i64 = row.cells[1..5].iter().map(cents).sum();
        assert_eq!(quarters, cents(&row.cells[5]), "quarters add up to the total");
    }
    assert_eq!(cents(report.rows.last().unwrap().cells.last().unwrap()), pl.total_revenue_cents);
    // Each account row drills to its own lines.
    let first = &report.rows[0];
    assert_eq!(first.drill.as_ref().unwrap().account_ids, vec![first.account_id.unwrap()]);

    // By month as rows: each drills to its month.
    let months = run(&pool, &ReportSettings { group_by: Some(GroupBy::Month), column_by: None, ..settings.clone() }).await;
    let feb = months.rows.iter().find(|r| text(&r.cells[0]) == "Feb 2025").unwrap();
    let drill = feb.drill.as_ref().unwrap();
    assert_eq!((drill.start_date, drill.end_date), (Some(d(2025, 2, 1)), Some(d(2025, 2, 28))));
}

#[tokio::test]
async fn one_row_per_transaction() {
    let pool = company().await;
    let settings = year(ReportSettings { per_transaction: true, columns: vec![Column::Type, Column::Amount], ..ReportSettings::default() });
    let report = run(&pool, &settings).await;
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries WHERE status = 'POSTED' AND entry_date BETWEEN '2025-01-01' AND '2025-12-31'").fetch_one(&pool).await.unwrap();
    assert_eq!(report.line_count as i64, entries);
    let total: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM v_posted_lines WHERE entry_date BETWEEN '2025-01-01' AND '2025-12-31'").fetch_one(&pool).await.unwrap();
    assert_eq!(cents(&report.rows.last().unwrap().cells[2]), total);
}
