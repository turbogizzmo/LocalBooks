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
//! | Income breakdown (income accounts, top five and the rest) | Profit and Loss, total income |
//! | Expense breakdown (cost of sales and expense accounts) | Profit and Loss, total expenses |
//! | Year over year (income, expenses and net, against the same dates a year earlier) | Profit and Loss for each period |
//! | To-dos (open, due by the period's end or undated) | To Do Notes |
//! | Note (text the user keeps on the page) | — |
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
    Notes,
    Todos,
    IncomeBreakdown,
    ExpenseBreakdown,
    YearOverYear,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 12] = [
        WidgetKind::AccountBalances,
        WidgetKind::WhoOwes,
        WidgetKind::VendorsToPay,
        WidgetKind::IncomeExpenseTrend,
        WidgetKind::TopCustomers,
        WidgetKind::TopItems,
        WidgetKind::TopVendors,
        WidgetKind::Notes,
        WidgetKind::Todos,
        WidgetKind::IncomeBreakdown,
        WidgetKind::ExpenseBreakdown,
        WidgetKind::YearOverYear,
    ];

    pub fn code(self) -> &'static str {
        match self {
            WidgetKind::AccountBalances => "ACCOUNT_BALANCES",
            WidgetKind::WhoOwes => "WHO_OWES",
            WidgetKind::VendorsToPay => "VENDORS_TO_PAY",
            WidgetKind::IncomeExpenseTrend => "INCOME_EXPENSE_TREND",
            WidgetKind::TopCustomers => "TOP_CUSTOMERS",
            WidgetKind::TopItems => "TOP_ITEMS",
            WidgetKind::TopVendors => "TOP_VENDORS",
            WidgetKind::Notes => "NOTES",
            WidgetKind::Todos => "TODOS",
            WidgetKind::IncomeBreakdown => "INCOME_BREAKDOWN",
            WidgetKind::ExpenseBreakdown => "EXPENSE_BREAKDOWN",
            WidgetKind::YearOverYear => "YEAR_OVER_YEAR",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code)
    }

    /// The period a new widget starts with.
    fn default_period(self) -> Period {
        match self {
            WidgetKind::AccountBalances | WidgetKind::WhoOwes | WidgetKind::VendorsToPay | WidgetKind::Notes | WidgetKind::Todos => Period::Today,
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
    const ALL: [Period; 8] = [
        Period::Today,
        Period::ThisMonth,
        Period::LastMonth,
        Period::ThisQuarter,
        Period::LastQuarter,
        Period::ThisFiscalYear,
        Period::LastFiscalYear,
        Period::Last12Months,
    ];

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
    /// Year over year: the same figure for the same dates a year earlier.
    pub previous_cents: Option<i64>,
    /// To-dos: the due date, if it has one.
    pub date: Option<String>,
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
    /// The widget's total, equal to the report's (to-dos: how many are open).
    pub total_cents: i64,
    /// The note widget's text.
    pub note: Option<String>,
    /// The report id the screen opens for the full figures (`bs`, `ar`, `ap`, `pl`,
    /// `sales-customer`, `sales-product`, `expenses-vendor`, `list:todo-notes`; empty for a note).
    pub report: &'static str,
}

/// How many names a "top" or aging widget lists before folding the rest into Others.
pub const TOP: usize = 5;

fn top(summary: &SummaryReport) -> (Vec<WidgetLine>, i64, i64) {
    let mut rows: Vec<&crate::report_center::SummaryRow> = summary.rows.iter().collect();
    rows.sort_by(|a, b| b.amount_cents.cmp(&a.amount_cents).then_with(|| a.label.cmp(&b.label)));
    let lines: Vec<WidgetLine> = rows
        .iter()
        .take(TOP)
        .map(|r| WidgetLine {
            label: r.label.clone(),
            amount_cents: r.amount_cents.get(),
            overdue_cents: None,
            account_id: None,
            previous_cents: None,
            date: None,
        })
        .collect();
    let shown: i64 = lines.iter().map(|l| l.amount_cents).sum();
    (lines, summary.total_cents.get() - shown, summary.total_cents.get())
}

async fn figures(
    pool: &SqlitePool,
    kind: WidgetKind,
    start: NaiveDate,
    end: NaiveDate,
    viewer: crate::reminders::Viewer,
) -> Result<(Vec<WidgetLine>, i64, Vec<TrendPoint>, i64, &'static str), String> {
    Ok(match kind {
        WidgetKind::AccountBalances => {
            let bs = crate::statements::balance_sheet(pool, end).await?;
            let kinds: Vec<(i64, String)> = sqlx::query_as("SELECT id, account_type FROM chart_of_accounts WHERE account_type IN ('BANK', 'CREDIT_CARD')")
                .fetch_all(pool)
                .await
                .map_err(db_err)?;
            let lines: Vec<WidgetLine> = bs
                .lines
                .iter()
                .filter(|l| l.kind == LineKind::Account && l.account_id.is_some_and(|id| kinds.iter().any(|(k, _)| *k == id)))
                .map(|l| WidgetLine {
                    label: l.label.clone(),
                    amount_cents: l.amount_cents.map_or(0, crate::money::Cents::get),
                    overdue_cents: None,
                    account_id: l.account_id,
                    previous_cents: None,
                    date: None,
                })
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
                .map(|r| WidgetLine {
                    label: r.party_name.clone(),
                    amount_cents: r.total_cents.get(),
                    overdue_cents: Some(r.buckets_cents[1..].iter().map(|c| c.get()).sum()),
                    account_id: None,
                    previous_cents: None,
                    date: None,
                })
                .collect();
            let shown: i64 = lines.iter().map(|l| l.amount_cents).sum();
            (lines, aging.total_cents.get() - shown, Vec::new(), aging.total_cents.get(), if payable { "ap" } else { "ar" })
        }
        WidgetKind::IncomeExpenseTrend => {
            let graph = crate::graphs::income_expense(pool, start, end, report_basis(pool).await?).await?;
            let trend: Vec<TrendPoint> = graph
                .months
                .iter()
                .map(|p| TrendPoint {
                    month: p.month.month.clone(),
                    income_cents: p.income_cents,
                    expense_cents: p.expense_cents,
                    net_cents: p.net_income_cents,
                })
                .collect();
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
        WidgetKind::Notes => (Vec::new(), 0, Vec::new(), 0, ""),
        WidgetKind::Todos => {
            let open: Vec<(String, Option<String>)> = sqlx::query_as(
                // The viewer's own and unassigned to-dos, as the To Do list's reminders show them.
                "SELECT title, due_date FROM todos WHERE is_done = 0 AND (due_date IS NULL OR due_date <= ?1)
                   AND (?2 IS NULL OR assigned_user_id IS NULL OR assigned_user_id = ?2)
                 ORDER BY due_date IS NULL, due_date, id",
            )
            .bind(end)
            .bind(viewer.user_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            let count = i64::try_from(open.len()).unwrap_or(i64::MAX);
            let lines = open
                .into_iter()
                .take(TOP)
                .map(|(title, due)| WidgetLine { label: title, amount_cents: 0, overdue_cents: None, account_id: None, previous_cents: None, date: due })
                .collect();
            (lines, 0, Vec::new(), count, "list:todo-notes")
        }
        WidgetKind::IncomeBreakdown | WidgetKind::ExpenseBreakdown => {
            let income = kind == WidgetKind::IncomeBreakdown;
            let pl = crate::statements::profit_and_loss_basis(pool, start, end, None, report_basis(pool).await?).await?;
            let ids: Vec<i64> = sqlx::query_scalar(if income {
                "SELECT id FROM chart_of_accounts WHERE account_type IN ('INCOME', 'OTHER_INCOME')"
            } else {
                "SELECT id FROM chart_of_accounts WHERE account_type IN ('COST_OF_GOODS_SOLD', 'EXPENSE', 'OTHER_EXPENSE')"
            })
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            let rows: Vec<crate::report_center::SummaryRow> = pl
                .lines
                .iter()
                .filter(|l| l.kind == LineKind::Account && l.account_id.is_some_and(|id| ids.contains(&id)))
                .map(|l| crate::report_center::SummaryRow {
                    label: l.label.clone(),
                    quantity_e4: None,
                    amount_cents: l.amount_cents.unwrap_or(crate::money::Cents::ZERO),
                })
                .collect();
            let total = if income { pl.total_revenue_cents } else { pl.total_expenses_cents };
            let summary = SummaryReport { start_date: start, end_date: end, rows, total_cents: total };
            let (lines, others, total) = top(&summary);
            (lines, others, Vec::new(), total, "pl")
        }
        WidgetKind::YearOverYear => {
            let basis = report_basis(pool).await?;
            let now = crate::statements::profit_and_loss_basis(pool, start, end, None, basis).await?;
            let (ps, pe) = (start - Months::new(12), end - Months::new(12));
            let then = crate::statements::profit_and_loss_basis(pool, ps, pe, None, basis).await?;
            let line = |label: &str, a: i64, b: i64| WidgetLine {
                label: label.into(),
                amount_cents: a,
                overdue_cents: None,
                account_id: None,
                previous_cents: Some(b),
                date: None,
            };
            let lines = vec![
                line("Income", now.total_revenue_cents.get(), then.total_revenue_cents.get()),
                line("Expenses", now.total_expenses_cents.get(), then.total_expenses_cents.get()),
                line("Net income", now.net_income_cents.get(), then.net_income_cents.get()),
            ];
            (lines, 0, Vec::new(), now.net_income_cents.get(), "pl")
        }
    })
}

/// Accrual or cash, as Preferences › Reports & Graphs says (the Profit and Loss report's basis).
async fn report_basis(pool: &SqlitePool) -> Result<Basis, String> {
    Ok(if crate::preferences::get(pool, crate::preferences::REPORT_BASIS).await? == "cash" { Basis::Cash } else { Basis::Accrual })
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    kind: String,
    period: String,
    note: Option<String>,
}

/// The snapshot page for `today`: every widget in order, with its figures. The to-dos widget lists
/// the `viewer`'s own and unassigned to-dos; the page and its notes are the company's.
pub async fn snapshot(pool: &SqlitePool, today: NaiveDate, viewer: crate::reminders::Viewer) -> Result<Vec<Widget>, String> {
    let rows: Vec<Row> = sqlx::query_as("SELECT id, kind, period, note FROM snapshot_widgets ORDER BY position, id").fetch_all(pool).await.map_err(db_err)?;
    let fiscal_month = crate::preferences::fiscal_start_month(pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let (Some(kind), Some(period)) = (WidgetKind::from_code(&r.kind), Period::from_code(&r.period)) else { continue };
        let (start, end) = period.dates(today, fiscal_month);
        let (lines, others_cents, trend, total_cents, report) = figures(pool, kind, start, end, viewer).await?;
        out.push(Widget { id: r.id, kind, period, start_date: start, end_date: end, lines, others_cents, trend, total_cents, note: r.note, report });
    }
    Ok(out)
}

/// Adds a widget at the end of the page, with its usual period.
pub async fn add(pool: &SqlitePool, kind: WidgetKind) -> Result<i64, String> {
    sqlx::query_scalar(
        "INSERT INTO snapshot_widgets (kind, period, position) VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM snapshot_widgets)) RETURNING id",
    )
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

/// The longest note a note widget keeps (the column allows a little more).
const NOTE_MAX: usize = 2_000;

/// Sets a note widget's text (blank clears it).
pub async fn set_note(pool: &SqlitePool, id: i64, note: &str) -> Result<(), String> {
    let note = note.trim();
    if note.chars().count() > NOTE_MAX {
        return Err(format!("SNAPSHOT_NOTE_LONG: keep a note to {NOTE_MAX} characters"));
    }
    let done = sqlx::query("UPDATE snapshot_widgets SET note = ?1 WHERE id = ?2 AND kind = 'NOTES'")
        .bind((!note.is_empty()).then_some(note))
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("SNAPSHOT_NO_WIDGET: that note was removed".into());
    }
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
            sqlx::query("UPDATE snapshot_widgets SET position = ?1 WHERE id = ?2")
                .bind(i64::try_from(position).unwrap_or(i64::MAX))
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
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
