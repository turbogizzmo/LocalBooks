//! Golden report tests: the fixture company's reports rendered as plain text and compared with the
//! snapshots in `snapshots/`. A change to any reported number fails the test with a diff; when the
//! change is intended, review and accept it with `cargo insta review` (or rerun with
//! `INSTA_UPDATE=always`) and commit the updated `.snap` files with the change.

use std::fmt::Write;

use sqlx::SqlitePool;

use super::fixture::{d, harbor_lane};
use crate::statements::ReportLine;

fn money(cents: i64) -> String {
    crate::ledger::format_money(cents, "")
}

fn row(out: &mut String, depth: u32, label: &str, amount: Option<i64>) {
    let label = format!("{}{label}", "  ".repeat(depth as usize));
    match amount {
        Some(cents) => writeln!(out, "{label:<64}{:>16}", money(cents)),
        None => writeln!(out, "{label}"),
    }
    .unwrap();
}

fn lines(out: &mut String, lines: &[ReportLine]) {
    for line in lines {
        row(out, line.depth, &line.label, line.amount_cents);
    }
}

async fn company() -> SqlitePool {
    harbor_lane().await
}

#[tokio::test]
async fn profit_and_loss() {
    let pool = company().await;
    let pl = crate::statements::profit_and_loss(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    let mut out = format!("Profit & Loss, {} to {}\n\n", pl.start_date, pl.end_date);
    lines(&mut out, &pl.lines);
    writeln!(out).unwrap();
    row(&mut out, 0, "Total revenue", Some(pl.total_revenue_cents));
    row(&mut out, 0, "Total expenses", Some(pl.total_expenses_cents));
    row(&mut out, 0, "Gross profit", Some(pl.gross_profit_cents));
    row(&mut out, 0, "Net income", Some(pl.net_income_cents));
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn balance_sheet() {
    let pool = company().await;
    let mut out = String::new();
    // Year end, and mid-January of the next year, when 2025's profit has closed into Retained Earnings.
    for as_of in [d(2025, 12, 31), d(2026, 1, 15)] {
        let bs = crate::statements::balance_sheet(&pool, as_of).await.unwrap();
        writeln!(out, "Balance Sheet as of {} (fiscal year from {})\n", bs.as_of, bs.fiscal_year_start).unwrap();
        lines(&mut out, &bs.lines);
        writeln!(out).unwrap();
        row(&mut out, 0, "Total assets", Some(bs.total_assets_cents));
        row(&mut out, 0, "Total liabilities", Some(bs.total_liabilities_cents));
        row(&mut out, 0, "Total equity", Some(bs.total_equity_cents));
        row(&mut out, 0, "Net income", Some(bs.net_income_cents));
        writeln!(out, "Balanced: {} (difference {})\n", bs.is_balanced, money(bs.difference_cents)).unwrap();
    }
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn trial_balance() {
    let pool = company().await;
    let tb = crate::statements::trial_balance_as_of(&pool, d(2025, 12, 31)).await.unwrap();
    let mut out = format!("Trial Balance as of {}\n\n", tb.as_of);
    writeln!(out, "{:<44}{:<24}{:>16}{:>16}", "Account", "Type", "Debit", "Credit").unwrap();
    for r in &tb.rows {
        let name = format!("{} {}", r.account_number.as_deref().unwrap_or("----"), r.full_name);
        let side = |c: i64| if c == 0 { String::new() } else { money(c) };
        writeln!(out, "{name:<44}{:<24}{:>16}{:>16}", r.account_type_name, side(r.debit_cents), side(r.credit_cents)).unwrap();
    }
    writeln!(out, "{:<68}{:>16}{:>16}", "Total", money(tb.total_debit_cents), money(tb.total_credit_cents)).unwrap();
    insta::assert_snapshot!(out);
}

async fn aging(payable: bool) -> String {
    let pool = company().await;
    let report = crate::report_center::aging(&pool, payable, d(2025, 12, 31)).await.unwrap();
    let mut out = format!("{} aging as of {}\n\n", report.kind, report.as_of);
    write!(out, "{:<28}", "Name").unwrap();
    for label in report.bucket_labels.iter().chain(["Total"].iter()) {
        write!(out, "{label:>14}").unwrap();
    }
    writeln!(out).unwrap();
    let mut row = |name: &str, buckets: &[i64; 5], total: i64| {
        write!(out, "{name:<28}").unwrap();
        for cents in buckets.iter().chain([total].iter()) {
            write!(out, "{:>14}", money(*cents)).unwrap();
        }
        writeln!(out).unwrap();
    };
    for r in &report.rows {
        row(&r.party_name, &r.buckets_cents, r.total_cents);
    }
    row("TOTAL", &report.totals_cents, report.total_cents);
    writeln!(out, "\nOpen items").unwrap();
    for item in &report.items {
        writeln!(
            out,
            "{:<28}{:<14}{:<10}{}  due {}  {:>4} days past due{:>14}",
            item.party_name,
            item.kind,
            item.number.as_deref().unwrap_or("-"),
            item.date,
            item.due_date,
            item.days_past_due,
            money(item.open_cents)
        )
        .unwrap();
    }
    out
}

#[tokio::test]
async fn ar_aging() {
    insta::assert_snapshot!(aging(false).await);
}

#[tokio::test]
async fn ap_aging() {
    insta::assert_snapshot!(aging(true).await);
}

#[tokio::test]
async fn cash_flow() {
    let pool = company().await;
    let flow = crate::statements::cash_flow(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    let mut out = format!("Statement of Cash Flows, {} to {}\n\n", flow.start_date, flow.end_date);
    lines(&mut out, &flow.lines);
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn vendors_1099() {
    let pool = company().await;
    let rows = crate::workflows::vendors_1099(&pool, 2025, crate::tax_ids::Access::OPEN).await.unwrap();
    let mut out = String::from("1099-NEC summary, 2025\n\n");
    for r in &rows {
        writeln!(
            out,
            "{:<24}{:<14}{:<40}{:>14}  {}",
            r.vendor_name,
            r.tax_id.as_deref().unwrap_or("-"),
            r.address.as_deref().unwrap_or("-"),
            money(r.paid_cents),
            if r.over_threshold { "report" } else { "below threshold" }
        )
        .unwrap();
    }
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn sales_tax_liability() {
    let pool = company().await;
    let mut out = String::new();
    for (start, end) in [(d(2025, 1, 1), d(2025, 12, 31)), (d(2025, 10, 1), d(2025, 12, 31))] {
        let report = crate::sales_tax::tax_liability(&pool, start, end).await.unwrap();
        writeln!(out, "Sales tax liability, {} to {}\n", report.start_date, report.end_date).unwrap();
        writeln!(out, "{:<24}{:>10}{:>16}{:>16}{:>16}", "Rate", "ppm", "Taxable", "Non-taxable", "Tax").unwrap();
        for r in &report.rows {
            let ppm = r.rate_ppm.map(|p| p.to_string()).unwrap_or_default();
            writeln!(out, "{:<24}{ppm:>10}{:>16}{:>16}{:>16}", r.name, money(r.taxable_sales_cents), money(r.non_taxable_sales_cents), money(r.tax_collected_cents)).unwrap();
        }
        row(&mut out, 0, "Tax collected", Some(report.tax_collected_cents));
        row(&mut out, 0, "Tax paid in period", Some(report.paid_cents));
        row(&mut out, 0, "Owed at period end", Some(report.owed_cents));
        writeln!(out).unwrap();
    }
    insta::assert_snapshot!(out);
}
