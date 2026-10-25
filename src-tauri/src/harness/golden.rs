//! Golden report tests: the fixture company's reports rendered as plain text and compared with the
//! snapshots in `snapshots/`. A change to any reported number fails the test with a diff; when the
//! change is intended, review and accept it with `cargo insta review` (or rerun with
//! `INSTA_UPDATE=always`) and commit the updated `.snap` files with the change.

use std::fmt::Write;

use chrono::NaiveDate;
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

/// A multi-column statement (comparisons, budget vs. actual): label, then each column.
fn columns(out: &mut String, report: &crate::statements::ProfitAndLossByClass) {
    write!(out, "{:<52}", "").unwrap();
    for c in &report.columns {
        write!(out, "{:>16}", c.label).unwrap();
    }
    writeln!(out).unwrap();
    for line in &report.lines {
        write!(out, "{:<52}", format!("{}{}", "  ".repeat(line.depth as usize), line.label)).unwrap();
        for (c, amount) in report.columns.iter().zip(&line.amounts_cents) {
            let cell = match amount {
                None => String::new(),
                Some(bp) if c.kind == "percent" => format!("{}{}.{:02}%", if *bp < 0 { "-" } else { "" }, bp.abs() / 100, bp.abs() % 100),
                Some(cents) => money(*cents),
            };
            write!(out, "{cell:>16}").unwrap();
        }
        writeln!(out).unwrap();
    }
}

#[tokio::test]
async fn previous_year_comparisons() {
    let pool = company().await;
    // The fixture covers one year, so compare the second half with the first.
    let periods = crate::statements::PeriodReportInput {
        columns: vec![
            crate::statements::PeriodColumn { label: "Jul-Dec 2025".into(), start_date: d(2025, 7, 1), end_date: d(2025, 12, 31) },
            crate::statements::PeriodColumn { label: "Jan-Jun 2025".into(), start_date: d(2025, 1, 1), end_date: d(2025, 6, 30) },
        ],
        total: false,
        compare: true,
        basis: crate::statements::Basis::Accrual,
        class_filter: None,
    };
    let pl = crate::statements::profit_and_loss_periods(&pool, &periods).await.unwrap();
    let mut out = String::from("Profit & Loss comparison\n\n");
    columns(&mut out, &pl);
    let dates = [
        crate::statements::AsOfColumn { label: "Jan 15, 2026".into(), as_of: d(2026, 1, 15) },
        crate::statements::AsOfColumn { label: "Jun 30, 2025".into(), as_of: d(2025, 6, 30) },
    ];
    let bs = crate::statements::balance_sheet_columns(&pool, &dates, true).await.unwrap();
    writeln!(out, "\nBalance Sheet comparison\n").unwrap();
    columns(&mut out, &bs);
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn budget_vs_actual() {
    let pool = company().await;
    let account = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (checking, card, services, rent, office) = (account("1010").await, account("2100").await, account("4100").await, account("6300").await, account("6250").await);
    let budget = crate::statements::create_budget(&pool, "2025 plan", d(2025, 1, 1)).await.unwrap();
    let mut amounts = Vec::new();
    for period in 1..=12 {
        let a = |account_id, amount_cents| crate::statements::BudgetAmount { account_id, period, amount_cents };
        amounts.extend([a(services, 1_500_000), a(rent, 240_000), a(office, 30_000), a(checking, 4_000_000 + period * 100_000), a(card, 50_000)]);
    }
    crate::statements::set_budget_amounts(&pool, budget.id, &amounts).await.unwrap();
    let mut out = String::from("Profit & Loss Budget vs. Actual, Q1 2025\n\n");
    columns(&mut out, &crate::statements::budget_vs_actual(&pool, budget.id, 1, 3).await.unwrap());
    let bs = crate::statements::balance_sheet_budget_vs_actual(&pool, budget.id, 6).await.unwrap();
    writeln!(out, "\nBalance Sheet Budget vs. Actual as of {}\n", bs.end_date).unwrap();
    columns(&mut out, &bs);
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn missing_checks() {
    let pool = company().await;
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let office: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6250'").fetch_one(&pool).await.unwrap();
    // Two more checks: one reusing a number, one skipping ahead.
    for (number, day) in [("1015", 22), ("1024", 29)] {
        let input: crate::expenses::ExpenseInput = serde_json::from_value(serde_json::json!({
            "kind": "CHECK", "payeeName": "Harbor Hardware", "paymentAccountId": checking, "expenseDate": d(2025, 12, day), "checkNumber": number,
            "paymentMethod": null, "memo": null, "lines": [{ "accountId": office, "amountCents": 4_250, "memo": null }]
        }))
        .unwrap();
        crate::expenses::post_expense(&pool, input).await.unwrap();
    }
    let report = crate::report_center::missing_checks(&pool, checking, None, None).await.unwrap();
    let mut out = format!("Missing Checks, {}\n\n", report.account_name);
    for (i, r) in report.rows.iter().enumerate() {
        for gap in report.gaps.iter().filter(|g| g.before == i) {
            writeln!(out, "*** Missing: {} to {} ***", gap.first, gap.last).unwrap();
        }
        writeln!(
            out,
            "{:<8}{}  {:<14}{:<30}{:>14}{}{}",
            r.number,
            r.date,
            r.kind,
            r.party_name.as_deref().unwrap_or("-"),
            money(r.amount_cents),
            if r.is_voided { "  VOID" } else { "" },
            if r.is_duplicate { "  DUPLICATE" } else { "" }
        )
        .unwrap();
    }
    writeln!(out, "\nMissing {}, duplicates {}, other references {}", report.missing_count, report.duplicate_count, report.unnumbered_count).unwrap();
    insta::assert_snapshot!(out);
}

#[tokio::test]
async fn inventory_reports() {
    let pool = company().await;
    let p = &pool;
    let sales: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(p).await.unwrap();
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(p).await.unwrap();
    let mut items = Vec::new();
    for (name, price) in [("Framed Print", 12_000), ("Sketchbook", 2_400)] {
        let product = crate::sales::create_product(p, crate::sales::ProductInput { name: name.into(), description: Some(format!("{name}, stocked")), rate_cents: price, income_account_id: sales, is_taxable: false })
            .await
            .unwrap()
            .id;
        let item = crate::inventory::track_product(p, &crate::inventory::TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap();
        items.push((product, item.asset_account_id));
    }
    let buy = |date: NaiveDate, (product, asset): (i64, i64), qty: i64, rate: i64| {
        let input: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
            "vendorName": "Gallery Supply", "billNumber": null, "billDate": date, "dueDate": date, "memo": null, "totalCents": qty * rate,
            "lines": [{ "accountId": asset, "amountCents": qty * rate, "memo": null, "quantityE4": qty * 10_000, "rateCents": rate, "productServiceId": product }]
        }))
        .unwrap();
        async move { crate::expenses::post_vendor_bill(p, input).await.unwrap() }
    };
    let sell = |date: NaiveDate, (product, _): (i64, i64), qty: i64, rate: i64| {
        let input: crate::sales::SalesReceiptInput = serde_json::from_value(serde_json::json!({
            "customerName": "Brightwater Cafe", "receiptDate": date, "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null,
            "lines": [{ "productServiceId": product, "description": "Stock", "quantityE4": qty * 10_000, "rateCents": rate, "incomeAccountId": sales }]
        }))
        .unwrap();
        async move { crate::sales::post_sales_receipt(p, input).await.unwrap() }
    };
    buy(d(2025, 11, 3), items[0], 12, 4_500).await;
    buy(d(2025, 11, 17), items[0], 6, 5_100).await;
    buy(d(2025, 11, 5), items[1], 40, 875).await;
    sell(d(2025, 11, 20), items[0], 7, 12_000).await;
    sell(d(2025, 12, 4), items[1], 15, 2_400).await;
    sell(d(2025, 12, 12), items[0], 3, 12_000).await;

    let qty = |e4: i64| crate::forms::format_quantity(e4);
    let cost = |c: Option<i64>| c.map(money).unwrap_or_default();
    let v = crate::report_center::inventory_valuation(p, d(2025, 12, 31)).await.unwrap();
    let mut out = format!("Inventory Valuation Summary as of {}\n\n", v.as_of);
    for r in &v.rows {
        writeln!(out, "{:<16}{:>10}{:>12}{:>14}{:>12}{:>14}", r.name, qty(r.qty_on_hand_e4), cost(r.average_cost_cents), money(r.asset_value_cents), money(r.sales_price_cents), money(r.retail_value_cents)).unwrap();
    }
    writeln!(out, "{:<50}{:>14}{:>26}", "Total", money(v.total_asset_value_cents), money(v.total_retail_value_cents)).unwrap();

    let detail = crate::report_center::inventory_valuation_detail(p, d(2025, 11, 15), d(2025, 12, 31)).await.unwrap();
    writeln!(out, "\nInventory Valuation Detail, {} to {}\n", detail.start_date, detail.end_date).unwrap();
    for item in &detail.items {
        writeln!(out, "{}  (opening {} worth {})", item.name, qty(item.opening_qty_e4), money(item.opening_value_cents)).unwrap();
        for r in &item.rows {
            writeln!(out, "  {}  {:<10}{:<20}{:>8}{:>12}{:>8}{:>12}{:>12}", r.movement_date, r.kind, r.party_name.as_deref().unwrap_or("-"), qty(r.qty_change_e4), money(r.value_change_cents), qty(r.qty_on_hand_e4), cost(r.average_cost_cents), money(r.asset_value_cents)).unwrap();
        }
    }
    writeln!(out, "Total value {}", money(detail.total_value_cents)).unwrap();

    let status = crate::report_center::stock_status(p, d(2025, 11, 1), d(2025, 12, 31)).await.unwrap();
    writeln!(out, "\nInventory Stock Status by Item, {} to {}\n", status.start_date, status.end_date).unwrap();
    for r in &status.rows {
        writeln!(out, "{:<16}{:>10}{:>10}{:>10}{:>12}", r.name, qty(r.qty_on_hand_e4), qty(r.qty_on_order_e4), qty(r.qty_sold_e4), qty(r.sales_per_week_e4)).unwrap();
    }
    insta::assert_snapshot!(out);
}
