//! Graph reports (Reports › Graphs): Income and Expense, Net Worth, A/R and A/P aging, Sales, and
//! Budget vs. Actual. Every series is built from the matching text report's own function (one call
//! per month, plus one for the whole range), so a bar or slice always equals what that report shows
//! for the same dates.

use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::report_center::{self, SummaryReport};
use crate::statements::{self, Basis, ClassReportLine, LineKind, ReportLine};

/// The most months one graph shows, as for the multi-column Profit and Loss.
pub const MAX_MONTHS: usize = 36;

/// Slices and bars past this many fold into "Other".
pub const TOP_N: usize = 7;

/// One calendar month of a graph's range; the first and last are clipped to the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMonth {
    /// `YYYY-MM`
    pub month: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

/// A slice of a pie or a bar of a ranking. `account_id` is set for account slices, so a click can
/// open the register; `other` marks the folded remainder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSlice {
    pub label: String,
    pub account_id: Option<i64>,
    pub amount_cents: i64,
    pub other: bool,
}

/// The calendar months from `start` to `end`.
pub fn months(start: NaiveDate, end: NaiveDate) -> Result<Vec<GraphMonth>, String> {
    if end < start {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let mut out = Vec::new();
    let mut first = NaiveDate::from_ymd_opt(start.year(), start.month(), 1).expect("first of month is valid");
    while first <= end {
        let next = first + Months::new(1);
        let last = next.pred_opt().expect("month has a last day");
        out.push(GraphMonth { month: first.format("%Y-%m").to_string(), start_date: first.max(start), end_date: last.min(end) });
        if out.len() > MAX_MONTHS {
            return Err(format!("RPT_BAD_COLUMNS: choose a range of at most {MAX_MONTHS} months"));
        }
        first = next;
    }
    Ok(out)
}

/// The largest `TOP_N` amounts, then one "Other" slice with the rest (when there is any rest).
fn top(mut slices: Vec<GraphSlice>) -> Vec<GraphSlice> {
    slices.sort_by(|a, b| b.amount_cents.cmp(&a.amount_cents).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
    if slices.len() <= TOP_N + 1 {
        return slices;
    }
    let rest: i64 = slices[TOP_N..].iter().map(|s| s.amount_cents).sum();
    let count = slices.len() - TOP_N;
    slices.truncate(TOP_N);
    slices.push(GraphSlice { label: format!("Other ({count})"), account_id: None, amount_cents: rest, other: true });
    slices
}

// ------------------------------------------------------------------------------------------------
// Income and Expense
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomeExpensePoint {
    #[serde(flatten)]
    pub month: GraphMonth,
    /// The month's Profit and Loss "Total Income" plus "Total Other Income".
    pub income_cents: i64,
    /// Its Cost of Goods Sold, Expenses and Other Expenses.
    pub expense_cents: i64,
    pub net_income_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomeExpenseGraph {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub basis: Basis,
    pub months: Vec<IncomeExpensePoint>,
    /// Totals of the Profit and Loss for the whole range.
    pub total_income_cents: i64,
    pub total_expense_cents: i64,
    pub net_income_cents: i64,
    /// Top-level income accounts (with their subaccounts) for the whole range, largest first.
    pub income_slices: Vec<GraphSlice>,
    pub expense_slices: Vec<GraphSlice>,
}

const INCOME_SECTIONS: &[&str] = &["Income", "Other Income"];
const EXPENSE_SECTIONS: &[&str] = &["Cost of Goods Sold", "Expenses", "Other Expenses"];

/// The top-level accounts of the given Profit and Loss sections, each with its subaccounts rolled
/// in (the "Total <account>" line when it has any).
fn section_accounts(lines: &[ReportLine], sections: &[&str]) -> Vec<GraphSlice> {
    let mut out: Vec<GraphSlice> = Vec::new();
    let mut inside = false;
    for line in lines {
        if line.depth == 0 {
            inside = line.kind == LineKind::Section && sections.contains(&line.label.as_str());
            continue;
        }
        if !inside || line.depth != 1 {
            continue;
        }
        match line.kind {
            LineKind::Account => out.push(GraphSlice { label: line.label.clone(), account_id: line.account_id, amount_cents: line.amount_cents.unwrap_or(0), other: false }),
            LineKind::Subtotal => {
                if let Some(parent) = out.iter_mut().rev().find(|s| s.account_id == line.account_id) {
                    parent.amount_cents = line.amount_cents.unwrap_or(0);
                }
            }
            _ => {}
        }
    }
    out.retain(|s| s.amount_cents != 0);
    out
}

pub async fn income_expense(pool: &SqlitePool, start: NaiveDate, end: NaiveDate, basis: Basis) -> Result<IncomeExpenseGraph, String> {
    let spans = months(start, end)?;
    let mut points = Vec::with_capacity(spans.len());
    for month in spans {
        let pl = statements::profit_and_loss_basis(pool, month.start_date, month.end_date, None, basis).await?;
        points.push(IncomeExpensePoint { month, income_cents: pl.total_revenue_cents, expense_cents: pl.total_expenses_cents, net_income_cents: pl.net_income_cents });
    }
    let whole = statements::profit_and_loss_basis(pool, start, end, None, basis).await?;
    Ok(IncomeExpenseGraph {
        start_date: start,
        end_date: end,
        basis,
        months: points,
        total_income_cents: whole.total_revenue_cents,
        total_expense_cents: whole.total_expenses_cents,
        net_income_cents: whole.net_income_cents,
        income_slices: top(section_accounts(&whole.lines, INCOME_SECTIONS)),
        expense_slices: top(section_accounts(&whole.lines, EXPENSE_SECTIONS)),
    })
}

// ------------------------------------------------------------------------------------------------
// Net Worth
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetWorthPoint {
    pub month: String,
    /// The Balance Sheet date: the month end, or the range's end date in its last month.
    pub as_of: NaiveDate,
    pub assets_cents: i64,
    pub liabilities_cents: i64,
    pub net_worth_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetWorthGraph {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub months: Vec<NetWorthPoint>,
}

/// Total assets, total liabilities and their difference from the Balance Sheet at each month end.
pub async fn net_worth(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<NetWorthGraph, String> {
    let mut points = Vec::new();
    for month in months(start, end)? {
        let bs = statements::balance_sheet(pool, month.end_date).await?;
        points.push(NetWorthPoint {
            month: month.month,
            as_of: month.end_date,
            assets_cents: bs.total_assets_cents,
            liabilities_cents: bs.total_liabilities_cents,
            net_worth_cents: bs.total_assets_cents - bs.total_liabilities_cents,
        });
    }
    Ok(NetWorthGraph { start_date: start, end_date: end, months: points })
}

// ------------------------------------------------------------------------------------------------
// A/R and A/P aging
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingBucket {
    pub label: &'static str,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingGraph {
    /// `RECEIVABLE` or `PAYABLE`.
    pub kind: String,
    pub as_of: NaiveDate,
    /// The A/R (A/P) Aging Summary's TOTAL row, column by column.
    pub buckets: Vec<AgingBucket>,
    pub total_cents: i64,
    /// Customers (vendors) by balance, largest first; credit balances are left out.
    pub parties: Vec<GraphSlice>,
}

pub async fn aging(pool: &SqlitePool, payable: bool, as_of: NaiveDate) -> Result<AgingGraph, String> {
    let report = report_center::aging(pool, payable, as_of).await?;
    let buckets = report.bucket_labels.iter().zip(report.totals_cents).map(|(label, amount_cents)| AgingBucket { label, amount_cents }).collect();
    let parties = report
        .rows
        .iter()
        .filter(|r| r.total_cents > 0)
        .map(|r| GraphSlice { label: r.party_name.clone(), account_id: None, amount_cents: r.total_cents, other: false })
        .collect();
    Ok(AgingGraph { kind: report.kind, as_of, buckets, total_cents: report.total_cents, parties: top(parties) })
}

// ------------------------------------------------------------------------------------------------
// Sales
// ------------------------------------------------------------------------------------------------

/// What the Sales graph's ranking (and its monthly totals) come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SalesBy {
    /// Sales by Customer Summary: income by customer.
    Customer,
    /// Sales by Product/Service Summary: invoice and sales receipt lines less credit memos.
    Item,
    /// Sales by Rep Summary.
    Rep,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesPoint {
    #[serde(flatten)]
    pub month: GraphMonth,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesGraph {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub by: SalesBy,
    pub months: Vec<SalesPoint>,
    pub total_cents: i64,
    pub top: Vec<GraphSlice>,
}

async fn sales_summary(pool: &SqlitePool, by: SalesBy, start: NaiveDate, end: NaiveDate) -> Result<SummaryReport, String> {
    match by {
        SalesBy::Customer => report_center::by_party(pool, false, start, end).await,
        SalesBy::Item => report_center::sales_by_product(pool, start, end).await,
        SalesBy::Rep => report_center::sales_by_rep(pool, start, end).await,
    }
}

pub async fn sales(pool: &SqlitePool, by: SalesBy, start: NaiveDate, end: NaiveDate) -> Result<SalesGraph, String> {
    let mut points = Vec::new();
    for month in months(start, end)? {
        let amount_cents = sales_summary(pool, by, month.start_date, month.end_date).await?.total_cents;
        points.push(SalesPoint { month, amount_cents });
    }
    let whole = sales_summary(pool, by, start, end).await?;
    let ranked = whole
        .rows
        .into_iter()
        .filter(|r| r.amount_cents > 0)
        .map(|r| GraphSlice { label: r.label, account_id: None, amount_cents: r.amount_cents, other: false })
        .collect();
    Ok(SalesGraph { start_date: start, end_date: end, by, months: points, total_cents: whole.total_cents, top: top(ranked) })
}

// ------------------------------------------------------------------------------------------------
// Budget vs. Actual
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetFigures {
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_income_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPoint {
    #[serde(flatten)]
    pub month: GraphMonth,
    /// Fiscal month of the budget's year (1 = its first month).
    pub period: i64,
    pub actual: BudgetFigures,
    pub budget: BudgetFigures,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetGraph {
    pub budget_id: i64,
    pub months: Vec<BudgetPoint>,
    pub actual: BudgetFigures,
    pub budget: BudgetFigures,
}

/// Income, expenses and net income in one column of a Profit and Loss Budget vs. Actual.
fn budget_figures(lines: &[ClassReportLine], column: usize) -> BudgetFigures {
    let mut figures = BudgetFigures::default();
    let mut section = "";
    for line in lines.iter().filter(|l| l.depth == 0) {
        let amount = line.amounts_cents.get(column).copied().flatten().unwrap_or(0);
        match line.kind {
            LineKind::Section => section = &line.label,
            LineKind::Total if line.label == format!("Total {section}") => {
                if INCOME_SECTIONS.contains(&section) {
                    figures.income_cents += amount;
                } else if EXPENSE_SECTIONS.contains(&section) {
                    figures.expense_cents += amount;
                }
            }
            LineKind::Net => figures.net_income_cents = amount,
            _ => {}
        }
    }
    figures
}

/// Each fiscal month of a budget's year: its Profit and Loss Budget vs. Actual for that month alone.
pub async fn budget_vs_actual(pool: &SqlitePool, budget_id: i64, from_period: i64, to_period: i64) -> Result<BudgetGraph, String> {
    if !(1..=12).contains(&from_period) || !(from_period..=12).contains(&to_period) {
        return Err("RPT_BAD_RANGE: choose months 1 to 12 of the fiscal year".into());
    }
    let mut points = Vec::new();
    for period in from_period..=to_period {
        let report = statements::budget_vs_actual(pool, budget_id, period, period).await?;
        let month = GraphMonth { month: report.start_date.format("%Y-%m").to_string(), start_date: report.start_date, end_date: report.end_date };
        points.push(BudgetPoint { month, period, actual: budget_figures(&report.lines, 0), budget: budget_figures(&report.lines, 1) });
    }
    let whole = statements::budget_vs_actual(pool, budget_id, from_period, to_period).await?;
    Ok(BudgetGraph { budget_id, months: points, actual: budget_figures(&whole.lines, 0), budget: budget_figures(&whole.lines, 1) })
}
