//! The job costing reports on the fixture company with jobs added: every report as a golden
//! snapshot, and checks that they add up to the Profit & Loss.
//!
//! In the autumn of 2025 Harbor Lane takes on building work for Kestrel Homes: two jobs (Lakeview
//! Remodel and Maple St Addition) and some work for Kestrel Homes itself. Lakeview's estimate
//! becomes its invoice; Maple St's estimate is still pending while it is billed separately. Costs
//! come from a bill split across both jobs and the shop, a check, a card charge, a voided card
//! charge, a vendor credit and stock (oak panels) sold to each job.

use std::fmt::Write;

use serde_json::Value;
use sqlx::SqlitePool;

use super::account;
use super::fixture::{d, harbor_lane, harbor_lane_in};
use crate::credits::{CreditMemoInput, VendorCreditInput, VendorCreditLineInput};
use crate::custom_reports::{ColumnKind, RowKind};
use crate::estimates::EstimateInput;
use crate::expenses::{BillInput, BillLineInput, ExpenseInput, SplitLineInput};
use crate::invoices::{InvoiceInput, InvoiceLineInput};
use crate::job_reports::{self, Breakdown, JobFilter, JobReport, JobReportKind, JobReportParams};
use crate::sales::{SalesLineInput, SalesReceiptInput};

const LAKEVIEW: &str = "Kestrel Homes:Lakeview Remodel";
const MAPLE: &str = "Kestrel Homes:Maple St Addition";

async fn customer(pool: &SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM customers WHERE name = ?1").bind(name).fetch_one(pool).await.unwrap_or_else(|e| panic!("customer {name}: {e}"))
}

async fn new_account(pool: &SqlitePool, number: &str, name: &str, account_type: &str) -> i64 {
    let input = crate::accounts::AccountInput { name: name.into(), account_number: Some(number.into()), account_type: account_type.into(), parent_id: None, description: None };
    crate::accounts::create_account(pool, input).await.expect("account").id
}

pub(crate) async fn company() -> SqlitePool {
    with_jobs(harbor_lane().await).await
}

async fn with_jobs(pool: SqlitePool) -> SqlitePool {
    let p = &pool;
    let sales = account(p, "4000").await;
    let services = account(p, "4100").await;
    let checking = account(p, "1010").await;
    let card = account(p, "2100").await;
    let contractors = account(p, "6400").await;
    let materials = new_account(p, "5100", "Job Materials", "COST_OF_GOODS_SOLD").await;
    let rental = new_account(p, "5200", "Equipment Rental", "COST_OF_GOODS_SOLD").await;

    let product = |name: &str, rate: i64, income: i64| crate::sales::ProductInput { name: name.into(), description: None, rate_cents: rate, income_account_id: income, is_taxable: false };
    let labor = crate::sales::create_product(p, product("Framing Labor", 8_500, services)).await.expect("product").id;
    let survey = crate::sales::create_product(p, product("Site Survey", 60_000, services)).await.expect("product").id;
    let oak = crate::sales::create_product(p, product("Oak Panels", 30_000, sales)).await.expect("product").id;
    let stock = crate::inventory::track_product(p, &crate::inventory::TrackInput { product_id: oak, sku: None, asset_account_id: None, cogs_account_id: None }).await.expect("track");
    for name in [LAKEVIEW, MAPLE] {
        crate::sales::create_customer(p, name, None).await.expect("job");
    }
    let (lakeview, maple, kestrel) = (customer(p, LAKEVIEW).await, customer(p, MAPLE).await, customer(p, "Kestrel Homes").await);

    let sale = |item: i64, description: &str, qty: i64, rate: i64, income: i64| SalesLineInput {
        product_service_id: Some(item),
        description: description.into(),
        quantity_e4: qty * 10_000,
        rate_cents: rate,
        income_account_id: income,
        class_id: None,
        is_taxable: false,
    };
    let invoice_line = |l: SalesLineInput| InvoiceLineInput {
        description: l.description,
        quantity_e4: l.quantity_e4,
        rate_cents: l.rate_cents,
        income_account_id: l.income_account_id,
        product_service_id: l.product_service_id,
        class_id: None,
        is_taxable: false,
    };
    let bill_line = |account_id: i64, cents: i64, memo: &str, job: Option<i64>| BillLineInput {
        account_id,
        amount_cents: cents,
        memo: Some(memo.into()),
        quantity_e4: None,
        rate_cents: None,
        product_service_id: None,
        class_id: None,
        customer_id: job,
    };

    // Twenty oak panels into stock at $150.
    let mut panels = bill_line(stock.asset_account_id, 300_000, "Oak panels", None);
    (panels.product_service_id, panels.quantity_e4, panels.rate_cents) = (Some(oak), Some(200_000), Some(15_000));
    let bill = |vendor: &str, number: &str, date, lines: Vec<BillLineInput>| BillInput {
        vendor_name: vendor.into(),
        bill_number: Some(number.into()),
        bill_date: date,
        due_date: date + chrono::Days::new(30),
        memo: None,
        total_cents: lines.iter().map(|l| l.amount_cents).sum(),
        lines,
    };
    crate::expenses::post_vendor_bill(p, bill("Timberline Supply", "TS-401", d(2025, 10, 1), vec![panels])).await.expect("stock bill");

    // Lakeview: estimate $7,000, invoiced in full from the estimate.
    let estimate = |name: &str, date, lines, discount_cents| EstimateInput {
        customer_name: name.into(),
        estimate_date: date,
        expiration_date: None,
        memo: Some("Kitchen and bath".into()),
        lines,
        tax_rate_id: None,
        discount_cents,
        details: Default::default(),
    };
    let lakeview_estimate = crate::estimates::create_estimate(
        p,
        estimate(LAKEVIEW, d(2025, 10, 5), vec![sale(labor, "Framing", 40, 8_500, services), sale(oak, "Oak panels", 10, 30_000, sales), sale(survey, "Survey", 1, 60_000, services)], 0),
    )
    .await
    .expect("estimate")
    .id;
    crate::estimates::convert_to_invoice(p, lakeview_estimate, d(2025, 10, 20), d(2025, 11, 19)).await.expect("convert");

    // Maple St: estimate $5,700 less a $200 discount, still pending; billed for 30 hours so far.
    crate::estimates::create_estimate(p, estimate(MAPLE, d(2025, 11, 1), vec![sale(labor, "Framing", 60, 8_500, services), sale(survey, "Survey", 1, 60_000, services)], 20_000))
        .await
        .expect("estimate");
    let invoice = |name: &str, date, lines: Vec<SalesLineInput>| InvoiceInput {
        customer_name: name.into(),
        issue_date: date,
        due_date: date + chrono::Days::new(30),
        lines: lines.into_iter().map(invoice_line).collect(),
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    crate::invoices::create_and_post_invoice(p, invoice(MAPLE, d(2025, 11, 15), vec![sale(labor, "Framing, first draw", 30, 8_500, services)])).await.expect("invoice");
    // Kestrel Homes itself: a survey of another lot.
    crate::invoices::create_and_post_invoice(p, invoice("Kestrel Homes", d(2025, 11, 20), vec![sale(survey, "Lot survey", 1, 60_000, services)])).await.expect("invoice");

    // One bill for both jobs and the shop: one ledger line for Job Materials, shared out.
    crate::expenses::post_vendor_bill(
        p,
        bill(
            "Timberline Supply",
            "TS-433",
            d(2025, 11, 5),
            vec![
                bill_line(materials, 120_000, "Lumber, Lakeview", Some(lakeview)),
                bill_line(materials, 80_000, "Lumber, Maple St", Some(maple)),
                bill_line(materials, 30_000, "Shop stock", None),
                bill_line(contractors, 100_000, "Framing subcontract", Some(lakeview)),
            ],
        ),
    )
    .await
    .expect("job bill");

    let split = |account_id: i64, cents: i64, job: i64| SplitLineInput { account_id, amount_cents: cents, memo: None, class_id: None, customer_id: Some(job) };
    let expense = |kind: &str, payee: &str, from: i64, date, lines| ExpenseInput {
        kind: kind.into(),
        payee_name: Some(payee.into()),
        payment_account_id: from,
        expense_date: date,
        check_number: (kind == "CHECK").then(|| "1050".into()),
        payment_method: None,
        memo: None,
        lines,
        currency: None,
    };
    crate::expenses::post_expense(p, expense("CHECK", "Metro Equipment Rental", checking, d(2025, 11, 12), vec![split(rental, 45_000, maple)])).await.expect("check");
    crate::expenses::post_expense(p, expense("EXPENSE", "Hardware Depot", card, d(2025, 11, 18), vec![split(materials, 12_000, lakeview)])).await.expect("card charge");
    let mistake = crate::expenses::post_expense(p, expense("EXPENSE", "Hardware Depot", card, d(2025, 11, 19), vec![split(materials, 7_500, maple)])).await.expect("card charge");
    crate::ledger::void_transaction(p, mistake.journal_entry_id.expect("posted")).await.expect("void");

    crate::credits::create_vendor_credit(
        p,
        VendorCreditInput {
            vendor_name: "Timberline Supply".into(),
            reference: Some("TS-C9".into()),
            credit_date: d(2025, 11, 25),
            memo: Some("Returned lumber".into()),
            lines: vec![VendorCreditLineInput { account_id: materials, amount_cents: 20_000, memo: None, class_id: None, customer_id: Some(lakeview) }],
        },
    )
    .await
    .expect("vendor credit");
    crate::credits::create_credit_memo(
        p,
        CreditMemoInput { customer_name: LAKEVIEW.into(), credit_date: d(2025, 11, 28), memo: None, refund_account_id: None, lines: vec![sale(labor, "Billing correction", 2, 8_500, services)], tax_rate_id: None },
    )
    .await
    .expect("credit memo");
    crate::sales::post_sales_receipt(
        p,
        SalesReceiptInput {
            customer_name: Some(MAPLE.into()),
            receipt_date: d(2025, 12, 2),
            deposit_account_id: checking,
            payment_method: "CHECK".into(),
            reference: None,
            memo: None,
            lines: vec![sale(oak, "Oak panels", 2, 30_000, sales)],
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .expect("sales receipt");
    assert!(kestrel > 0);
    pool
}

fn year() -> JobReportParams {
    JobReportParams { start_date: Some(d(2025, 1, 1)), end_date: Some(d(2025, 12, 31)), ..Default::default() }
}

async fn run(pool: &SqlitePool, kind: JobReportKind, params: &JobReportParams) -> JobReport {
    job_reports::run(pool, kind, params, d(2025, 12, 31)).await.unwrap_or_else(|e| panic!("{kind:?}: {e}"))
}

fn cents(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn render(title: &str, report: &JobReport) -> String {
    let mut out = format!("{title}, {} to {}\n\n", report.start_date.map_or("all dates".into(), |s| s.to_string()), report.end_date);
    let widths: Vec<usize> = report.columns.iter().enumerate().map(|(i, c)| if i == 0 { 34 } else if matches!(c.kind, ColumnKind::Text) { 22 } else { 13 }).collect();
    let right = |k: ColumnKind| !matches!(k, ColumnKind::Text | ColumnKind::Date);
    let mut head = String::new();
    for (c, w) in report.columns.iter().zip(&widths) {
        head += &if right(c.kind) { format!("{:>w$} ", c.label, w = *w) } else { format!("{:<w$} ", c.label, w = *w) };
    }
    writeln!(out, "{}", head.trim_end()).unwrap();
    for row in &report.rows {
        let mut line = String::new();
        for (i, ((c, w), v)) in report.columns.iter().zip(&widths).zip(&row.cells).enumerate() {
            let mut s = match (c.kind, v) {
                (_, Value::Null) => String::new(),
                (ColumnKind::Money, v) => crate::ledger::format_money(cents(v), ""),
                (_, Value::String(s)) => s.chars().take(if i == 0 { 34 } else { 22 }).collect(),
                (_, v) => v.to_string(),
            };
            if i == 0 {
                s = format!("{}{s}", "  ".repeat(row.depth as usize));
            }
            line += &if i > 0 && right(c.kind) { format!("{s:>w$} ", w = *w) } else { format!("{s:<w$} ", w = *w) };
        }
        let marks = [row.journal_entry_id.map(|_| "↗"), row.drill.as_ref().map(|_| "▸")].into_iter().flatten().collect::<String>();
        writeln!(out, "{} {marks}", line.trim_end()).unwrap();
    }
    writeln!(out, "\n{} lines", report.line_count).unwrap();
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n"
}

/// (cost, revenue, diff) of the row labelled `label` (the first match).
fn profit(report: &JobReport, label: &str) -> (i64, i64, i64) {
    let r = report.rows.iter().find(|r| r.cells[0].as_str() == Some(label)).unwrap_or_else(|| panic!("row {label}"));
    let n = r.cells.len();
    (cents(&r.cells[n - 3]), cents(&r.cells[n - 2]), cents(&r.cells[n - 1]))
}

fn total(report: &JobReport) -> &[Value] {
    let r = report.rows.last().expect("rows");
    assert_eq!(r.kind, RowKind::Total);
    &r.cells
}

#[tokio::test]
async fn snapshots() {
    let pool = company().await;
    let all = |by| JobReportParams { by, ..year() };
    for (name, kind, params) in [
        ("job_profit_summary", JobReportKind::JobProfitSummary, year()),
        ("job_profit_detail_by_item", JobReportKind::JobProfitDetail, all(Breakdown::Item)),
        ("job_profit_detail_by_account", JobReportKind::JobProfitDetail, all(Breakdown::Account)),
        ("job_estimates_summary", JobReportKind::JobEstimatesSummary, year()),
        ("job_estimates_detail", JobReportKind::JobEstimatesDetail, year()),
        ("item_profit", JobReportKind::ItemProfit, year()),
        ("item_estimates", JobReportKind::ItemEstimates, year()),
        ("estimates_by_job", JobReportKind::EstimatesByJob, year()),
        ("progress_invoices", JobReportKind::ProgressInvoices, year()),
    ] {
        let report = run(&pool, kind, &params).await;
        insta::assert_snapshot!(format!("jobs_{name}"), render(name, &report));
    }
    let kestrel = customer(&pool, "Kestrel Homes").await;
    let params = JobReportParams { filter: JobFilter { job_ids: vec![kestrel], with_subjobs: true, ..Default::default() }, ..year() };
    insta::assert_snapshot!("jobs_transactions_kestrel", render("job_transactions", &run(&pool, JobReportKind::JobTransactions, &params).await));

    let pl = job_reports::profit_and_loss_by_job(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    let mut out = String::new();
    let jobs: Vec<String> = pl.columns.iter().map(|c| c.label.clone()).collect();
    writeln!(out, "{:<34} {}", "", jobs.iter().map(|j| format!("{:>16}", j.chars().take(16).collect::<String>())).collect::<String>()).unwrap();
    for l in &pl.lines {
        let amounts: String = l.amounts_cents.iter().map(|a| format!("{:>16}", a.map(|c| crate::ledger::format_money(c, "")).unwrap_or_default())).collect();
        writeln!(out, "{:<34} {amounts}", format!("{}{}", "  ".repeat(l.depth as usize), l.label)).unwrap();
    }
    insta::assert_snapshot!("jobs_profit_and_loss_by_job", out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n");
}

#[tokio::test]
async fn job_amounts_follow_the_documents() {
    let pool = company().await;
    let summary = run(&pool, JobReportKind::JobProfitSummary, &year()).await;
    // Revenue: $7,000 invoiced less a $170 credit memo. Cost: $1,500 of panels sold, $1,200 lumber
    // and $1,000 framing on the shared bill, a $120 card charge, less a $200 vendor credit.
    assert_eq!(profit(&summary, "Lakeview Remodel"), (362_000, 683_000, 321_000));
    // Revenue: $2,550 invoiced and $600 of panels; cost: $800 lumber, $450 rental, $300 of panels
    // (the voided $75 charge nets to nothing).
    assert_eq!(profit(&summary, "Maple St Addition"), (155_000, 315_000, 160_000));
    assert_eq!(profit(&summary, "Kestrel Homes (other)"), (0, 60_000, 60_000));
    assert_eq!(profit(&summary, "Total Kestrel Homes"), (517_000, 1_058_000, 541_000));

    let estimates = run(&pool, JobReportKind::JobEstimatesSummary, &year()).await;
    let lakeview = estimates.rows.iter().find(|r| r.cells[0] == "Lakeview Remodel").unwrap();
    assert_eq!((cents(&lakeview.cells[2]), cents(&lakeview.cells[3]), cents(&lakeview.cells[4])), (700_000, 683_000, -17_000));
    assert_eq!(lakeview.cells[5].as_f64(), Some(-2.4));
    let maple = estimates.rows.iter().find(|r| r.cells[0] == "Maple St Addition").unwrap();
    assert_eq!(cents(&maple.cells[2]), 550_000, "estimate less its discount");

    let progress = run(&pool, JobReportKind::ProgressInvoices, &year()).await;
    let invoiced: Vec<(i64, i64, Option<f64>)> =
        progress.rows.iter().filter(|r| r.kind == RowKind::Detail).map(|r| (cents(&r.cells[3]), cents(&r.cells[4]), r.cells[5].as_f64())).collect();
    assert_eq!(invoiced, vec![(700_000, 700_000, Some(100.0)), (550_000, 0, Some(0.0))]);
    assert!(progress.rows.iter().filter(|r| r.kind == RowKind::Detail).next().unwrap().journal_entry_id.is_some(), "an invoiced estimate opens its invoice");
}

#[tokio::test]
async fn reports_add_up_to_the_profit_and_loss() {
    let pool = company().await;
    for (start, end) in [(d(2025, 1, 1), d(2025, 12, 31)), (d(2025, 11, 1), d(2025, 11, 30)), (d(2025, 10, 1), d(2025, 10, 31))] {
        let params = JobReportParams { start_date: Some(start), end_date: Some(end), ..Default::default() };
        let pl = crate::statements::profit_and_loss(&pool, start, end).await.unwrap();
        let expected = (pl.total_expenses_cents, pl.total_revenue_cents, pl.net_income_cents);
        let sums = |cells: &[Value]| {
            let n = cells.len();
            (cents(&cells[n - 3]), cents(&cells[n - 2]), cents(&cells[n - 1]))
        };
        for (kind, by) in [
            (JobReportKind::JobProfitSummary, Breakdown::Item),
            (JobReportKind::JobProfitDetail, Breakdown::Item),
            (JobReportKind::JobProfitDetail, Breakdown::Account),
            (JobReportKind::ItemProfit, Breakdown::Item),
        ] {
            let report = run(&pool, kind, &JobReportParams { by, ..params.clone() }).await;
            assert_eq!(sums(total(&report)), expected, "{kind:?} by {by:?}, {start} to {end}");
        }
        let lines = run(&pool, JobReportKind::JobTransactions, &params).await;
        let t = total(&lines);
        assert_eq!((cents(&t[7]), cents(&t[8])), (expected.0, expected.1), "transactions {start} to {end}");
        let detail_sum: (i64, i64) =
            lines.rows.iter().filter(|r| r.kind == RowKind::Detail).fold((0, 0), |(c, r), row| (c + cents(&row.cells[7]), r + cents(&row.cells[8])));
        assert_eq!(detail_sum, (expected.0, expected.1));

        // Actual revenue and cost in the estimate reports are the same totals.
        for kind in [JobReportKind::JobEstimatesSummary, JobReportKind::JobEstimatesDetail, JobReportKind::ItemEstimates] {
            let t = total(&run(&pool, kind, &params).await).to_vec();
            assert_eq!((cents(&t[1]), cents(&t[3])), (expected.0, expected.1), "{kind:?} {start} to {end}");
        }

        // Profit and Loss by Job: the Total column is the Profit & Loss, and each row adds across.
        let by_job = job_reports::profit_and_loss_by_job(&pool, start, end).await.unwrap();
        let totals: Vec<Option<i64>> = by_job.lines.iter().map(|l| *l.amounts_cents.last().unwrap()).collect();
        let plain: Vec<Option<i64>> = pl.lines.iter().map(|l| l.amount_cents).collect();
        assert_eq!(totals, plain, "P&L by Job total column {start} to {end}");
        let leaves: Vec<usize> = by_job.columns.iter().enumerate().filter(|(_, c)| matches!(c.kind, "class" | "other" | "unclassified")).map(|(i, _)| i).collect();
        for l in &by_job.lines {
            if let Some(total) = l.amounts_cents.last().copied().flatten() {
                let across: i64 = leaves.iter().map(|i| l.amounts_cents[*i].unwrap_or(0)).sum();
                assert_eq!(across, total, "{} adds across, {start} to {end}", l.label);
            }
        }
    }
}

#[tokio::test]
async fn filters_and_drill_downs() {
    let pool = company().await;
    let lakeview = customer(&pool, LAKEVIEW).await;
    let summary = run(&pool, JobReportKind::JobProfitSummary, &year()).await;
    // Each row's drill-down shows the same amounts.
    for r in summary.rows.iter().filter(|r| r.drill.is_some()) {
        let drill = r.drill.as_ref().unwrap();
        let detail = run(&pool, drill.report, &JobReportParams { filter: drill.filter.clone(), ..year() }).await;
        let n = r.cells.len();
        let t = total(&detail);
        assert_eq!((cents(&t[n - 3]), cents(&t[n - 2])), (cents(&r.cells[n - 3]), cents(&r.cells[n - 2])), "drill of {}", r.cells[0]);
    }
    // A job's own filter, and its transactions.
    let only = JobReportParams { filter: JobFilter { job_ids: vec![lakeview], ..Default::default() }, ..year() };
    let lines = run(&pool, JobReportKind::JobTransactions, &only).await;
    assert!(lines.rows.iter().filter(|r| r.kind == RowKind::Detail).all(|r| r.journal_entry_id.is_some()));
    assert_eq!(lines.rows.iter().filter(|r| r.kind == RowKind::Section).count(), 1);
    // Lines with no job: the shop's share of the shared bill, among the year's other activity.
    let shop = JobReportParams { filter: JobFilter { no_job: true, ..Default::default() }, ..year() };
    let detail = run(&pool, JobReportKind::JobProfitDetail, &JobReportParams { by: Breakdown::Account, ..shop }).await;
    assert_eq!(profit(&detail, "Job Materials").0, 30_000);
}

/// Writes this company to `$LOCALBOOKS_SEED_DIR/Job Costing.localbooks`, for trying the job reports
/// in the app: `LOCALBOOKS_SEED_DIR=<folder> cargo test -- --ignored seed_job_costing_company`.
#[tokio::test]
#[ignore = "writes a company file for hands-on testing"]
async fn seed_job_costing_company() {
    let dir = std::path::PathBuf::from(std::env::var("LOCALBOOKS_SEED_DIR").expect("set LOCALBOOKS_SEED_DIR"));
    let path = dir.join("Job Costing.localbooks");
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let pool = crate::db::open(&path).await.expect("create the company file");
    let pool = with_jobs(harbor_lane_in(pool).await).await;
    pool.close().await;
}
