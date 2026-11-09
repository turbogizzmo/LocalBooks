//! Reports that are not presets of the custom report engine: the list reports (phone and contact
//! lists, the item and account listings), summaries reshaped from the financial statements (Balance
//! Sheet Summary, Profit & Loss YTD Comparison), and reports over records the ledger engine does not
//! read (time, purchase orders, inventory counts, the audit trail, days to pay).
//!
//! Every report comes back in the engine's shape ([`CustomReport`] with notes, as the Accountant &
//! Taxes reports do), so the report screen shows, prints and exports it the same way. Nothing here
//! writes. Money is integer cents; hours are a quantity scaled by 10,000.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{self, type_label, ColumnHead, ColumnKind, CustomReport, NameKind, ReportRow, RowKind};
use crate::ledger::{db_err, mul_div_round, QUANTITY_SCALE};
use crate::preferences::{fiscal_start_month, fiscal_year_bounds};
use crate::statements::LineKind;

mod profitability;

// ------------------------------------------------------------------------------------------------
// Catalog
// ------------------------------------------------------------------------------------------------

/// Which dates a report asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Dates {
    /// None: the list as it is now.
    None,
    /// One date the report is figured as of.
    AsOf,
    /// From and to, starting at the report's `period`.
    Range,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReportInfo {
    pub id: &'static str,
    pub title: &'static str,
    /// The Reports menu group it is listed under.
    pub group: &'static str,
    pub dates: Dates,
    /// The report period a range report opens with (an id from `ReportParts.jsx`'s `PERIODS`).
    pub period: &'static str,
}

/// The Reports menu group for the list reports.
pub const LISTS: &str = "Lists";

/// Every report this module runs, in menu order within each group.
pub fn catalog() -> Vec<ListReportInfo> {
    let r = |id, title, group, dates, period| ListReportInfo { id, title, group, dates, period };
    use Dates::{AsOf, None as Now, Range};
    vec![
        r("bs-summary", "Balance Sheet Summary", "Business overview", AsOf, ""),
        r("pl-ytd", "Profit & Loss YTD Comparison", "Business overview", Range, "thisMonth"),
        r("average-days-to-pay", "Average Days to Pay Summary", "Who owes you", Range, "fiscalYearToDate"),
        r("invoice-profit-customer", "Invoice Profitability by Customer", "Sales and customers", Range, "fiscalYearToDate"),
        r("invoice-profit-detail", "Invoice Profitability Detail", "Sales and customers", Range, "fiscalYearToDate"),
        r("unpaid-bills-job", "Unpaid Bills by Job", "What you owe", AsOf, ""),
        r("open-po-detail", "Open Purchase Orders Detail", "Expenses and vendors", AsOf, ""),
        r("time-job-summary", "Time by Job Summary", "Jobs", Range, "thisMonth"),
        r("time-job-detail", "Time by Job Detail", "Jobs", Range, "thisMonth"),
        r("time-name", "Time by Name", "Jobs", Range, "thisMonth"),
        r("time-item", "Time by Item", "Jobs", Range, "thisMonth"),
        r("physical-inventory", "Physical Inventory Worksheet", "Inventory", Now, ""),
        r("inventory-turnover", "Inventory Turnover by Item", "Inventory", Range, "fiscalYearToDate"),
        r("audit-trail", "Audit Trail", custom_reports::ACCOUNTANT, Range, "thisMonth"),
        r("customer-phone", "Customer Phone List", LISTS, Now, ""),
        r("customer-contact", "Customer Contact List", LISTS, Now, ""),
        r("vendor-phone", "Vendor Phone List", LISTS, Now, ""),
        r("vendor-contact", "Vendor Contact List", LISTS, Now, ""),
        r("item-listing", "Item Listing", LISTS, Now, ""),
        r("account-listing", "Account Listing", LISTS, Now, ""),
    ]
}

/// Runs report `id`. `end` defaults to `today` (the as-of date, or the last day of a range); a
/// range report's `start` defaults to the first day of the fiscal year holding `end`.
pub async fn run(pool: &SqlitePool, id: &str, start: Option<NaiveDate>, end: Option<NaiveDate>, today: NaiveDate) -> Result<AccountantReport, String> {
    let end = end.unwrap_or(today);
    let start = match start {
        Some(s) => s,
        None => fiscal_year_bounds(end, fiscal_start_month(pool).await?).0,
    };
    if start > end {
        return Err("RPT_BAD_RANGE: the start date is after the end date".into());
    }
    match id {
        "bs-summary" => balance_sheet_summary(pool, end).await,
        "pl-ytd" => pl_ytd_comparison(pool, start, end).await,
        "average-days-to-pay" => average_days_to_pay(pool, start, end).await,
        "invoice-profit-customer" => profitability::invoice_profitability_by_customer(pool, start, end).await,
        "invoice-profit-detail" => profitability::invoice_profitability_detail(pool, start, end).await,
        "unpaid-bills-job" => unpaid_bills_by_job(pool, end).await,
        "open-po-detail" => open_po_detail(pool, end).await,
        "time-job-summary" => time_summary(pool, TimeBy::Job, start, end).await,
        "time-job-detail" => time_by_job_detail(pool, start, end).await,
        "time-name" => time_summary(pool, TimeBy::Name, start, end).await,
        "time-item" => time_summary(pool, TimeBy::Item, start, end).await,
        "physical-inventory" => physical_inventory(pool, today).await,
        "inventory-turnover" => profitability::inventory_turnover(pool, start, end).await,
        "audit-trail" => audit_trail(pool, start, end).await,
        "customer-phone" => phone_list(pool, NameKind::Customer, today).await,
        "customer-contact" => contact_list(pool, NameKind::Customer, today).await,
        "vendor-phone" => phone_list(pool, NameKind::Vendor, today).await,
        "vendor-contact" => contact_list(pool, NameKind::Vendor, today).await,
        "item-listing" => item_listing(pool, today).await,
        "account-listing" => account_listing(pool, today).await,
        _ => Err(format!("RPT_UNKNOWN: no report named {id}")),
    }
}

// ------------------------------------------------------------------------------------------------
// Building rows
// ------------------------------------------------------------------------------------------------

fn head(label: &str, kind: ColumnKind) -> ColumnHead {
    ColumnHead { label: label.into(), kind }
}

fn row(kind: RowKind, depth: u32, cells: Vec<Value>) -> ReportRow {
    ReportRow { kind, depth, cells, journal_entry_id: None, account_id: None, drill: None }
}

/// A row with `label` in the first column, `values` in the last ones and blanks between.
fn labeled(kind: RowKind, depth: u32, width: usize, label: &str, values: &[Value]) -> ReportRow {
    let mut cells = vec![json!(label)];
    cells.resize(width - values.len(), Value::Null);
    cells.extend_from_slice(values);
    row(kind, depth, cells)
}

fn section(depth: u32, width: usize, label: &str) -> ReportRow {
    labeled(RowKind::Section, depth, width, label, &[])
}

fn done(start: Option<NaiveDate>, end: NaiveDate, columns: Vec<ColumnHead>, rows: Vec<ReportRow>, line_count: usize, notes: Vec<String>) -> AccountantReport {
    AccountantReport { report: CustomReport { start_date: start, end_date: end, columns, rows, line_count }, notes }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Minutes as hours scaled by 10,000.
fn hours_e4(minutes: i64) -> i64 {
    mul_div_round(minutes, QUANTITY_SCALE, 60).unwrap_or(0)
}

fn terms_text(days: Option<i64>) -> Option<String> {
    match days {
        Some(0) => Some("Due on receipt".into()),
        Some(d) => Some(format!("Net {d}")),
        None => None,
    }
}

/// An address on one line.
fn one_line(text: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = text?.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Customers with jobs shown as `Customer:Job`, as the customer list does.
const CUSTOMER_NAMES: &str = "WITH RECURSIVE cn(id, full_name) AS (
        SELECT id, name FROM customers WHERE parent_id IS NULL
        UNION ALL SELECT c.id, cn.full_name || ':' || c.name FROM customers c JOIN cn ON c.parent_id = cn.id)";

// ------------------------------------------------------------------------------------------------
// Lists
// ------------------------------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct Party {
    id: i64,
    name: String,
    company_name: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    terms_days: Option<i64>,
    is_1099: bool,
}

/// Active customers (or vendors), by name.
async fn parties(pool: &SqlitePool, kind: NameKind) -> Result<Vec<Party>, String> {
    let sql = match kind {
        NameKind::Customer => format!(
            "{CUSTOMER_NAMES}
             SELECT c.id, cn.full_name AS name, c.company_name, c.phone, c.email, c.billing_address AS address, c.terms_days, 0 AS is_1099
             FROM customers c JOIN cn ON cn.id = c.id WHERE c.is_active = 1 ORDER BY cn.full_name COLLATE NOCASE, c.id"
        ),
        NameKind::Vendor => "SELECT id, name, company_name, phone, email, address, terms_days, is_1099
                             FROM vendors WHERE is_active = 1 ORDER BY name COLLATE NOCASE, id"
            .to_string(),
    };
    sqlx::query_as(&sql).fetch_all(pool).await.map_err(db_err)
}

fn noun(kind: NameKind) -> (&'static str, &'static str) {
    match kind {
        NameKind::Customer => ("Customer", "customers"),
        NameKind::Vendor => ("Vendor", "vendors"),
    }
}

async fn phone_list(pool: &SqlitePool, kind: NameKind, today: NaiveDate) -> Result<AccountantReport, String> {
    let list = parties(pool, kind).await?;
    let (one, many) = noun(kind);
    let columns = vec![head(one, ColumnKind::Text), head("Phone", ColumnKind::Text)];
    let rows = list.iter().map(|p| row(RowKind::Detail, 0, vec![json!(p.name), json!(p.phone)])).collect();
    let notes = vec![format!("{} active {many}.", list.len())];
    Ok(done(None, today, columns, rows, list.len(), notes))
}

/// Each name's balance as of `as_of`, from the Customer (or Vendor) Balance Summary preset.
async fn balances(pool: &SqlitePool, kind: NameKind, as_of: NaiveDate) -> Result<(BTreeMap<i64, i64>, i64), String> {
    let id = match kind {
        NameKind::Customer => "customer-balance-summary",
        NameKind::Vendor => "vendor-balance-summary",
    };
    let mut settings = custom_reports::presets().into_iter().find(|p| p.id == id).ok_or("RPT_UNKNOWN: balance summary")?.settings;
    settings.end_date = Some(as_of);
    let report = custom_reports::run(pool, &settings, as_of).await?;
    let mut out = BTreeMap::new();
    let mut total = 0;
    for r in &report.rows {
        let amount = r.cells.last().and_then(Value::as_i64).unwrap_or(0);
        match r.kind {
            RowKind::Detail => {
                for n in r.drill.iter().flat_map(|d| d.names.iter()).filter(|n| n.kind == kind) {
                    *out.entry(n.id).or_insert(0) += amount;
                }
            }
            RowKind::Total => total = amount,
            _ => {}
        }
    }
    Ok((out, total))
}

async fn contact_list(pool: &SqlitePool, kind: NameKind, today: NaiveDate) -> Result<AccountantReport, String> {
    let list = parties(pool, kind).await?;
    let (balance, _) = balances(pool, kind, today).await?;
    let (one, many) = noun(kind);
    let mut columns = vec![
        head(one, ColumnKind::Text),
        head("Company", ColumnKind::Text),
        head(if kind == NameKind::Customer { "Bill To" } else { "Address" }, ColumnKind::Text),
        head("Phone", ColumnKind::Text),
        head("Email", ColumnKind::Text),
        head("Terms", ColumnKind::Text),
    ];
    if kind == NameKind::Vendor {
        columns.push(head("1099", ColumnKind::Text));
    }
    columns.push(head("Balance", ColumnKind::Money));
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0;
    for p in &list {
        let b = balance.get(&p.id).copied().unwrap_or(0);
        total += b;
        let mut cells = vec![json!(p.name), json!(p.company_name), json!(one_line(p.address.as_deref())), json!(p.phone), json!(p.email), json!(terms_text(p.terms_days))];
        if kind == NameKind::Vendor {
            cells.push(json!(if p.is_1099 { "Yes" } else { "No" }));
        }
        cells.push(json!(b));
        rows.push(row(RowKind::Detail, 0, cells));
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let notes = vec![format!("{} active {many}; balances as of {today}.", list.len())];
    Ok(done(None, today, columns, rows, list.len(), notes))
}

#[derive(Debug, sqlx::FromRow)]
struct ItemRow {
    name: String,
    description: Option<String>,
    rate_cents: i64,
    account: String,
    is_inventory: bool,
    sku: Option<String>,
    qty_on_hand_e4: Option<i64>,
}

async fn item_listing(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let items: Vec<ItemRow> = sqlx::query_as(
        "SELECT p.name, p.description, p.rate_cents, a.full_name AS account, i.id IS NOT NULL AS is_inventory, i.sku, i.qty_on_hand_e4
         FROM products_services p JOIN v_chart_of_accounts a ON a.id = p.income_account_id
         LEFT JOIN inventory_items i ON i.product_id = p.id
         WHERE p.is_active = 1 ORDER BY p.name COLLATE NOCASE, p.id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Item", ColumnKind::Text),
        head("Description", ColumnKind::Text),
        head("Type", ColumnKind::Text),
        head("SKU", ColumnKind::Text),
        head("Income Account", ColumnKind::Text),
        head("Price", ColumnKind::Money),
        head("On Hand", ColumnKind::Quantity),
    ];
    let rows = items
        .iter()
        .map(|i| {
            let kind = if i.is_inventory { "Inventory" } else { "Service or non-stock" };
            row(RowKind::Detail, 0, vec![json!(i.name), json!(i.description), json!(kind), json!(i.sku), json!(i.account), json!(i.rate_cents), json!(i.qty_on_hand_e4)])
        })
        .collect();
    let notes = vec![format!("{}; On Hand is today's quantity of tracked items.", plural(items.len(), "active item", "active items"))];
    Ok(done(None, today, columns, rows, items.len(), notes))
}

#[derive(Debug, sqlx::FromRow)]
struct AccountRow {
    id: i64,
    account_number: Option<String>,
    full_name: String,
    type_name: String,
    is_balance_sheet: bool,
    description: Option<String>,
    balance_cents: i64,
}

async fn account_listing(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let accounts: Vec<AccountRow> = sqlx::query_as(
        "SELECT a.id, a.account_number, a.full_name, a.account_type_name AS type_name, a.is_balance_sheet, a.description,
                COALESCE((SELECT SUM(CASE WHEN a.normal_balance = 'DEBIT' THEN l.debit_cents - l.credit_cents ELSE l.credit_cents - l.debit_cents END)
                          FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
                          WHERE l.account_id = a.id AND e.status = 'POSTED' AND e.entry_date <= ?1), 0) AS balance_cents
         FROM v_chart_of_accounts a
         WHERE a.is_active = 1
         ORDER BY a.type_sort_order, COALESCE(a.account_number, ''), a.full_name COLLATE NOCASE, a.id",
    )
    .bind(today)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![head("Account", ColumnKind::Text), head("Type", ColumnKind::Text), head("Balance Total", ColumnKind::Money), head("Description", ColumnKind::Text)];
    let rows = accounts
        .iter()
        .map(|a| {
            let name = match &a.account_number {
                Some(n) => format!("{n} · {}", a.full_name),
                None => a.full_name.clone(),
            };
            // Income and expense accounts close each year, so only balance sheet accounts show one.
            let balance = a.is_balance_sheet.then_some(a.balance_cents);
            let mut r = row(RowKind::Detail, 0, vec![json!(name), json!(a.type_name), json!(balance), json!(a.description)]);
            r.account_id = Some(a.id);
            r
        })
        .collect();
    let notes = vec![format!("{}; balances of balance sheet accounts as of {today}.", plural(accounts.len(), "active account", "active accounts"))];
    Ok(done(None, today, columns, rows, accounts.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// From the financial statements
// ------------------------------------------------------------------------------------------------

fn row_kind(kind: LineKind) -> RowKind {
    match kind {
        LineKind::Section => RowKind::Section,
        LineKind::Account => RowKind::Detail,
        // A statement's group totals are subtotals here; its bottom lines (TOTAL ASSETS, Net Income) totals.
        LineKind::Subtotal | LineKind::Total => RowKind::Subtotal,
        LineKind::Net => RowKind::Total,
    }
}

/// The Balance Sheet with each group of accounts folded into one line (Bank Accounts, Accounts
/// Receivable, ...): a heading whose lines down to its subtotal are all accounts becomes one row
/// with the subtotal's amount. Accounts outside such a group (Net Income, Retained Earnings) stay.
async fn balance_sheet_summary(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    let bs = crate::statements::balance_sheet(pool, as_of).await?;
    let lines = &bs.lines;
    let columns = vec![head("", ColumnKind::Text), head("Total", ColumnKind::Money)];
    let mut rows = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.kind == LineKind::Section {
            // The heading's own total: the first line at its depth or above that is not an account
            // (a parent account's subtotal sits deeper). Fold it when no heading is in between.
            let close = (i + 1..lines.len()).find(|&j| lines[j].depth <= l.depth && lines[j].kind != LineKind::Account);
            let folds = |j: usize| matches!(lines[j].kind, LineKind::Total | LineKind::Subtotal) && lines[j].depth == l.depth && lines[i + 1..j].iter().all(|x| x.kind != LineKind::Section);
            if let Some(j) = close.filter(|&j| folds(j)) {
                rows.push(labeled(RowKind::Detail, l.depth, 2, &l.label, &[json!(lines[j].amount_cents)]));
                i = j + 1;
                continue;
            }
        }
        let mut r = labeled(row_kind(l.kind), l.depth, 2, &l.label, &[json!(l.amount_cents)]);
        if l.kind == LineKind::Section {
            r.cells[1] = Value::Null;
        }
        r.account_id = l.account_id;
        rows.push(r);
        i += 1;
    }
    let count = rows.iter().filter(|r| r.kind == RowKind::Detail).count();
    let notes = vec![if bs.is_balanced { "Accrual basis.".to_string() } else { format!("Accrual basis. Out of balance by {}.", crate::ledger::format_money(bs.difference_cents, "")) }];
    Ok(done(None, as_of, columns, rows, count, notes))
}

/// Profit & Loss for the period beside the fiscal year to its end date.
async fn pl_ytd_comparison(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let fy_start = fiscal_year_bounds(end, fiscal_start_month(pool).await?).0;
    let period_label = if start == fy_start { "This period".to_string() } else { format!("{start} to {end}") };
    let input = crate::statements::PeriodReportInput {
        columns: vec![
            crate::statements::PeriodColumn { label: period_label, start_date: start, end_date: end },
            crate::statements::PeriodColumn { label: format!("{fy_start} to {end}"), start_date: fy_start, end_date: end },
        ],
        total: false,
        compare: false,
        basis: crate::statements::Basis::default(),
        class_filter: None,
    };
    let pl = crate::statements::profit_and_loss_periods(pool, &input).await?;
    let mut columns = vec![head("", ColumnKind::Text)];
    columns.push(head(&pl.columns[0].label, ColumnKind::Money));
    columns.push(head("Year to Date", ColumnKind::Money));
    let rows: Vec<ReportRow> = pl
        .lines
        .iter()
        .map(|l| {
            let mut cells = vec![json!(l.label)];
            cells.extend(l.amounts_cents.iter().map(|a| json!(a)));
            let mut r = row(row_kind(l.kind), l.depth, cells);
            r.account_id = l.account_id;
            r
        })
        .collect();
    let count = rows.iter().filter(|r| r.kind == RowKind::Detail).count();
    let notes = vec![format!("Accrual basis. Year to Date runs from {fy_start}, the start of the fiscal year.")];
    Ok(done(Some(start), end, columns, rows, count, notes))
}

// ------------------------------------------------------------------------------------------------
// Customers and vendors
// ------------------------------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct Settlement {
    invoice_id: i64,
    customer_id: i64,
    customer: String,
    issue_date: NaiveDate,
    total_cents: i64,
    date: NaiveDate,
    amount_cents: i64,
}

/// For each invoice paid in full during the range (the payment or credit that brought its balance
/// to zero is dated in the range), the days from the invoice date to that day; averaged by customer.
async fn average_days_to_pay(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    // Payments and credits applied, in date order, with the same arithmetic as the A/R aging.
    let events: Vec<Settlement> = sqlx::query_as(&format!(
        "{CUSTOMER_NAMES}
         SELECT i.id AS invoice_id, i.customer_id, cn.full_name AS customer, i.issue_date, COALESCE(i.home_total_cents, i.total_cents) AS total_cents, s.date, s.amount_cents
         FROM invoices i JOIN cn ON cn.id = i.customer_id
         JOIN (SELECT invoice_id, payment_date AS date, COALESCE(ar_relief_cents, amount_cents) AS amount_cents, id AS seq FROM invoice_payments WHERE is_voided = 0
               UNION ALL SELECT invoice_id, applied_date, amount_cents, id + 1000000000000 FROM credit_applications WHERE is_voided = 0 AND invoice_id IS NOT NULL) s
           ON s.invoice_id = i.id
         WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND s.date <= ?1
         ORDER BY cn.full_name COLLATE NOCASE, i.customer_id, i.id, s.date, s.seq"
    ))
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    // customer → (name, invoices, total days)
    let mut by_customer: BTreeMap<(String, i64), (usize, i64)> = BTreeMap::new();
    let mut i = 0;
    while i < events.len() {
        let first = &events[i];
        let mut paid = 0i64;
        let mut paid_on = None;
        while i < events.len() && events[i].invoice_id == first.invoice_id {
            let e = &events[i];
            paid += e.amount_cents;
            if paid_on.is_none() && paid >= first.total_cents {
                paid_on = Some(e.date);
            }
            i += 1;
        }
        if let Some(day) = paid_on.filter(|d| *d >= start) {
            let entry = by_customer.entry((first.customer.to_lowercase(), first.customer_id)).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += (day - first.issue_date).num_days();
        }
    }
    let names: BTreeMap<i64, String> = events.iter().map(|e| (e.customer_id, e.customer.clone())).collect();
    let columns = vec![head("Customer", ColumnKind::Text), head("Invoices Paid", ColumnKind::Number), head("Average Days to Pay", ColumnKind::Number)];
    let average = |days: i64, count: usize| mul_div_round(days, 1, count.max(1) as i64).unwrap_or(0);
    let mut rows = Vec::new();
    let (mut invoices, mut days) = (0usize, 0i64);
    for ((_, id), (count, sum)) in &by_customer {
        rows.push(row(RowKind::Detail, 0, vec![json!(names[id]), json!(count), json!(average(*sum, *count))]));
        invoices += count;
        days += sum;
    }
    rows.push(row(RowKind::Total, 0, vec![json!("TOTAL"), json!(invoices), json!(average(days, invoices))]));
    let notes = vec![format!(
        "{} paid in full from {start} to {end}. Days run from the invoice date to the payment or credit that paid it off.",
        plural(invoices, "invoice", "invoices")
    )];
    Ok(done(Some(start), end, columns, rows, by_customer.len(), notes))
}

/// A bill's lines for one job: the job (none for lines without one), its name and their amount.
type JobPart = (Option<i64>, Option<String>, i64);

/// Sorts groups by name with "none" last: (1 for none, lowercase name, id).
type GroupKey = (u8, String, i64);

/// A job's name and its open bills, each with the job's share of the open balance.
type JobBills<'a> = (String, Vec<(&'a OpenBill, i64)>);

#[derive(Debug, sqlx::FromRow)]
struct OpenBill {
    bill_id: i64,
    journal_entry_id: i64,
    vendor: String,
    number: Option<String>,
    date: NaiveDate,
    due_date: NaiveDate,
    total_cents: i64,
    open_cents: i64,
}

/// Bills with a balance as of `as_of`, under the customer or job their lines are for. A bill for
/// several jobs splits its open balance in proportion to their lines (the last job takes the
/// rounding), so the report totals what Unpaid Bills shows for bills.
async fn unpaid_bills_by_job(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    let bills: Vec<OpenBill> = sqlx::query_as(
        "SELECT * FROM (
            SELECT b.id AS bill_id, b.journal_entry_id, v.name AS vendor, b.bill_number AS number, b.bill_date AS date, b.due_date, b.total_cents,
                   b.total_cents
                   - COALESCE((SELECT SUM(p.amount_cents) FROM bill_payments p WHERE p.bill_id = b.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
                   - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.bill_id = b.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                   AS open_cents
            FROM bills b JOIN vendors v ON v.id = b.vendor_id
            WHERE b.is_voided = 0 AND b.journal_entry_id IS NOT NULL AND b.bill_date <= ?1
         ) WHERE open_cents <> 0 ORDER BY due_date, date, bill_id",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    // bill → [(job id, job name, line amount)] in line order.
    let lines: Vec<(i64, Option<i64>, Option<String>, i64)> = sqlx::query_as(&format!(
        "{CUSTOMER_NAMES}
         SELECT l.bill_id, l.customer_id, cn.full_name, SUM(l.amount_cents) FROM bill_lines l LEFT JOIN cn ON cn.id = l.customer_id
         GROUP BY l.bill_id, l.customer_id ORDER BY l.bill_id, MIN(l.line_number)"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut by_bill: BTreeMap<i64, Vec<JobPart>> = BTreeMap::new();
    for (bill, job, name, amount) in lines {
        by_bill.entry(bill).or_default().push((job, name, amount));
    }
    // (sort name, job id) → (shown name, [(bill, share)])
    let mut jobs: BTreeMap<GroupKey, JobBills<'_>> = BTreeMap::new();
    for b in &bills {
        let parts = by_bill.get(&b.bill_id).cloned().unwrap_or_default();
        let mut left = b.open_cents;
        for (k, (job, name, amount)) in parts.iter().enumerate() {
            let share = if k + 1 == parts.len() { left } else { mul_div_round(b.open_cents, *amount, b.total_cents).ok_or("RPT_OVERFLOW: open balance")? };
            left -= share;
            let key = match (job, name) {
                (Some(id), Some(n)) => (0, n.to_lowercase(), *id),
                _ => (1, String::new(), 0),
            };
            let shown = name.clone().unwrap_or_else(|| "No customer or job".into());
            jobs.entry(key).or_insert_with(|| (shown, Vec::new())).1.push((b, share));
        }
    }
    let columns = vec![
        head("", ColumnKind::Text),
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Num", ColumnKind::Text),
        head("Vendor", ColumnKind::Text),
        head("Due Date", ColumnKind::Date),
        head("Aging", ColumnKind::Number),
        head("Open Balance", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0;
    for (name, items) in jobs.values() {
        rows.push(section(0, w, name));
        let mut sub = 0;
        for (b, share) in items {
            let aging = (as_of - b.due_date).num_days().max(0);
            let mut r = row(RowKind::Detail, 1, vec![Value::Null, json!("Bill"), json!(b.date), json!(b.number), json!(b.vendor), json!(b.due_date), json!(aging), json!(share)]);
            r.journal_entry_id = Some(b.journal_entry_id);
            rows.push(r);
            sub += share;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {name}"), &[json!(sub)]));
        total += sub;
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let notes = vec![format!("{} with a balance as of {as_of}. A bill for several jobs is split by its lines' amounts.", plural(bills.len(), "bill", "bills"))];
    Ok(done(None, as_of, columns, rows, bills.len(), notes))
}

#[derive(Debug, sqlx::FromRow)]
struct PoLine {
    vendor_id: i64,
    vendor: String,
    po_number: i64,
    po_date: NaiveDate,
    expected_date: Option<NaiveDate>,
    item: Option<String>,
    description: Option<String>,
    quantity_e4: Option<i64>,
    rate_cents: Option<i64>,
    amount_cents: i64,
}

/// Every line of the purchase orders still open as of `as_of`, by vendor (the Open Purchase Orders
/// report shows one row per order).
async fn open_po_detail(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    let lines: Vec<PoLine> = sqlx::query_as(
        "SELECT v.id AS vendor_id, v.name AS vendor, p.po_number, p.po_date, p.expected_date, ps.name AS item, l.description,
                l.quantity_e4, l.rate_cents, l.amount_cents
         FROM purchase_orders p JOIN vendors v ON v.id = p.vendor_id
         JOIN purchase_order_lines l ON l.purchase_order_id = p.id
         LEFT JOIN products_services ps ON ps.id = l.product_service_id
         WHERE p.status = 'OPEN' AND p.po_date <= ?1
         ORDER BY v.name COLLATE NOCASE, v.id, p.po_date, p.po_number, l.line_number",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Num", ColumnKind::Text),
        head("Item", ColumnKind::Text),
        head("Description", ColumnKind::Text),
        head("Qty", ColumnKind::Quantity),
        head("Rate", ColumnKind::Money),
        head("Expected", ColumnKind::Date),
        head("Amount", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0;
    let mut orders = 0;
    let mut i = 0;
    while i < lines.len() {
        let vendor = &lines[i];
        rows.push(section(0, w, &vendor.vendor));
        let mut sub = 0;
        let mut last_po = None;
        while i < lines.len() && lines[i].vendor_id == vendor.vendor_id {
            let l = &lines[i];
            if last_po != Some(l.po_number) {
                orders += 1;
                last_po = Some(l.po_number);
            }
            rows.push(row(
                RowKind::Detail,
                1,
                vec![Value::Null, json!(l.po_date), json!(l.po_number.to_string()), json!(l.item), json!(l.description), json!(l.quantity_e4), json!(l.rate_cents), json!(l.expected_date), json!(l.amount_cents)],
            ));
            sub += l.amount_cents;
            i += 1;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {}", vendor.vendor), &[json!(sub)]));
        total += sub;
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let notes = vec![format!("{} open as of {as_of}. Purchase orders do not post to the books.", plural(orders, "purchase order", "purchase orders"))];
    Ok(done(None, as_of, columns, rows, lines.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Time
// ------------------------------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct TimeRow {
    date: NaiveDate,
    employee_id: i64,
    employee: String,
    job_id: Option<i64>,
    job: Option<String>,
    item_id: Option<i64>,
    item: Option<String>,
    description: Option<String>,
    minutes: i64,
    billable: bool,
}

async fn time_rows(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<Vec<TimeRow>, String> {
    sqlx::query_as(&format!(
        "{CUSTOMER_NAMES}
         SELECT t.activity_date AS date, t.employee_id, e.name AS employee, t.customer_id AS job_id, cn.full_name AS job,
                t.product_service_id AS item_id, ps.name AS item, t.description, t.minutes, t.billable
         FROM time_activities t JOIN employees e ON e.id = t.employee_id
         LEFT JOIN cn ON cn.id = t.customer_id
         LEFT JOIN products_services ps ON ps.id = t.product_service_id
         WHERE t.activity_date BETWEEN ?1 AND ?2
         ORDER BY t.activity_date, t.id"
    ))
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeBy {
    Job,
    Name,
    Item,
}

/// Sort key and label of a time row's group; rows without one sort last.
fn time_group(by: TimeBy, t: &TimeRow) -> (GroupKey, String) {
    let (id, name, none) = match by {
        TimeBy::Job => (t.job_id, t.job.clone(), "No customer or job"),
        TimeBy::Name => (Some(t.employee_id), Some(t.employee.clone()), ""),
        TimeBy::Item => (t.item_id, t.item.clone(), "No service item"),
    };
    match (id, name) {
        (Some(id), Some(n)) => ((0, n.to_lowercase(), id), n),
        _ => ((1, String::new(), 0), none.to_string()),
    }
}

/// Hours by job, by employee or by service item, with the billable hours beside them.
async fn time_summary(pool: &SqlitePool, by: TimeBy, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let time = time_rows(pool, start, end).await?;
    let mut groups: BTreeMap<GroupKey, (String, i64, i64)> = BTreeMap::new();
    for t in &time {
        let (key, label) = time_group(by, t);
        let g = groups.entry(key).or_insert((label, 0, 0));
        g.1 += t.minutes;
        if t.billable {
            g.2 += t.minutes;
        }
    }
    let first = match by {
        TimeBy::Job => "Customer:Job",
        TimeBy::Name => "Employee",
        TimeBy::Item => "Service Item",
    };
    let columns = vec![head(first, ColumnKind::Text), head("Billable Hours", ColumnKind::Quantity), head("Hours", ColumnKind::Quantity)];
    let mut rows = Vec::new();
    let (mut minutes, mut billable) = (0, 0);
    for (label, m, b) in groups.values() {
        rows.push(row(RowKind::Detail, 0, vec![json!(label), json!(hours_e4(*b)), json!(hours_e4(*m))]));
        minutes += m;
        billable += b;
    }
    rows.push(row(RowKind::Total, 0, vec![json!("TOTAL"), json!(hours_e4(billable)), json!(hours_e4(minutes))]));
    let notes = vec![format!("{} of time from {start} to {end}.", plural(time.len(), "entry", "entries"))];
    Ok(done(Some(start), end, columns, rows, groups.len(), notes))
}

async fn time_by_job_detail(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let time = time_rows(pool, start, end).await?;
    let mut groups: BTreeMap<GroupKey, (String, Vec<&TimeRow>)> = BTreeMap::new();
    for t in &time {
        let (key, label) = time_group(TimeBy::Job, t);
        groups.entry(key).or_insert_with(|| (label, Vec::new())).1.push(t);
    }
    let columns = vec![
        head("", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Name", ColumnKind::Text),
        head("Service Item", ColumnKind::Text),
        head("Description", ColumnKind::Text),
        head("Billable", ColumnKind::Text),
        head("Hours", ColumnKind::Quantity),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut minutes = 0;
    for (label, items) in groups.values() {
        rows.push(section(0, w, label));
        let mut sub = 0;
        for t in items {
            rows.push(row(
                RowKind::Detail,
                1,
                vec![Value::Null, json!(t.date), json!(t.employee), json!(t.item), json!(t.description), json!(if t.billable { "Yes" } else { "No" }), json!(hours_e4(t.minutes))],
            ));
            sub += t.minutes;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {label}"), &[json!(hours_e4(sub))]));
        minutes += sub;
    }
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(hours_e4(minutes))]));
    let notes = vec![format!("{} of time from {start} to {end}.", plural(time.len(), "entry", "entries"))];
    Ok(done(Some(start), end, columns, rows, time.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Inventory
// ------------------------------------------------------------------------------------------------

/// Every tracked item with today's quantity on hand and a blank column to write the count in.
async fn physical_inventory(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    let valuation = crate::report_center::inventory_valuation(pool, today).await?;
    let columns = vec![
        head("Item", ColumnKind::Text),
        head("Description", ColumnKind::Text),
        head("SKU", ColumnKind::Text),
        head("On Hand", ColumnKind::Quantity),
        head("Physical Count", ColumnKind::Text),
    ];
    let rows = valuation
        .rows
        .iter()
        .map(|r| row(RowKind::Detail, 0, vec![json!(r.name), json!(r.description), json!(r.sku), json!(r.qty_on_hand_e4), Value::Null]))
        .collect();
    let notes = vec![format!("{} tracked; On Hand as of {today}. Write the count in the last column.", plural(valuation.rows.len(), "item", "items"))];
    Ok(done(None, today, columns, rows, valuation.rows.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Audit trail
// ------------------------------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct AuditRow {
    day: NaiveDate,
    time: String,
    actor: Option<String>,
    action_type: String,
    target_entity_id: i64,
    entry_id: Option<i64>,
    entry_number: Option<i64>,
    entry_date: Option<NaiveDate>,
    source_type: Option<String>,
    amount_cents: Option<i64>,
    list: Option<String>,
    list_name: Option<String>,
    changed: Option<String>,
    reversal_number: Option<i64>,
    /// What an `EDIT` record was (`UNDO_RECONCILIATION`), and its own description.
    kind: Option<String>,
    payload_detail: Option<String>,
}

fn action_label(action: &str) -> &str {
    match action {
        "POST" => "Posted",
        "VOID" => "Voided",
        "MIGRATION" => "File updated",
        "ACCOUNT_CHANGE" => "Account changed",
        "FORENSIC_HEAL" => "Correction",
        "USER_CHANGE" => "User changed",
        "PRICE_CHANGE" => "Prices changed",
        "ACH_FILE" => "ACH file",
        "LIST_CHANGE" => "List changed",
        "CLEANUP" => "Clean up",
        "CONDENSE" => "Condense",
        "EDIT" => "Edited",
        other => other,
    }
}

/// Every record of the hash-chained audit trail (`audit_chain.rs`) made on the days in the range,
/// in the order written, with who made it. Read only; the note says whether the chain checks out.
async fn audit_trail(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let records: Vec<AuditRow> = sqlx::query_as(
        "SELECT date(a.timestamp, 'localtime') AS day, strftime('%H:%M', a.timestamp, 'localtime') AS time, a.actor, a.action_type, a.target_entity_id,
                e.id AS entry_id, COALESCE(e.entry_number, json_extract(a.payload_snapshot, '$.entryNumber')) AS entry_number,
                COALESCE(e.entry_date, json_extract(a.payload_snapshot, '$.entryDate')) AS entry_date,
                CASE WHEN e.id IS NOT NULL THEN COALESCE(json_extract(a.payload_snapshot, '$.sourceType'), e.source_type) END AS source_type,
                COALESCE(json_extract(a.payload_snapshot, '$.totalDebitCents'), json_extract(a.payload_snapshot, '$.amountCents')) AS amount_cents,
                json_extract(a.payload_snapshot, '$.list') AS list, json_extract(a.payload_snapshot, '$.after.name') AS list_name,
                (SELECT group_concat(value, ', ') FROM json_each(a.payload_snapshot, '$.changed')) AS changed,
                json_extract(a.payload_snapshot, '$.reversalEntryNumber') AS reversal_number,
                json_extract(a.payload_snapshot, '$.kind') AS kind, json_extract(a.payload_snapshot, '$.detail') AS payload_detail
         FROM ledger_audit_log a
         LEFT JOIN journal_entries e ON a.action_type IN ('POST', 'VOID') AND e.id = a.target_entity_id
         WHERE date(a.timestamp, 'localtime') BETWEEN ?1 AND ?2
         ORDER BY a.id",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Date", ColumnKind::Date),
        head("Time", ColumnKind::Text),
        head("User", ColumnKind::Text),
        head("Action", ColumnKind::Text),
        head("Num", ColumnKind::Text),
        head("Type", ColumnKind::Text),
        head("Entry Date", ColumnKind::Date),
        head("Detail", ColumnKind::Text),
        head("Amount", ColumnKind::Money),
    ];
    let rows = records
        .iter()
        .map(|a| {
            let detail = match a.action_type.as_str() {
                "VOID" => a.reversal_number.map(|n| format!("Reversed by entry {n}")),
                "LIST_CHANGE" => {
                    let what = a.list.as_deref().map(|l| l.to_lowercase()).unwrap_or_else(|| "list".into());
                    let name = a.list_name.clone().unwrap_or_default();
                    Some(match a.changed.as_deref().filter(|c| !c.is_empty()) {
                        Some(c) => format!("{what} {name}: {c}"),
                        None => format!("{what} {name}"),
                    })
                }
                "EDIT" if a.kind.as_deref() == Some("UNDO_RECONCILIATION") => a.payload_detail.clone(),
                "POST" | "MIGRATION" => None,
                _ => Some(format!("Record {}", a.target_entity_id)),
            };
            let kind = a.source_type.as_deref().map(type_label);
            let mut r = row(
                RowKind::Detail,
                0,
                vec![
                    json!(a.day),
                    json!(a.time),
                    json!(a.actor.as_deref().unwrap_or("")),
                    json!(if a.kind.as_deref() == Some("UNDO_RECONCILIATION") { "Reconciliation undone" } else { action_label(&a.action_type) }),
                    json!(a.entry_number.map(|n| n.to_string())),
                    json!(kind),
                    json!(a.entry_date),
                    json!(detail),
                    json!(a.amount_cents),
                ],
            );
            r.journal_entry_id = a.entry_id;
            r
        })
        .collect();
    let check = crate::audit_chain::verify(pool).await?;
    let integrity = if check.problems.is_empty() {
        format!("The audit trail's chain checks out ({} sealed).", plural(check.records_sealed as usize, "record", "records"))
    } else {
        format!("The audit trail's chain has {}; see Company › Audit Log › Verify.", plural(check.problems.len(), "problem", "problems"))
    };
    let notes = vec![format!("{} from {start} to {end}.", plural(records.len(), "record", "records")), integrity];
    Ok(done(Some(start), end, columns, rows, records.len(), notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_and_run() {
        let mut seen = std::collections::HashSet::new();
        for r in catalog() {
            assert!(seen.insert(r.id), "{} twice", r.id);
            assert_eq!(r.dates == Dates::Range, !r.period.is_empty(), "{} has a period only when it takes a range", r.id);
        }
    }

    #[test]
    fn hours_and_terms() {
        assert_eq!(hours_e4(90), 15_000);
        assert_eq!(hours_e4(20), 3_333);
        assert_eq!(terms_text(Some(0)).as_deref(), Some("Due on receipt"));
        assert_eq!(terms_text(Some(30)).as_deref(), Some("Net 30"));
        assert_eq!(one_line(Some("12 Main St\n\nSpringfield, IL")).as_deref(), Some("12 Main St, Springfield, IL"));
        assert_eq!(one_line(Some("  ")), None);
    }
}
