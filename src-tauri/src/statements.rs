//! Financial statements: Profit & Loss for a date range and Balance Sheet as of a date.
//!
//! Each statement is returned as a flat list of typed lines (section heading, account, subtotal,
//! total, net) carrying an indent depth, so the UI only applies typography. Subaccounts nest under
//! their parent with a "Total <parent>" line, as classic accounting software does. The fiscal year boundary (which
//! splits current-year Net Income from Retained Earnings) comes from the `fiscal_year_start`
//! preference.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{db_err, format_money};
use crate::preferences::{currency_symbol, fiscal_start_month, fiscal_year_bounds};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    Section,
    Account,
    Subtotal,
    Total,
    Net,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportLine {
    pub kind: LineKind,
    pub label: String,
    pub depth: u32,
    pub amount_cents: Option<i64>,
    pub account_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitAndLoss {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub lines: Vec<ReportLine>,
    pub total_revenue_cents: i64,
    pub total_expenses_cents: i64,
    pub gross_profit_cents: i64,
    pub net_income_cents: i64,
    pub net_income_display: String,
    pub currency_symbol: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSheet {
    pub as_of: NaiveDate,
    pub lines: Vec<ReportLine>,
    pub total_assets_cents: i64,
    pub total_liabilities_cents: i64,
    pub total_equity_cents: i64,
    pub net_income_cents: i64,
    pub is_balanced: bool,
    pub difference_cents: i64,
    /// First day of the fiscal year containing `as_of`; Net Income covers this date through `as_of`.
    pub fiscal_year_start: NaiveDate,
    pub currency_symbol: String,
}

struct Account {
    id: i64,
    label: String,
    account_type: String,
    is_debit_normal: bool,
    is_system: bool,
    name: String,
}

struct Chart {
    accounts: Vec<Account>,
    children: HashMap<Option<i64>, Vec<usize>>,
}

impl Chart {
    async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let rows: Vec<(i64, Option<i64>, Option<String>, String, String, String, bool)> = sqlx::query_as(
            "SELECT a.id, a.parent_id, a.account_number, a.name, a.account_type, t.normal_balance, a.is_system
             FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
             ORDER BY COALESCE(a.account_number, ''), a.name COLLATE NOCASE",
        )
        .fetch_all(pool)
        .await
        .map_err(db_err)?;

        let mut chart = Chart { accounts: Vec::with_capacity(rows.len()), children: HashMap::new() };
        for (id, parent_id, number, name, account_type, normal, is_system) in rows {
            chart.children.entry(parent_id).or_default().push(chart.accounts.len());
            chart.accounts.push(Account {
                id,
                label: number.map_or_else(|| name.clone(), |n| format!("{n} {name}")),
                account_type,
                is_debit_normal: normal == "DEBIT",
                is_system,
                name,
            });
        }
        Ok(chart)
    }

    fn natural(&self, debit_net: &HashMap<i64, i64>) -> HashMap<i64, i64> {
        self.accounts
            .iter()
            .filter_map(|a| {
                debit_net.get(&a.id).map(|&net| (a.id, if a.is_debit_normal { net } else { -net }))
            })
            .collect()
    }
}

/// Net debit movement per account (debits minus credits) over posted entries in `[from, to]`.
async fn debit_net(pool: &SqlitePool, from: Option<NaiveDate>, to: NaiveDate) -> Result<HashMap<i64, i64>, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT account_id, SUM(debit_cents) - SUM(credit_cents)
         FROM v_posted_lines
         WHERE (?1 IS NULL OR entry_date >= ?1) AND entry_date <= ?2
         GROUP BY account_id",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

const PL_TYPES: &[&str] = &["INCOME", "COST_OF_GOODS_SOLD", "EXPENSE", "OTHER_INCOME", "OTHER_EXPENSE"];

/// Net income (credit-positive) of all P&L accounts in a debit-net movement map.
fn net_income(chart: &Chart, debit_net: &HashMap<i64, i64>) -> i64 {
    -chart
        .accounts
        .iter()
        .filter(|a| PL_TYPES.contains(&a.account_type.as_str()))
        .filter_map(|a| debit_net.get(&a.id))
        .sum::<i64>()
}

struct Builder<'a> {
    chart: &'a Chart,
    /// Amounts that decide which accounts and sections appear. Equal to `natural`, except in a
    /// Profit & Loss by Class column, where every column takes its shape from the Total column so
    /// all columns have the same rows.
    shape: HashMap<i64, i64>,
    natural: HashMap<i64, i64>,
    lines: Vec<ReportLine>,
}

impl<'a> Builder<'a> {
    fn new(chart: &'a Chart, natural: HashMap<i64, i64>) -> Self {
        Builder { chart, shape: natural.clone(), natural, lines: Vec::new() }
    }
}

impl<'a> Builder<'a> {
    fn push(&mut self, kind: LineKind, label: impl Into<String>, depth: u32, amount_cents: Option<i64>, account_id: Option<i64>) {
        self.lines.push(ReportLine { kind, label: label.into(), depth, amount_cents, account_id });
    }

    /// Emits one account and its subaccounts; returns (rolled-up total, has activity).
    fn node(&mut self, idx: usize, depth: u32) -> (i64, bool) {
        let account = &self.chart.accounts[idx];
        let (id, label) = (account.id, account.label.clone());
        let own = self.natural.get(&id).copied().unwrap_or(0);
        let shown = self.shape.get(&id).is_some_and(|&a| a != 0);

        let start = self.lines.len();
        let mut child_total = 0;
        let mut child_active = false;
        for child in self.chart.children.get(&Some(id)).cloned().unwrap_or_default() {
            let (total, active) = self.node(child, depth + 1);
            child_total += total;
            child_active |= active;
        }
        let child_lines: Vec<ReportLine> = self.lines.drain(start..).collect();

        if !child_active {
            if shown {
                self.push(LineKind::Account, label, depth, Some(own), Some(id));
            }
            return (own, shown);
        }
        self.push(LineKind::Account, label.clone(), depth, shown.then_some(own), Some(id));
        self.lines.extend(child_lines);
        self.push(LineKind::Subtotal, format!("Total {label}"), depth, Some(own + child_total), Some(id));
        (own + child_total, true)
    }

    /// Emits every top-level account of the given types.
    fn accounts(&mut self, types: &[&str], depth: u32) -> (i64, bool) {
        let roots: Vec<usize> = self.chart.children.get(&None).cloned().unwrap_or_default();
        let mut total = 0;
        let mut active = false;
        for idx in roots {
            if types.contains(&self.chart.accounts[idx].account_type.as_str()) {
                let (t, a) = self.node(idx, depth);
                total += t;
                active |= a;
            }
        }
        (total, active)
    }

    /// A headed group closed by a total line. Inactive groups are dropped unless `always`.
    fn group(
        &mut self,
        title: &str,
        total_label: &str,
        total_kind: LineKind,
        depth: u32,
        always: bool,
        body: impl FnOnce(&mut Self) -> (i64, bool),
    ) -> i64 {
        let start = self.lines.len();
        self.push(LineKind::Section, title, depth, None, None);
        let (total, active) = body(self);
        if !active && !always {
            self.lines.truncate(start);
            return 0;
        }
        self.push(total_kind, total_label, depth, Some(total), None);
        total
    }

    fn section(&mut self, title: &str, types: &'static [&'static str], depth: u32, always: bool) -> i64 {
        self.group(title, &format!("Total {title}"), LineKind::Total, depth, always, |b| b.accounts(types, depth + 1))
    }
}

pub async fn profit_and_loss(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<ProfitAndLoss, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let chart = Chart::load(pool).await?;
    let movement = debit_net(pool, Some(start_date), end_date).await?;
    let pl = build_pl(Builder::new(&chart, chart.natural(&movement)), None);
    debug_assert_eq!(pl.net, net_income(&chart, &movement));

    let symbol = currency_symbol(pool).await?;
    Ok(ProfitAndLoss {
        start_date,
        end_date,
        lines: pl.lines,
        total_revenue_cents: pl.income + pl.other_income,
        total_expenses_cents: pl.cogs + pl.expenses + pl.other_expense,
        gross_profit_cents: pl.income - pl.cogs,
        net_income_cents: pl.net,
        net_income_display: format_money(pl.net, &symbol),
        currency_symbol: symbol,
    })
}

struct PlBody {
    lines: Vec<ReportLine>,
    income: i64,
    cogs: i64,
    expenses: i64,
    other_income: i64,
    other_expense: i64,
    net: i64,
    has_net_other: bool,
}

/// Lays out a Profit & Loss. `net_other` forces whether the "Net Other Income" line appears (so a
/// by-class column matches its Total column); `None` shows it when there is other income or expense.
fn build_pl(mut b: Builder<'_>, net_other: Option<bool>) -> PlBody {
    let income = b.section("Income", &["INCOME"], 0, true);
    let cogs = b.section("Cost of Goods Sold", &["COST_OF_GOODS_SOLD"], 0, false);
    let gross_profit = income - cogs;
    b.push(LineKind::Total, "Gross Profit", 0, Some(gross_profit), None);
    let expenses = b.section("Expenses", &["EXPENSE"], 0, true);
    let net_operating = gross_profit - expenses;
    b.push(LineKind::Total, "Net Operating Income", 0, Some(net_operating), None);
    let other_income = b.section("Other Income", &["OTHER_INCOME"], 0, false);
    let other_expense = b.section("Other Expenses", &["OTHER_EXPENSE"], 0, false);
    let has_net_other = net_other.unwrap_or(other_income != 0 || other_expense != 0);
    if has_net_other {
        b.push(LineKind::Total, "Net Other Income", 0, Some(other_income - other_expense), None);
    }
    let net = net_operating + other_income - other_expense;
    b.push(LineKind::Net, "Net Income", 0, Some(net), None);
    PlBody { lines: b.lines, income, cogs, expenses, other_income, other_expense, net, has_net_other }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassColumn {
    /// `None` for the Unclassified column and for the Total column (`is_total`).
    pub class_id: Option<i64>,
    pub label: String,
    pub is_total: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassReportLine {
    pub kind: LineKind,
    pub label: String,
    pub depth: u32,
    pub account_id: Option<i64>,
    /// One amount per column, in `columns` order; `None` where the row has no amount (headings).
    pub amounts_cents: Vec<Option<i64>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitAndLossByClass {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub columns: Vec<ClassColumn>,
    pub lines: Vec<ClassReportLine>,
    pub currency_symbol: String,
}

/// Profit & Loss with one column per class that has income or expense in the range, an
/// "Unclassified" column for lines without a class, and a Total column equal to the ordinary
/// Profit & Loss. Every column has the same rows, so each row adds across to its total.
pub async fn profit_and_loss_by_class(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<ProfitAndLossByClass, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let chart = Chart::load(pool).await?;
    let pl_accounts: std::collections::HashSet<i64> =
        chart.accounts.iter().filter(|a| PL_TYPES.contains(&a.account_type.as_str())).map(|a| a.id).collect();

    // Debit-net movement per (class, account) over P&L accounts only.
    let rows: Vec<(Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT class_id, account_id, SUM(debit_cents) - SUM(credit_cents)
         FROM v_posted_lines
         WHERE entry_date >= ?1 AND entry_date <= ?2
         GROUP BY class_id, account_id",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut by_class: HashMap<Option<i64>, HashMap<i64, i64>> = HashMap::new();
    let mut total: HashMap<i64, i64> = HashMap::new();
    for (class, account, net) in rows {
        if net != 0 && pl_accounts.contains(&account) {
            by_class.entry(class).or_default().insert(account, net);
            *total.entry(account).or_insert(0) += net;
        }
    }

    let names: Vec<(i64, String)> = sqlx::query_as("SELECT id, full_name FROM v_classes ORDER BY full_name COLLATE NOCASE")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let mut columns: Vec<ClassColumn> = names
        .into_iter()
        .filter(|(id, _)| by_class.contains_key(&Some(*id)))
        .map(|(id, label)| ClassColumn { class_id: Some(id), label, is_total: false })
        .collect();
    if by_class.contains_key(&None) {
        columns.push(ClassColumn { class_id: None, label: "Unclassified".into(), is_total: false });
    }

    let total_natural = chart.natural(&total);
    let totals = build_pl(Builder::new(&chart, total_natural.clone()), None);
    let bodies: Vec<PlBody> = columns
        .iter()
        .map(|c| {
            let natural = chart.natural(by_class.get(&c.class_id).unwrap_or(&HashMap::new()));
            let b = Builder { chart: &chart, shape: total_natural.clone(), natural, lines: Vec::new() };
            build_pl(b, Some(totals.has_net_other))
        })
        .collect();

    let lines = totals
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let mut amounts_cents: Vec<Option<i64>> = bodies.iter().map(|b| b.lines[i].amount_cents).collect();
            amounts_cents.push(line.amount_cents);
            ClassReportLine { kind: line.kind, label: line.label.clone(), depth: line.depth, account_id: line.account_id, amounts_cents }
        })
        .collect();
    columns.push(ClassColumn { class_id: None, label: "Total".into(), is_total: true });

    Ok(ProfitAndLossByClass { start_date, end_date, columns, lines, currency_symbol: currency_symbol(pool).await? })
}

pub async fn balance_sheet(pool: &SqlitePool, as_of: NaiveDate) -> Result<BalanceSheet, String> {
    let chart = Chart::load(pool).await?;
    let (fiscal_start, _) = fiscal_year_bounds(as_of, fiscal_start_month(pool).await?);
    let cumulative = debit_net(pool, None, as_of).await?;
    let prior = match fiscal_start.pred_opt() {
        Some(day_before) => debit_net(pool, None, day_before).await?,
        None => HashMap::new(),
    };
    let prior_net_income = net_income(&chart, &prior);
    let year_net_income = net_income(&chart, &cumulative) - prior_net_income;

    // Prior years' profit closes into Retained Earnings; this year's shows as Net Income.
    let mut natural = chart.natural(&cumulative);
    if let Some(re) = chart.accounts.iter().find(|a| a.is_system && a.account_type == "EQUITY" && a.name == "Retained Earnings") {
        *natural.entry(re.id).or_insert(0) += prior_net_income;
    }
    let mut b = Builder::new(&chart, natural);

    let assets = b.group("ASSETS", "TOTAL ASSETS", LineKind::Net, 0, true, |b| {
        let current = b.group("Current Assets", "Total Current Assets", LineKind::Total, 1, true, |b| {
            let bank = b.section("Bank Accounts", &["BANK"], 2, false);
            let ar = b.section("Accounts Receivable", &["ACCOUNTS_RECEIVABLE"], 2, false);
            let other = b.section("Other Current Assets", &["OTHER_CURRENT_ASSET"], 2, false);
            (bank + ar + other, true)
        });
        let fixed = b.section("Fixed Assets", &["FIXED_ASSET"], 1, false);
        let other = b.section("Other Assets", &["OTHER_ASSET"], 1, false);
        (current + fixed + other, true)
    });

    let mut liabilities = 0;
    let mut equity = 0;
    let liabilities_and_equity =
        b.group("LIABILITIES AND EQUITY", "TOTAL LIABILITIES AND EQUITY", LineKind::Net, 0, true, |b| {
            liabilities = b.group("Liabilities", "Total Liabilities", LineKind::Total, 1, true, |b| {
                let current = b.group("Current Liabilities", "Total Current Liabilities", LineKind::Total, 2, true, |b| {
                    let ap = b.section("Accounts Payable", &["ACCOUNTS_PAYABLE"], 3, false);
                    let cards = b.section("Credit Cards", &["CREDIT_CARD"], 3, false);
                    let other = b.section("Other Current Liabilities", &["OTHER_CURRENT_LIABILITY"], 3, false);
                    (ap + cards + other, true)
                });
                let long_term = b.section("Long-Term Liabilities", &["LONG_TERM_LIABILITY"], 2, false);
                (current + long_term, true)
            });
            equity = b.group("Equity", "Total Equity", LineKind::Total, 1, true, |b| {
                let (accounts, _) = b.accounts(&["EQUITY"], 2);
                if year_net_income != 0 {
                    b.push(LineKind::Account, "Net Income", 2, Some(year_net_income), None);
                }
                (accounts + year_net_income, true)
            });
            (liabilities + equity, true)
        });

    let difference_cents = assets - liabilities_and_equity;
    Ok(BalanceSheet {
        as_of,
        lines: b.lines,
        total_assets_cents: assets,
        total_liabilities_cents: liabilities,
        total_equity_cents: equity,
        net_income_cents: year_net_income,
        is_balanced: difference_cents == 0,
        difference_cents,
        fiscal_year_start: fiscal_start,
        currency_symbol: currency_symbol(pool).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
            .bind(number)
            .fetch_one(pool)
            .await
            .expect("seeded account exists")
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    async fn post(pool: &SqlitePool, date: NaiveDate, debit: i64, credit: i64, cents: i64) {
        post_journal_entry(
            pool,
            JournalEntry { id: None, date, description: String::new(), lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)], currency: None },
        )
        .await
        .expect("posts");
    }

    fn find<'l>(lines: &'l [ReportLine], label: &str) -> &'l ReportLine {
        lines.iter().find(|l| l.label == label).unwrap_or_else(|| panic!("missing line {label}"))
    }

    #[tokio::test]
    async fn profit_and_loss_by_class_splits_every_row_and_adds_across() {
        use crate::classes::{create_class, ClassInput};
        let pool = test_pool().await;
        let (checking, sales, rent, interest) =
            (account_id(&pool, "1010").await, account_id(&pool, "4000").await, account_id(&pool, "6300").await, account_id(&pool, "7000").await);
        let class = |name: &str| create_class(&pool, ClassInput { name: name.into(), parent_id: None });
        let (east, west, idle) = (class("East").await.unwrap().id, class("West").await.unwrap().id, class("Idle").await.unwrap().id);
        let post_lines = |lines: Vec<JournalLine>| {
            post_journal_entry(&pool, JournalEntry { id: None, date: ymd(2026, 4, 10), description: String::new(), lines, currency: None })
        };

        post_lines(vec![
            JournalLine::debit(checking, 100_000),
            JournalLine::credit(sales, 60_000).with_class(Some(east)),
            JournalLine::credit(sales, 40_000).with_class(Some(west)),
        ])
        .await
        .unwrap();
        post_lines(vec![JournalLine::debit(rent, 30_000).with_class(Some(east)), JournalLine::credit(checking, 30_000)]).await.unwrap();
        post_lines(vec![JournalLine::debit(checking, 500), JournalLine::credit(interest, 500)]).await.unwrap();
        // A voided West expense nets to zero in West.
        let voided = post_lines(vec![JournalLine::debit(rent, 9_999).with_class(Some(west)), JournalLine::credit(checking, 9_999)]).await.unwrap();
        crate::ledger::void_transaction(&pool, voided).await.unwrap();
        // Outside the range.
        post_journal_entry(
            &pool,
            JournalEntry { id: None, date: ymd(2026, 6, 1), description: String::new(), currency: None,
                lines: vec![JournalLine::debit(rent, 1).with_class(Some(idle)), JournalLine::credit(checking, 1)] },
        )
        .await
        .unwrap();

        let report = profit_and_loss_by_class(&pool, ymd(2026, 4, 1), ymd(2026, 4, 30)).await.unwrap();
        let labels: Vec<&str> = report.columns.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["East", "West", "Unclassified", "Total"]);

        let plain = profit_and_loss(&pool, ymd(2026, 4, 1), ymd(2026, 4, 30)).await.unwrap();
        assert_eq!(report.lines.len(), plain.lines.len());
        for (row, line) in report.lines.iter().zip(&plain.lines) {
            assert_eq!(row.label, line.label);
            assert_eq!(*row.amounts_cents.last().unwrap(), line.amount_cents, "Total column of {}", row.label);
            if let Some(total) = line.amount_cents {
                let across: i64 = row.amounts_cents[..3].iter().map(|a| a.unwrap_or(0)).sum();
                assert_eq!(across, total, "{} adds across", row.label);
            }
        }
        let row = |label: &str| report.lines.iter().find(|l| l.label == label).unwrap().amounts_cents.clone();
        assert_eq!(row("Net Income"), [Some(30_000), Some(40_000), Some(500), Some(70_500)]);
        assert_eq!(row("Net Other Income"), [Some(0), Some(0), Some(500), Some(500)]);
        assert_eq!(row("6300 Rent & Lease"), [Some(30_000), Some(0), Some(0), Some(30_000)]);
    }

    #[tokio::test]
    async fn statements_tie_out_with_subaccounts_and_retained_earnings() {
        let pool = test_pool().await;
        let n = |s| account_id(&pool, s);
        let (checking, card, owner, services, cogs, rent, utilities, interest) =
            (n("1010").await, n("2100").await, n("3100").await, n("4100").await, n("5000").await, n("6300").await, n("6500").await, n("7000").await);
        let electric: i64 = sqlx::query_scalar(
            "INSERT INTO chart_of_accounts (account_number, name, account_type, parent_id) VALUES ('6510', 'Electric', 'EXPENSE', ?1) RETURNING id",
        )
        .bind(utilities)
        .fetch_one(&pool)
        .await
        .unwrap();

        // Prior year: invest 10,000 and earn 2,000 of service income.
        post(&pool, ymd(2025, 3, 1), checking, owner, 1_000_000).await;
        post(&pool, ymd(2025, 6, 1), checking, services, 200_000).await;
        // Current year activity.
        post(&pool, ymd(2026, 2, 1), checking, services, 500_000).await;
        post(&pool, ymd(2026, 2, 5), cogs, checking, 50_000).await;
        post(&pool, ymd(2026, 2, 10), rent, checking, 120_000).await;
        post(&pool, ymd(2026, 2, 12), utilities, card, 10_000).await;
        post(&pool, ymd(2026, 2, 13), electric, card, 7_500).await;
        post(&pool, ymd(2026, 2, 20), checking, interest, 1_234).await;

        let pl = profit_and_loss(&pool, ymd(2026, 1, 1), ymd(2026, 12, 31)).await.unwrap();
        assert_eq!(pl.total_revenue_cents, 501_234);
        assert_eq!(pl.total_expenses_cents, 187_500);
        assert_eq!(pl.gross_profit_cents, 450_000);
        assert_eq!(pl.net_income_cents, 313_734);
        assert_eq!(pl.net_income_display, "$3,137.34");
        let net = pl.lines.last().unwrap();
        assert_eq!((net.kind, net.label.as_str(), net.amount_cents), (LineKind::Net, "Net Income", Some(313_734)));
        // Subaccount nests under its parent with a parent total.
        assert_eq!(find(&pl.lines, "6510 Electric").depth, 2);
        assert_eq!(find(&pl.lines, "6500 Utilities").amount_cents, Some(10_000));
        assert_eq!(find(&pl.lines, "Total 6500 Utilities").amount_cents, Some(17_500));
        assert_eq!(find(&pl.lines, "Total Other Income").amount_cents, Some(1_234));

        let bs = balance_sheet(&pool, ymd(2026, 6, 30)).await.unwrap();
        assert!(bs.is_balanced, "difference {}", bs.difference_cents);
        assert_eq!(bs.total_assets_cents, 1_531_234);
        assert_eq!(bs.total_liabilities_cents, 17_500);
        assert_eq!(bs.net_income_cents, 313_734);
        assert_eq!(bs.total_equity_cents, 1_000_000 + 200_000 + 313_734);
        assert_eq!(find(&bs.lines, "3900 Retained Earnings").amount_cents, Some(200_000));
        assert_eq!(find(&bs.lines, "TOTAL ASSETS").kind, LineKind::Net);
        assert_eq!(find(&bs.lines, "TOTAL LIABILITIES AND EQUITY").amount_cents, Some(1_531_234));

        // As of the prior year end, everything is still current-year income.
        let prior = balance_sheet(&pool, ymd(2025, 12, 31)).await.unwrap();
        assert!(prior.is_balanced);
        assert_eq!(prior.net_income_cents, 200_000);
        assert!(prior.lines.iter().all(|l| l.label != "3900 Retained Earnings"));
    }

    #[tokio::test]
    async fn balance_sheet_closes_net_income_at_the_configured_fiscal_year() {
        let pool = test_pool().await;
        let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
        post(&pool, ymd(2026, 3, 1), checking, services, 100_000).await; // fiscal 2025-26 when FY starts in July
        post(&pool, ymd(2026, 8, 1), checking, services, 40_000).await; // fiscal 2026-27

        let calendar = balance_sheet(&pool, ymd(2026, 10, 13)).await.unwrap();
        assert_eq!((calendar.fiscal_year_start, calendar.net_income_cents), (ymd(2026, 1, 1), 140_000));

        crate::preferences::set(&pool, "fiscal_year_start", "07-01").await.unwrap();
        crate::preferences::set(&pool, "currency_symbol", "€").await.unwrap();
        let july = balance_sheet(&pool, ymd(2026, 10, 13)).await.unwrap();
        assert!(july.is_balanced);
        assert_eq!((july.fiscal_year_start, july.net_income_cents), (ymd(2026, 7, 1), 40_000));
        assert_eq!(find(&july.lines, "3900 Retained Earnings").amount_cents, Some(100_000));
        assert_eq!(july.currency_symbol, "€");
        let pl = profit_and_loss(&pool, ymd(2026, 7, 1), ymd(2026, 10, 13)).await.unwrap();
        assert_eq!(pl.net_income_display, "€400.00");
    }

    #[tokio::test]
    async fn empty_books_still_render_and_balance() {
        let pool = test_pool().await;
        let pl = profit_and_loss(&pool, ymd(2026, 1, 1), ymd(2026, 1, 31)).await.unwrap();
        assert_eq!(pl.net_income_display, "$0.00");
        assert!(pl.lines.iter().any(|l| l.label == "Total Income"));
        let bs = balance_sheet(&pool, ymd(2026, 1, 31)).await.unwrap();
        assert!(bs.is_balanced);
        assert!(profit_and_loss(&pool, ymd(2026, 2, 1), ymd(2026, 1, 1)).await.is_err());
    }
}
