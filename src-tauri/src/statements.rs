//! Financial statements: Profit & Loss for a date range and Balance Sheet as of a date.
//!
//! Each statement is returned as a flat list of typed lines (section heading, account, subtotal,
//! total, net) carrying an indent depth, so the UI only applies typography. Subaccounts nest under
//! their parent with a "Total <parent>" line, as classic accounting software does. The fiscal year boundary (which
//! splits current-year Net Income from Retained Earnings) comes from the `fiscal_year_start`
//! preference.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, format_money, mul_div_round};
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

/// Report filter by class: the chosen classes (each with all of its subclasses), and optionally the
/// lines that have no class.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassFilter {
    #[serde(default)]
    pub class_ids: Vec<i64>,
    #[serde(default)]
    pub unclassified: bool,
}

/// "Class: East, West" or "Class: Unclassified", for report subtitles and exports.
pub async fn class_filter_label(pool: &SqlitePool, filter: &ClassFilter) -> Result<String, String> {
    let mut names: Vec<String> = Vec::new();
    for id in &filter.class_ids {
        let name: Option<String> = sqlx::query_scalar("SELECT full_name FROM v_classes WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?;
        names.push(name.ok_or_else(|| format!("CLASS_NOT_FOUND: class {id} does not exist"))?);
    }
    if filter.unclassified {
        names.push("Unclassified".into());
    }
    Ok(format!("Class: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") }))
}

/// Net debit movement per account (debits minus credits) over posted entries in `[from, to]`.
async fn debit_net(pool: &SqlitePool, from: Option<NaiveDate>, to: NaiveDate) -> Result<HashMap<i64, i64>, String> {
    debit_net_filtered(pool, from, to, None).await
}

async fn debit_net_filtered(pool: &SqlitePool, from: Option<NaiveDate>, to: NaiveDate, filter: Option<&ClassFilter>) -> Result<HashMap<i64, i64>, String> {
    let ids = filter.map(|f| serde_json::to_string(&f.class_ids).unwrap_or_else(|_| "[]".into()));
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT account_id, SUM(debit_cents) - SUM(credit_cents)
         FROM v_posted_lines
         WHERE (?1 IS NULL OR entry_date >= ?1) AND entry_date <= ?2
           AND (?3 IS NULL
                OR class_id IN (SELECT a.class_id FROM v_class_ancestry a JOIN json_each(?3) j ON j.value = a.ancestor_id)
                OR (?4 AND class_id IS NULL))
         GROUP BY account_id",
    )
    .bind(from)
    .bind(to)
    .bind(ids)
    .bind(filter.is_some_and(|f| f.unclassified))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

const PL_TYPES: &[&str] = &["INCOME", "COST_OF_GOODS_SOLD", "EXPENSE", "OTHER_INCOME", "OTHER_EXPENSE"];

/// Income and expense movement for a period on either basis.
async fn pl_movement(
    pool: &SqlitePool,
    chart: &Chart,
    start: NaiveDate,
    end: NaiveDate,
    filter: Option<&ClassFilter>,
    basis: Basis,
) -> Result<HashMap<i64, i64>, String> {
    match basis {
        Basis::Accrual => debit_net_filtered(pool, Some(start), end, filter).await,
        Basis::Cash => cash_basis_movement(pool, chart, start, end, filter).await,
    }
}

/// Journal entries whose income and expense are recognized only as they are settled on a cash
/// basis: invoices, bills, credit memos kept as credit, vendor credits, and the voids of those.
const DEFERRED_ENTRIES: &str = "WITH deferred AS (
        SELECT e.id FROM journal_entries e
        WHERE e.source_type IN ('INVOICE', 'BILL', 'VENDOR_CREDIT')
           OR (e.source_type = 'CREDIT_MEMO' AND (SELECT refund_account_id FROM credit_memos WHERE id = e.source_id) IS NULL)
    )";

/// Cash-basis movement: everything paid at the time (sales receipts, expenses, checks, deposits,
/// journal entries, payment FX differences) counts on its date as on the accrual basis; an
/// invoice, bill or credit counts in proportion to how much of it was settled in the period, on
/// the settlement dates (payments, and credits applied or refunded). Two documents settled against
/// each other with a credit cancel out, as no cash moved.
async fn cash_basis_movement(pool: &SqlitePool, chart: &Chart, start: NaiveDate, end: NaiveDate, filter: Option<&ClassFilter>) -> Result<HashMap<i64, i64>, String> {
    let ids = filter.map(|f| serde_json::to_string(&f.class_ids).unwrap_or_else(|_| "[]".into()));
    let unclassified = filter.is_some_and(|f| f.unclassified);
    let class_clause = "(?3 IS NULL
                OR l.class_id IN (SELECT a.class_id FROM v_class_ancestry a JOIN json_each(?3) j ON j.value = a.ancestor_id)
                OR (?4 AND l.class_id IS NULL))";
    let immediate: Vec<(i64, i64)> = sqlx::query_as(&format!(
        "{DEFERRED_ENTRIES}
         SELECT l.account_id, SUM(l.debit_cents) - SUM(l.credit_cents)
         FROM v_posted_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
         WHERE l.entry_date BETWEEN ?1 AND ?2 AND {class_clause}
           AND e.id NOT IN (SELECT id FROM deferred)
           AND (e.reverses_entry_id IS NULL OR e.reverses_entry_id NOT IN (SELECT id FROM deferred))
         GROUP BY l.account_id"
    ))
    .bind(start)
    .bind(end)
    .bind(&ids)
    .bind(unclassified)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut movement: HashMap<i64, i64> = immediate.into_iter().collect();

    // (document's journal entry, document total, amount settled in the period)
    let settled: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT * FROM (
            SELECT i.journal_entry_id, COALESCE(i.home_total_cents, i.total_cents),
                   COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents)) FROM invoice_payments p
                             WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2), 0)
                 + COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a
                             WHERE a.invoice_id = i.id AND a.is_voided = 0 AND a.applied_date BETWEEN ?1 AND ?2), 0) AS settled
            FROM invoices i WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL
            UNION ALL
            SELECT b.journal_entry_id, b.total_cents,
                   COALESCE((SELECT SUM(p.amount_cents) FROM bill_payments p WHERE p.bill_id = b.id AND p.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2), 0)
                 + COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a
                             WHERE a.bill_id = b.id AND a.is_voided = 0 AND a.applied_date BETWEEN ?1 AND ?2), 0)
            FROM bills b WHERE b.is_voided = 0 AND b.journal_entry_id IS NOT NULL
            UNION ALL
            SELECT m.journal_entry_id, m.total_cents,
                   COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a
                             WHERE a.credit_memo_id = m.id AND a.is_voided = 0 AND a.applied_date BETWEEN ?1 AND ?2), 0)
                 + COALESCE((SELECT SUM(r.amount_cents) FROM credit_refunds r JOIN journal_entries re ON re.id = r.journal_entry_id
                             WHERE r.credit_memo_id = m.id AND re.is_voided = 0 AND r.refund_date BETWEEN ?1 AND ?2), 0)
            FROM credit_memos m WHERE m.is_voided = 0 AND m.refund_account_id IS NULL AND m.journal_entry_id IS NOT NULL
            UNION ALL
            SELECT c.journal_entry_id, c.total_cents,
                   COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a
                             WHERE a.vendor_credit_id = c.id AND a.is_voided = 0 AND a.applied_date BETWEEN ?1 AND ?2), 0)
                 + COALESCE((SELECT SUM(r.amount_cents) FROM vendor_credit_refunds r JOIN journal_entries re ON re.id = r.journal_entry_id
                             WHERE r.vendor_credit_id = c.id AND re.is_voided = 0 AND r.refund_date BETWEEN ?1 AND ?2), 0)
            FROM vendor_credits c WHERE c.is_voided = 0 AND c.journal_entry_id IS NOT NULL
         ) WHERE settled > 0",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let pl_accounts: std::collections::HashSet<i64> =
        chart.accounts.iter().filter(|a| PL_TYPES.contains(&a.account_type.as_str())).map(|a| a.id).collect();
    for (entry_id, total, paid) in settled {
        let lines: Vec<(i64, i64)> = sqlx::query_as(&format!(
            "SELECT l.account_id, l.debit_cents - l.credit_cents FROM journal_lines l WHERE l.journal_entry_id = ?5 AND {class_clause}"
        ))
        .bind(start)
        .bind(end)
        .bind(&ids)
        .bind(unclassified)
        .bind(entry_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        for (account, net) in lines.into_iter().filter(|(a, _)| pl_accounts.contains(a)) {
            let share = mul_div_round(net, paid.min(total), total).ok_or("RPT_OVERFLOW: amount out of range")?;
            *movement.entry(account).or_insert(0) += share;
        }
    }
    Ok(movement)
}

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
    profit_and_loss_filtered(pool, start_date, end_date, None).await
}

/// Profit & Loss limited to the lines of the classes in `filter` (all lines when `None`).
pub async fn profit_and_loss_filtered(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate, filter: Option<&ClassFilter>) -> Result<ProfitAndLoss, String> {
    profit_and_loss_basis(pool, start_date, end_date, filter, Basis::Accrual).await
}

/// Accrual (income when invoiced, expenses when billed) or cash (when paid).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Basis {
    #[default]
    Accrual,
    Cash,
}

/// Profit & Loss on either basis, optionally limited to some classes.
pub async fn profit_and_loss_basis(
    pool: &SqlitePool,
    start_date: NaiveDate,
    end_date: NaiveDate,
    filter: Option<&ClassFilter>,
    basis: Basis,
) -> Result<ProfitAndLoss, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let chart = Chart::load(pool).await?;
    let movement = pl_movement(pool, &chart, start_date, end_date, filter, basis).await?;
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
    /// The class the column is for; `None` for Unclassified and Total.
    pub class_id: Option<i64>,
    pub label: String,
    /// `class` (a class with no subclass columns), `other` (a parent's own lines, shown beside
    /// its subclasses), `subtotal` (a parent with all its subclasses), `unclassified` or `total`.
    pub kind: &'static str,
    /// Nesting of the class (0 for top-level classes, Unclassified and Total).
    pub depth: u32,
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

    let classes: Vec<(i64, Option<i64>, String, String)> =
        sqlx::query_as("SELECT id, parent_id, name, full_name FROM v_classes ORDER BY replace(lower(full_name), ':', char(1))")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    // Each column adds up the movement of these classes (None = unclassified lines).
    let mut columns: Vec<(ClassColumn, Vec<Option<i64>>)> = Vec::new();
    for (id, parent, _, _) in &classes {
        if parent.is_none() {
            class_columns(&classes, &by_class, *id, 0, &mut columns);
        }
    }
    if by_class.contains_key(&None) {
        columns.push((ClassColumn { class_id: None, label: "Unclassified".into(), kind: "unclassified", depth: 0, is_total: false }, vec![None]));
    }

    let total_natural = chart.natural(&total);
    let totals = build_pl(Builder::new(&chart, total_natural.clone()), None);
    let bodies: Vec<PlBody> = columns
        .iter()
        .map(|(_, members)| {
            let mut movement: HashMap<i64, i64> = HashMap::new();
            for member in members {
                for (account, net) in by_class.get(member).into_iter().flatten() {
                    *movement.entry(*account).or_insert(0) += net;
                }
            }
            let b = Builder { chart: &chart, shape: total_natural.clone(), natural: chart.natural(&movement), lines: Vec::new() };
            build_pl(b, Some(totals.has_net_other))
        })
        .collect();
    let mut columns: Vec<ClassColumn> = columns.into_iter().map(|(c, _)| c).collect();

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
    columns.push(ClassColumn { class_id: None, label: "Total".into(), kind: "total", depth: 0, is_total: true });

    Ok(ProfitAndLossByClass { start_date, end_date, columns, lines, currency_symbol: currency_symbol(pool).await? })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatedTrialBalance {
    pub as_of: NaiveDate,
    pub fiscal_year_start: NaiveDate,
    pub rows: Vec<crate::reports::TrialBalanceRow>,
    pub total_debit_cents: i64,
    pub total_credit_cents: i64,
}

/// Trial balance as of a date: balance sheet accounts as of that date, income and expense accounts
/// for the fiscal year to date, and earlier years' profit in Retained Earnings, as the desktop
/// report shows it. Debits always equal credits.
pub async fn trial_balance_as_of(pool: &SqlitePool, as_of: NaiveDate) -> Result<DatedTrialBalance, String> {
    let chart = Chart::load(pool).await?;
    let (fiscal_start, _) = fiscal_year_bounds(as_of, fiscal_start_month(pool).await?);
    let cumulative = debit_net(pool, None, as_of).await?;
    let prior = match fiscal_start.pred_opt() {
        Some(day_before) => debit_net(pool, None, day_before).await?,
        None => HashMap::new(),
    };
    let prior_net_income = net_income(&chart, &prior);
    let order: Vec<(i64, Option<String>, String, String)> = sqlx::query_as(
        "SELECT id, account_number, full_name, account_type_name FROM v_chart_of_accounts ORDER BY type_sort_order, COALESCE(account_number, ''), full_name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let retained = chart.accounts.iter().find(|a| a.is_system && a.account_type == "EQUITY" && a.name == "Retained Earnings").map(|a| a.id);
    let is_pl: HashMap<i64, bool> = chart.accounts.iter().map(|a| (a.id, PL_TYPES.contains(&a.account_type.as_str()))).collect();

    let mut tb = DatedTrialBalance { as_of, fiscal_year_start: fiscal_start, rows: Vec::new(), total_debit_cents: 0, total_credit_cents: 0 };
    for (account_id, account_number, full_name, account_type_name) in order {
        let mut net = cumulative.get(&account_id).copied().unwrap_or(0);
        if is_pl.get(&account_id).copied().unwrap_or(false) {
            net -= prior.get(&account_id).copied().unwrap_or(0);
        }
        if Some(account_id) == retained {
            net -= prior_net_income; // profit is a credit
        }
        if net == 0 {
            continue;
        }
        let (debit_cents, credit_cents) = if net > 0 { (net, 0) } else { (0, -net) };
        tb.total_debit_cents += debit_cents;
        tb.total_credit_cents += credit_cents;
        tb.rows.push(crate::reports::TrialBalanceRow { account_id, account_number, full_name, account_type_name, debit_cents, credit_cents });
    }
    Ok(tb)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashFlowStatement {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub lines: Vec<ReportLine>,
    pub net_change_cents: i64,
    pub cash_start_cents: i64,
    pub cash_end_cents: i64,
    pub currency_symbol: String,
}

/// Statement of Cash Flows by the indirect method. Cash is the Bank accounts. Net income is
/// adjusted by the change in every other current asset and liability (operating), fixed and other
/// assets (investing), and long-term liabilities and equity (financing), so the net change always
/// equals the change in bank balances.
pub async fn cash_flow(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<CashFlowStatement, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let chart = Chart::load(pool).await?;
    let movement = debit_net(pool, Some(start_date), end_date).await?;
    let before = match start_date.pred_opt() {
        Some(day) => debit_net(pool, None, day).await?,
        None => HashMap::new(),
    };
    let net = net_income(&chart, &movement);
    let mut lines: Vec<ReportLine> = Vec::new();
    let mut push = |kind, label: String, depth, amount: Option<i64>, account_id| lines.push(ReportLine { kind, label, depth, amount_cents: amount, account_id });
    // Cash goes up when an asset goes down or a liability or equity account goes up.
    let section = |types: &[&str]| -> Vec<(i64, String, i64)> {
        chart
            .accounts
            .iter()
            .filter(|a| types.contains(&a.account_type.as_str()))
            .filter_map(|a| movement.get(&a.id).filter(|m| **m != 0).map(|m| (a.id, a.label.clone(), -m)))
            .collect()
    };
    let operating = section(&["ACCOUNTS_RECEIVABLE", "OTHER_CURRENT_ASSET", "ACCOUNTS_PAYABLE", "CREDIT_CARD", "OTHER_CURRENT_LIABILITY"]);
    let investing = section(&["FIXED_ASSET", "OTHER_ASSET"]);
    let financing = section(&["LONG_TERM_LIABILITY", "EQUITY"]);

    push(LineKind::Section, "OPERATING ACTIVITIES".into(), 0, None, None);
    push(LineKind::Account, "Net Income".into(), 1, Some(net), None);
    let mut operating_total = net;
    if !operating.is_empty() {
        push(LineKind::Section, "Adjustments to reconcile Net Income to net cash provided by operations".into(), 1, None, None);
        for (id, label, amount) in &operating {
            push(LineKind::Account, label.clone(), 2, Some(*amount), Some(*id));
            operating_total += amount;
        }
    }
    push(LineKind::Total, "Net cash provided by operating activities".into(), 0, Some(operating_total), None);
    let mut group = |title: &str, total_label: &str, rows: &[(i64, String, i64)]| -> i64 {
        if rows.is_empty() {
            return 0;
        }
        push(LineKind::Section, title.into(), 0, None, None);
        let total: i64 = rows.iter().map(|r| r.2).sum();
        for (id, label, amount) in rows {
            push(LineKind::Account, label.clone(), 1, Some(*amount), Some(*id));
        }
        push(LineKind::Total, total_label.into(), 0, Some(total), None);
        total
    };
    let investing_total = group("INVESTING ACTIVITIES", "Net cash provided by investing activities", &investing);
    let financing_total = group("FINANCING ACTIVITIES", "Net cash provided by financing activities", &financing);
    let change = operating_total + investing_total + financing_total;
    let is_cash = |a: &&Account| a.account_type == "BANK";
    let cash_start: i64 = chart.accounts.iter().filter(is_cash).filter_map(|a| before.get(&a.id)).sum();
    let cash_change: i64 = chart.accounts.iter().filter(is_cash).filter_map(|a| movement.get(&a.id)).sum();
    debug_assert_eq!(change, cash_change, "the statement ties to the bank accounts");
    push(LineKind::Net, "Net cash increase for period".into(), 0, Some(change), None);
    push(LineKind::Account, "Cash at beginning of period".into(), 0, Some(cash_start), None);
    push(LineKind::Net, "Cash at end of period".into(), 0, Some(cash_start + cash_change), None);
    Ok(CashFlowStatement {
        start_date,
        end_date,
        lines,
        net_change_cents: change,
        cash_start_cents: cash_start,
        cash_end_cents: cash_start + cash_change,
        currency_symbol: currency_symbol(pool).await?,
    })
}

// ------------------------------------------------------------------------------------------------
// Budgets
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub id: i64,
    pub name: String,
    pub fiscal_year_start: NaiveDate,
}

/// One budget amount: income or expense expected for an account in a month of the fiscal year
/// (period 1 = the first month), on the account's normal side (income and expenses positive).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BudgetAmount {
    pub account_id: i64,
    pub period: i64,
    pub amount_cents: i64,
}

pub async fn list_budgets(pool: &SqlitePool) -> Result<Vec<BudgetView>, String> {
    sqlx::query_as("SELECT id, name, fiscal_year_start FROM budgets ORDER BY fiscal_year_start DESC, name COLLATE NOCASE").fetch_all(pool).await.map_err(db_err)
}

/// Creates a budget for the fiscal year containing `in_year`.
pub async fn create_budget(pool: &SqlitePool, name: &str, in_year: NaiveDate) -> Result<BudgetView, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("BUDGET_NAME_REQUIRED: name the budget".into());
    }
    let (start, _) = fiscal_year_bounds(in_year, fiscal_start_month(pool).await?);
    let id: i64 = sqlx::query_scalar("INSERT INTO budgets (name, fiscal_year_start) VALUES (?1, ?2) RETURNING id")
        .bind(name)
        .bind(start)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") { format!("BUDGET_DUPLICATE: there is already a budget named '{name}'") } else { msg }
        })?;
    Ok(BudgetView { id, name: name.into(), fiscal_year_start: start })
}

pub async fn delete_budget(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM budgets WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

pub async fn budget_amounts(pool: &SqlitePool, id: i64) -> Result<Vec<BudgetAmount>, String> {
    sqlx::query_as("SELECT account_id, period, amount_cents FROM budget_amounts WHERE budget_id = ?1").bind(id).fetch_all(pool).await.map_err(db_err)
}

/// Saves budget amounts (zero removes one). Only income and expense accounts are budgeted.
pub async fn set_budget_amounts(pool: &SqlitePool, id: i64, amounts: &[BudgetAmount]) -> Result<(), String> {
    let chart = Chart::load(pool).await?;
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    for a in amounts {
        let pl = chart.accounts.iter().any(|acct| acct.id == a.account_id && PL_TYPES.contains(&acct.account_type.as_str()));
        if !pl || !(1..=12).contains(&a.period) {
            let _ = tx.rollback().await;
            return Err("BUDGET_BAD_AMOUNT: budget income and expense accounts by month (1 to 12)".into());
        }
        let sql = if a.amount_cents == 0 {
            "DELETE FROM budget_amounts WHERE budget_id = ?1 AND account_id = ?2 AND period = ?3"
        } else {
            "INSERT INTO budget_amounts (budget_id, account_id, period, amount_cents) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (budget_id, account_id, period) DO UPDATE SET amount_cents = excluded.amount_cents"
        };
        if let Err(e) = sqlx::query(sql).bind(id).bind(a.account_id).bind(a.period).bind(a.amount_cents).execute(&mut *tx).await {
            let _ = tx.rollback().await;
            return Err(db_err(e));
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Budget vs. Actual: actual income and expenses for fiscal months `from_period`..=`to_period` of
/// the budget's year beside the budget, with the amount over budget and the percent of budget
/// (basis points: 112.5% = 11250).
pub async fn budget_vs_actual(pool: &SqlitePool, budget_id: i64, from_period: i64, to_period: i64) -> Result<ProfitAndLossByClass, String> {
    if !(1..=12).contains(&from_period) || !(from_period..=12).contains(&to_period) {
        return Err("RPT_BAD_RANGE: choose months 1 to 12 of the fiscal year".into());
    }
    let start: NaiveDate = sqlx::query_scalar("SELECT fiscal_year_start FROM budgets WHERE id = ?1")
        .bind(budget_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or("BUDGET_NOT_FOUND: that budget does not exist")?;
    let from = start.checked_add_months(chrono::Months::new(from_period as u32 - 1)).ok_or("RPT_BAD_RANGE: date out of range")?;
    let end = start.checked_add_months(chrono::Months::new(to_period as u32)).and_then(|d| d.pred_opt()).ok_or("RPT_BAD_RANGE: date out of range")?;
    let chart = Chart::load(pool).await?;
    let actual = debit_net(pool, Some(from), end).await?;
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT account_id, SUM(amount_cents) FROM budget_amounts WHERE budget_id = ?1 AND period BETWEEN ?2 AND ?3 GROUP BY account_id",
    )
    .bind(budget_id)
    .bind(from_period)
    .bind(to_period)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    // Budgets are entered on the natural side; the builder works from debit-net movement.
    let budget: HashMap<i64, i64> = rows
        .into_iter()
        .filter_map(|(id, natural)| chart.accounts.iter().find(|a| a.id == id).map(|a| (id, if a.is_debit_normal { natural } else { -natural })))
        .collect();
    let mut union: HashMap<i64, i64> = HashMap::new();
    for m in [&actual, &budget] {
        for (id, net) in m.iter() {
            if PL_TYPES.contains(&chart.accounts.iter().find(|a| a.id == *id).map_or("", |a| a.account_type.as_str())) {
                *union.entry(*id).or_insert(0) += net.abs();
            }
        }
    }
    let shape = chart.natural(&union);
    let shaped = build_pl(Builder::new(&chart, shape.clone()), None);
    let columns_of = |m: &HashMap<i64, i64>| build_pl(Builder { chart: &chart, shape: shape.clone(), natural: chart.natural(m), lines: Vec::new() }, Some(shaped.has_net_other));
    let (a, b) = (columns_of(&actual), columns_of(&budget));
    let lines = shaped
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let (act, bud) = (a.lines[i].amount_cents, b.lines[i].amount_cents);
            let over = act.zip(bud).map(|(x, y)| x - y);
            let pct = act.zip(bud).and_then(|(x, y)| if y == 0 { None } else { mul_div_round(x, 10_000, y) });
            ClassReportLine { kind: line.kind, label: line.label.clone(), depth: line.depth, account_id: line.account_id, amounts_cents: vec![act, bud, over, pct] }
        })
        .collect();
    let column = |label: &str, kind| ClassColumn { class_id: None, label: label.into(), kind, depth: 0, is_total: false };
    Ok(ProfitAndLossByClass {
        start_date: from,
        end_date: end,
        columns: vec![column("Actual", "period"), column("Budget", "period"), column("$ Over Budget", "change"), column("% of Budget", "percent")],
        lines,
        currency_symbol: currency_symbol(pool).await?,
    })
}

/// One column of a multi-period Profit & Loss.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodColumn {
    pub label: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodReportInput {
    pub columns: Vec<PeriodColumn>,
    /// Adds a Total column (the sum of the period columns).
    #[serde(default)]
    pub total: bool,
    /// Adds "$ Change" and "% Change" columns comparing the first column with the second.
    #[serde(default)]
    pub compare: bool,
    #[serde(default)]
    pub basis: Basis,
    #[serde(default)]
    pub class_filter: Option<ClassFilter>,
}

/// Profit & Loss with a column per period (months, quarters, or this period against an earlier
/// one), every column with the same rows. `% Change` amounts are basis points (12.5% = 1250).
pub async fn profit_and_loss_periods(pool: &SqlitePool, input: &PeriodReportInput) -> Result<ProfitAndLossByClass, String> {
    if input.columns.is_empty() || input.columns.len() > 36 {
        return Err("RPT_BAD_COLUMNS: choose between 1 and 36 periods".into());
    }
    if input.columns.iter().any(|c| c.end_date < c.start_date) {
        return Err("RPT_BAD_RANGE: a period ends before it starts".into());
    }
    if input.compare && input.columns.len() != 2 {
        return Err("RPT_BAD_COMPARE: a comparison needs exactly two periods".into());
    }
    let chart = Chart::load(pool).await?;
    let mut movements = Vec::with_capacity(input.columns.len());
    for c in &input.columns {
        movements.push(pl_movement(pool, &chart, c.start_date, c.end_date, input.class_filter.as_ref(), input.basis).await?);
    }
    // Rows come from every period together, so each column lines up.
    let mut union: HashMap<i64, i64> = HashMap::new();
    for m in &movements {
        for (account, net) in m {
            *union.entry(*account).or_insert(0) += net.abs();
        }
    }
    let shape = chart.natural(&union);
    let shaped = build_pl(Builder::new(&chart, shape.clone()), None);
    let bodies: Vec<PlBody> = movements
        .iter()
        .map(|m| build_pl(Builder { chart: &chart, shape: shape.clone(), natural: chart.natural(m), lines: Vec::new() }, Some(shaped.has_net_other)))
        .collect();
    let mut columns: Vec<ClassColumn> =
        input.columns.iter().map(|c| ClassColumn { class_id: None, label: c.label.clone(), kind: "period", depth: 0, is_total: false }).collect();
    if input.total {
        columns.push(ClassColumn { class_id: None, label: "Total".into(), kind: "total", depth: 0, is_total: true });
    }
    if input.compare {
        columns.push(ClassColumn { class_id: None, label: "$ Change".into(), kind: "change", depth: 0, is_total: false });
        columns.push(ClassColumn { class_id: None, label: "% Change".into(), kind: "percent", depth: 0, is_total: false });
    }
    let lines = shaped
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let mut amounts: Vec<Option<i64>> = bodies.iter().map(|b| b.lines[i].amount_cents).collect();
            if input.total {
                amounts.push(line.amount_cents.map(|_| amounts.iter().map(|a| a.unwrap_or(0)).sum()));
            }
            if input.compare {
                let (current, previous) = (amounts[0], amounts[1]);
                amounts.push(current.zip(previous).map(|(c, p)| c - p));
                amounts.push(current.zip(previous).and_then(|(c, p)| if p == 0 { None } else { mul_div_round(c - p, 10_000, p.abs()) }));
            }
            ClassReportLine { kind: line.kind, label: line.label.clone(), depth: line.depth, account_id: line.account_id, amounts_cents: amounts }
        })
        .collect();
    Ok(ProfitAndLossByClass {
        start_date: input.columns.iter().map(|c| c.start_date).min().unwrap(),
        end_date: input.columns.iter().map(|c| c.end_date).max().unwrap(),
        columns,
        lines,
        currency_symbol: currency_symbol(pool).await?,
    })
}

/// The columns for class `id` and its subclasses, as classic desktop software lays them out: a class
/// whose subclasses have activity shows a column per subclass, then "<class> (other)" for lines
/// tagged with the class itself, then "Total <class>"; any other class with activity is one column.
fn class_columns(
    classes: &[(i64, Option<i64>, String, String)],
    by_class: &HashMap<Option<i64>, HashMap<i64, i64>>,
    id: i64,
    depth: u32,
    out: &mut Vec<(ClassColumn, Vec<Option<i64>>)>,
) -> Vec<Option<i64>> {
    let (_, _, name, full_name) = classes.iter().find(|c| c.0 == id).expect("class exists");
    let start = out.len();
    let mut subtree: Vec<Option<i64>> = Vec::new();
    for (child, parent, _, _) in classes {
        if *parent == Some(id) {
            subtree.extend(class_columns(classes, by_class, *child, depth + 1, out));
        }
    }
    let own = by_class.contains_key(&Some(id));
    let column = |label: String, kind| ClassColumn { class_id: Some(id), label, kind, depth, is_total: false };
    if out.len() == start {
        // No subclass has activity: one column, if the class itself has any.
        if own {
            out.push((column(full_name.clone(), "class"), vec![Some(id)]));
            return vec![Some(id)];
        }
        return Vec::new();
    }
    if own {
        out.push((column(format!("{name} (other)"), "other"), vec![Some(id)]));
        subtree.push(Some(id));
    }
    out.push((column(format!("Total {full_name}"), "subtotal"), subtree.clone()));
    subtree
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
mod tests;
