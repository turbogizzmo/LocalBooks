//! Company Snapshot: a page of widgets the user adds, removes, orders and gives a period (gaps row
//! T61).
//!
//! Every figure comes from the same function as the report it summarizes, with the same dates and
//! basis, so it ties out by construction, and each widget names that report so the screen can open
//! it:
//!
//! | Widget | Report |
//! |---|---|
//! | Account balances (bank and credit card; the total is the bank accounts') | Balance Sheet as of the period's end |
//! | Who owes you | A/R Aging Summary as of the period's end |
//! | Vendors to pay | A/P Aging Summary as of the period's end |
//! | Income and expense trend | Profit and Loss by month (`graphs::income_expense`) |
//! | Top customers | Sales by Customer Summary |
//! | Top items | Sales by Product/Service Summary |
//! | Top vendors | Expenses by Vendor Summary |
//!
//! The layout (which widgets, in what order, with what period) is kept in the company file
//! (`snapshot_widgets`), so everyone opening it sees the same page.

use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::report_center::SummaryReport;
use crate::statements::{Basis, LineKind};

/// What a widget shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WidgetKind {
    AccountBalances,
    WhoOwes,
    VendorsToPay,
    IncomeExpenseTrend,
    TopCustomers,
    TopItems,
    TopVendors,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 7] =
        [WidgetKind::AccountBalances, WidgetKind::WhoOwes, WidgetKind::VendorsToPay, WidgetKind::IncomeExpenseTrend, WidgetKind::TopCustomers, WidgetKind::TopItems, WidgetKind::TopVendors];

    pub fn code(self) -> &'static str {
        match self {
            WidgetKind::AccountBalances => "ACCOUNT_BALANCES",
            WidgetKind::WhoOwes => "WHO_OWES",
            WidgetKind::VendorsToPay => "VENDORS_TO_PAY",
            WidgetKind::IncomeExpenseTrend => "INCOME_EXPENSE_TREND",
            WidgetKind::TopCustomers => "TOP_CUSTOMERS",
            WidgetKind::TopItems => "TOP_ITEMS",
            WidgetKind::TopVendors => "TOP_VENDORS",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code)
    }

    /// The period a new widget starts with.
    fn default_period(self) -> Period {
        match self {
            WidgetKind::AccountBalances | WidgetKind::WhoOwes | WidgetKind::VendorsToPay => Period::Today,
            WidgetKind::IncomeExpenseTrend => Period::Last12Months,
            _ => Period::ThisFiscalYear,
        }
    }
}

/// A widget's dates. The balance widgets use the period's end as their "as of" date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Period {
    Today,
    ThisMonth,
    LastMonth,
    ThisQuarter,
    LastQuarter,
    ThisFiscalYear,
    LastFiscalYear,
    Last12Months,
}

impl Period {
    const ALL: [Period; 8] = [Period::Today, Period::ThisMonth, Period::LastMonth, Period::ThisQuarter, Period::LastQuarter, Period::ThisFiscalYear, Period::LastFiscalYear, Period::Last12Months];

    fn code(self) -> &'static str {
        match self {
            Period::Today => "TODAY",
            Period::ThisMonth => "THIS_MONTH",
            Period::LastMonth => "LAST_MONTH",
            Period::ThisQuarter => "THIS_QUARTER",
            Period::LastQuarter => "LAST_QUARTER",
            Period::ThisFiscalYear => "THIS_FISCAL_YEAR",
            Period::LastFiscalYear => "LAST_FISCAL_YEAR",
            Period::Last12Months => "LAST_12_MONTHS",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.code() == code)
    }

    /// The period's first and last day, for `today` and a fiscal year starting in `fiscal_month`.
    /// `Today` is the fiscal year to date (its end is today, for the balance widgets).
    pub fn dates(self, today: NaiveDate, fiscal_month: u32) -> (NaiveDate, NaiveDate) {
        let first_of = |d: NaiveDate| d.with_day(1).unwrap_or(d);
        let month_end = |start: NaiveDate, months: u32| start + Months::new(months) - chrono::Duration::days(1);
        let this_month = first_of(today);
        let quarter = first_of(today.with_month0(today.month0() / 3 * 3).unwrap_or(today));
        let (fy_start, fy_end) = crate::preferences::fiscal_year_bounds(today, fiscal_month);
        match self {
            Period::Today => (fy_start, today),
            Period::ThisMonth => (this_month, month_end(this_month, 1)),
            Period::LastMonth => {
                let start = this_month - Months::new(1);
                (start, month_end(start, 1))
            }
            Period::ThisQuarter => (quarter, month_end(quarter, 3)),
            Period::LastQuarter => {
                let start = quarter - Months::new(3);
                (start, month_end(start, 3))
            }
            Period::ThisFiscalYear => (fy_start, fy_end),
            Period::LastFiscalYear => (fy_start - Months::new(12), fy_start - chrono::Duration::days(1)),
            Period::Last12Months => (this_month - Months::new(11), month_end(this_month, 1)),
        }
    }
}

/// One line of a widget: a name and an amount (the account, customer, vendor or item), with the
/// overdue part for the aging widgets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetLine {
    pub label: String,
    pub amount_cents: i64,
    pub overdue_cents: Option<i64>,
    pub account_id: Option<i64>,
}

/// A month of the income and expense trend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
}

/// A widget with its figures and the report they tie to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    pub id: i64,
    pub kind: WidgetKind,
    pub period: Period,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub lines: Vec<WidgetLine>,
    /// Names beyond the top ones, folded into one figure.
    pub others_cents: i64,
    pub trend: Vec<TrendPoint>,
    /// The widget's total, equal to the report's.
    pub total_cents: i64,
    /// The report id the screen opens for the full figures (`bs`, `ar`, `ap`, `pl`,
    /// `sales-customer`, `sales-product`, `expenses-vendor`).
    pub report: &'static str,
}

/// How many names a "top" or aging widget lists before folding the rest into Others.
pub const TOP: usize = 5;

fn top(summary: &SummaryReport) -> (Vec<WidgetLine>, i64, i64) {
    let mut rows: Vec<&crate::report_center::SummaryRow> = summary.rows.iter().collect();
    rows.sort_by(|a, b| b.amount_cents.cmp(&a.amount_cents).then_with(|| a.label.cmp(&b.label)));
    let lines: Vec<WidgetLine> = rows.iter().take(TOP).map(|r| WidgetLine { label: r.label.clone(), amount_cents: r.amount_cents, overdue_cents: None, account_id: None }).collect();
    let shown: i64 = lines.iter().map(|l| l.amount_cents).sum();
    (lines, summary.total_cents - shown, summary.total_cents)
}

async fn figures(pool: &SqlitePool, kind: WidgetKind, start: NaiveDate, end: NaiveDate) -> Result<(Vec<WidgetLine>, i64, Vec<TrendPoint>, i64, &'static str), String> {
    Ok(match kind {
        WidgetKind::AccountBalances => {
            let bs = crate::statements::balance_sheet(pool, end).await?;
            let kinds: Vec<(i64, String)> = sqlx::query_as("SELECT id, account_type FROM chart_of_accounts WHERE account_type IN ('BANK', 'CREDIT_CARD')").fetch_all(pool).await.map_err(db_err)?;
            let lines: Vec<WidgetLine> = bs
                .lines
                .iter()
                .filter(|l| l.kind == LineKind::Account && l.account_id.is_some_and(|id| kinds.iter().any(|(k, _)| *k == id)))
                .map(|l| WidgetLine { label: l.label.clone(), amount_cents: l.amount_cents.unwrap_or(0), overdue_cents: None, account_id: l.account_id })
                .collect();
            // The total is the bank balances (the cards are listed, owed, beside them).
            let total = lines.iter().filter(|l| kinds.iter().any(|(id, t)| Some(*id) == l.account_id && t == "BANK")).map(|l| l.amount_cents).sum();
            (lines, 0, Vec::new(), total, "bs")
        }
        WidgetKind::WhoOwes | WidgetKind::VendorsToPay => {
            let payable = kind == WidgetKind::VendorsToPay;
            let aging = crate::report_center::aging(pool, payable, end).await?;
            let mut rows = aging.rows.clone();
            rows.sort_by(|a, b| b.total_cents.cmp(&a.total_cents).then_with(|| a.party_name.cmp(&b.party_name)));
            let lines: Vec<WidgetLine> = rows
                .iter()
                .take(TOP)
                .map(|r| WidgetLine { label: r.party_name.clone(), amount_cents: r.total_cents, overdue_cents: Some(r.buckets_cents[1..].iter().sum()), account_id: None })
                .collect();
            let shown: i64 = lines.iter().map(|l| l.amount_cents).sum();
            (lines, aging.total_cents - shown, Vec::new(), aging.total_cents, if payable { "ap" } else { "ar" })
        }
        WidgetKind::IncomeExpenseTrend => {
            let basis = if crate::preferences::get(pool, crate::preferences::REPORT_BASIS).await? == "cash" { Basis::Cash } else { Basis::Accrual };
            let graph = crate::graphs::income_expense(pool, start, end, basis).await?;
            let trend: Vec<TrendPoint> =
                graph.months.iter().map(|p| TrendPoint { month: p.month.month.clone(), income_cents: p.income_cents, expense_cents: p.expense_cents, net_cents: p.net_income_cents }).collect();
            (Vec::new(), 0, trend, graph.net_income_cents, "pl")
        }
        WidgetKind::TopCustomers => {
            let (lines, others, total) = top(&crate::report_center::by_party(pool, false, start, end).await?);
            (lines, others, Vec::new(), total, "sales-customer")
        }
        WidgetKind::TopVendors => {
            let (lines, others, total) = top(&crate::report_center::by_party(pool, true, start, end).await?);
            (lines, others, Vec::new(), total, "expenses-vendor")
        }
        WidgetKind::TopItems => {
            let (lines, others, total) = top(&crate::report_center::sales_by_product(pool, start, end).await?);
            (lines, others, Vec::new(), total, "sales-product")
        }
    })
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    kind: String,
    period: String,
}

/// The snapshot page for `today`: every widget in order, with its figures.
pub async fn snapshot(pool: &SqlitePool, today: NaiveDate) -> Result<Vec<Widget>, String> {
    let rows: Vec<Row> = sqlx::query_as("SELECT id, kind, period FROM snapshot_widgets ORDER BY position, id").fetch_all(pool).await.map_err(db_err)?;
    let fiscal_month = crate::preferences::fiscal_start_month(pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let (Some(kind), Some(period)) = (WidgetKind::from_code(&r.kind), Period::from_code(&r.period)) else { continue };
        let (start, end) = period.dates(today, fiscal_month);
        let (lines, others_cents, trend, total_cents, report) = figures(pool, kind, start, end).await?;
        out.push(Widget { id: r.id, kind, period, start_date: start, end_date: end, lines, others_cents, trend, total_cents, report });
    }
    Ok(out)
}

/// Adds a widget at the end of the page, with its usual period.
pub async fn add(pool: &SqlitePool, kind: WidgetKind) -> Result<i64, String> {
    sqlx::query_scalar("INSERT INTO snapshot_widgets (kind, period, position) VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM snapshot_widgets)) RETURNING id")
        .bind(kind.code())
        .bind(kind.default_period().code())
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn remove(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM snapshot_widgets WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn set_period(pool: &SqlitePool, id: i64, period: Period) -> Result<(), String> {
    let done = sqlx::query("UPDATE snapshot_widgets SET period = ?1 WHERE id = ?2").bind(period.code()).bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("SNAPSHOT_NO_WIDGET: that widget was removed".into());
    }
    Ok(())
}

/// Puts the widgets in the order given (every widget's id, once).
pub async fn arrange(pool: &SqlitePool, order: &[i64]) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<(), String> = async {
        let mut ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM snapshot_widgets").fetch_all(&mut *tx).await.map_err(db_err)?;
        let mut given = order.to_vec();
        ids.sort_unstable();
        given.sort_unstable();
        if ids != given {
            return Err("SNAPSHOT_ORDER: the page changed meanwhile; try again".into());
        }
        for (position, id) in order.iter().enumerate() {
            sqlx::query("UPDATE snapshot_widgets SET position = ?1 WHERE id = ?2").bind(i64::try_from(position).unwrap_or(i64::MAX)).bind(id).execute(&mut *tx).await.map_err(db_err)?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err),
        Err(e) => Err(crate::ledger::rollback(tx, e).await),
    }
}

#[cfg(test)]
mod tests;
