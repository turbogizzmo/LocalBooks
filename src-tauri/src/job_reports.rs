//! Job costing reports, like classic desktop accounting software's Jobs, Time & Mileage reports:
//! Job Profitability, Job Estimates vs. Actuals, Item Profitability, Item Estimates vs. Actuals,
//! Estimates by Job, Progress Invoices vs. Estimates and Profit and Loss by Job.
//!
//! **Which job a ledger line belongs to.** Every income and expense line in the date range is given
//! a customer or job (or none) and a product or service (or none):
//! - Lines of an invoice, sales receipt, credit memo or payment belong to that document's customer.
//! - Lines of a bill, expense, check, card charge or card credit, or vendor credit belong to the
//!   customer or job named on the document line they were posted from (`customer_id`, added for
//!   job costing). A bill posts one line per account and class, so a ledger line is shared among
//!   the document lines with its account and class in proportion to their amounts, and the shares
//!   always add up to the ledger line exactly. The product comes from the same document lines;
//!   cost of goods sold comes from the stock leaving on the sale.
//! - Voids follow the document they reverse. Everything else (journal entries, deposits, ...) has
//!   no job.
//!
//! So every report adds up to the Profit & Loss for the same dates: actual revenue is income less
//! its debits (credit memos, discounts), actual cost is expense and cost of goods sold, and the
//! difference is net income. Time isn't costed: a time activity has a billing rate but there is no
//! cost rate to cost it at. Estimates carry no cost, so the estimate reports compare revenue only.
//!
//! Output uses the custom report engine's columns and row kinds, so the screen, drill-down and
//! exports work the same way.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::custom_reports::{type_label, ColumnHead, ColumnKind, RowKind};
use crate::ledger::{db_err, mul_div_round, DISPLAY_SOURCE_TYPE};
use crate::statements::ProfitAndLossByClass;

/// Label of the row and column for lines with no customer or job.
const NO_JOB: &str = "No customer/job";
const NO_ITEM: &str = "No product/service";

// ------------------------------------------------------------------------------------------------
// Settings and output
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobReportKind {
    /// Actual cost, revenue and difference per customer and job.
    JobProfitSummary,
    /// The same per job, broken down by product/service or account.
    JobProfitDetail,
    /// The ledger lines behind the other reports, grouped by job.
    JobTransactions,
    /// Estimated against actual revenue per customer and job.
    JobEstimatesSummary,
    /// The same per job, by product/service.
    JobEstimatesDetail,
    /// Actual cost, revenue and difference per product/service.
    ItemProfit,
    /// Estimated against actual revenue per product/service.
    ItemEstimates,
    /// Every estimate, grouped by customer and job.
    EstimatesByJob,
    /// Each estimate's invoiced amount and share.
    ProgressInvoices,
}

/// What Job Profitability Detail breaks each job down by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Breakdown {
    #[default]
    Item,
    Account,
}

/// Which lines (and estimates) a report includes. Empty lists mean "any", unless the matching
/// `no_*` flag is set, which then means "only those with none".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JobFilter {
    pub job_ids: Vec<i64>,
    /// Each of `job_ids` with every job below it.
    pub with_subjobs: bool,
    /// Lines with no customer or job.
    pub no_job: bool,
    pub item_ids: Vec<i64>,
    /// Lines with no product or service.
    pub no_item: bool,
    pub account_ids: Vec<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JobReportParams {
    /// First day; `None` for all dates.
    pub start_date: Option<NaiveDate>,
    /// Last day; `None` for today.
    pub end_date: Option<NaiveDate>,
    pub filter: JobFilter,
    pub by: Breakdown,
}

/// The report a row opens: the same dates, narrowed to the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDrill {
    pub report: JobReportKind,
    pub filter: JobFilter,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRow {
    pub kind: RowKind,
    pub depth: u32,
    /// One value per column: text, a `YYYY-MM-DD` date, integer cents or a percentage, or null. The
    /// first column holds group headings and total labels.
    pub cells: Vec<Value>,
    /// The transaction a row opens.
    pub journal_entry_id: Option<i64>,
    /// The estimate a row is for.
    pub estimate_id: Option<i64>,
    pub drill: Option<JobDrill>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobReport {
    pub kind: JobReportKind,
    pub start_date: Option<NaiveDate>,
    pub end_date: NaiveDate,
    pub columns: Vec<ColumnHead>,
    pub rows: Vec<JobRow>,
    /// Ledger lines (or estimates) the report is built from.
    pub line_count: usize,
}

fn col(label: &str, kind: ColumnKind) -> ColumnHead {
    ColumnHead { label: label.into(), kind }
}

fn row(kind: RowKind, depth: u32, cells: Vec<Value>) -> JobRow {
    JobRow { kind, depth, cells, journal_entry_id: None, estimate_id: None, drill: None }
}

/// `part` as a percentage of `whole`, to one decimal place; null when `whole` is zero.
fn percent(part: i64, whole: i64) -> Value {
    if whole == 0 {
        return Value::Null;
    }
    json!((part as f64 * 1000.0 / whole as f64).round() / 10.0)
}

fn money(cents: i64) -> Value {
    json!(cents)
}

// ------------------------------------------------------------------------------------------------
// Customers and jobs
// ------------------------------------------------------------------------------------------------

struct Customer {
    /// The full name, `Customer:Job`.
    name: String,
    /// The last part of the name.
    short: String,
    parent: Option<i64>,
}

struct Customers {
    by_id: HashMap<i64, Customer>,
    /// Parents before their jobs, each level by name.
    order: Vec<i64>,
}

impl Customers {
    async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let rows: Vec<(i64, String, Option<i64>)> =
            sqlx::query_as("SELECT id, name, parent_id FROM customers ORDER BY replace(lower(name), ':', char(1)), id").fetch_all(pool).await.map_err(db_err)?;
        let mut by_id = HashMap::new();
        let mut order = Vec::new();
        for (id, name, parent) in rows {
            let short = name.rsplit(':').next().unwrap_or(&name).to_string();
            by_id.insert(id, Customer { name, short, parent });
            order.push(id);
        }
        Ok(Customers { by_id, order })
    }

    fn name(&self, id: Option<i64>) -> String {
        id.and_then(|i| self.by_id.get(&i)).map_or_else(|| NO_JOB.to_string(), |c| c.name.clone())
    }

    fn children(&self, id: Option<i64>) -> impl Iterator<Item = i64> + '_ {
        self.order.iter().copied().filter(move |c| self.by_id[c].parent == id)
    }

    /// The ids and every job below them.
    fn with_descendants(&self, ids: &[i64]) -> HashSet<i64> {
        let mut out: HashSet<i64> = ids.iter().copied().collect();
        let mut grew = true;
        while grew {
            grew = false;
            for (id, c) in &self.by_id {
                if c.parent.is_some_and(|p| out.contains(&p)) && out.insert(*id) {
                    grew = true;
                }
            }
        }
        out
    }

    /// (id, parent, name, full name) in display order, for Profit and Loss columns.
    fn tree(&self) -> Vec<(i64, Option<i64>, String, String)> {
        self.order.iter().map(|id| (*id, self.by_id[id].parent, self.by_id[id].short.clone(), self.by_id[id].name.clone())).collect()
    }

    /// Jobs (and None) in display order, None last.
    fn sorted(&self, jobs: impl IntoIterator<Item = Option<i64>>) -> Vec<Option<i64>> {
        let set: HashSet<Option<i64>> = jobs.into_iter().collect();
        let mut out: Vec<Option<i64>> = self.order.iter().copied().map(Some).filter(|j| set.contains(j)).collect();
        if set.contains(&None) {
            out.push(None);
        }
        out
    }
}

/// Refuses a document line's customer or job that doesn't exist. `context` is `CODE: line n`.
pub(crate) async fn check_job(tx: &mut Transaction<'_, Sqlite>, customer_id: i64, context: &str) -> Result<(), String> {
    let (code, line) = context.split_once(": ").unwrap_or((context, "a line"));
    let found: Option<i64> = sqlx::query_scalar("SELECT id FROM customers WHERE id = ?1").bind(customer_id).fetch_optional(&mut **tx).await.map_err(db_err)?;
    match found {
        Some(_) => Ok(()),
        None => Err(format!("{code}_UNKNOWN_JOB: {line} names a customer or job that does not exist")),
    }
}

// ------------------------------------------------------------------------------------------------
// Ledger lines by job
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct RawLine {
    journal_entry_id: i64,
    date: NaiveDate,
    source_type: String,
    is_reversal: bool,
    doc_type: String,
    doc_id: Option<i64>,
    doc_entry: i64,
    num: Option<String>,
    name: Option<String>,
    memo: Option<String>,
    account_id: i64,
    account_name: String,
    account_number: Option<String>,
    is_income: bool,
    type_sort: i64,
    class_id: Option<i64>,
    debit_cents: i64,
    credit_cents: i64,
}

/// An income or expense line (or a share of one) with its customer or job and product.
#[derive(Debug, Clone)]
struct JobLine {
    journal_entry_id: i64,
    date: NaiveDate,
    type_text: String,
    num: Option<String>,
    name: Option<String>,
    memo: Option<String>,
    account_id: i64,
    account_name: String,
    account_number: Option<String>,
    type_sort: i64,
    is_income: bool,
    job: Option<i64>,
    item: Option<i64>,
    /// Credits minus debits: revenue positive, cost negative.
    amount: i64,
}

impl JobLine {
    fn revenue(&self) -> i64 {
        if self.is_income {
            self.amount
        } else {
            0
        }
    }

    fn cost(&self) -> i64 {
        if self.is_income {
            0
        } else {
            -self.amount
        }
    }
}

/// A document line a ledger line may have been posted from.
#[derive(Debug, Clone, sqlx::FromRow)]
struct Piece {
    doc_type: String,
    doc_id: i64,
    account_id: i64,
    class_id: Option<i64>,
    item_id: Option<i64>,
    customer_id: Option<i64>,
    weight: i64,
}

/// The document lines of every sales and purchase document. Sales lines carry the document's
/// customer; purchase lines carry their own.
const PIECES: &str = "
    SELECT 'INVOICE' AS doc_type, it.invoice_id AS doc_id, it.income_account_id AS account_id, it.class_id,
           it.product_service_id AS item_id, i.customer_id, it.amount_cents AS weight
    FROM invoice_items it JOIN invoices i ON i.id = it.invoice_id
    UNION ALL
    SELECT 'SALES_RECEIPT', sl.sales_receipt_id, sl.income_account_id, sl.class_id, sl.product_service_id, r.customer_id, sl.amount_cents
    FROM sales_receipt_lines sl JOIN sales_receipts r ON r.id = sl.sales_receipt_id
    UNION ALL
    SELECT 'CREDIT_MEMO', cl.credit_memo_id, cl.income_account_id, cl.class_id, cl.product_service_id, m.customer_id, cl.amount_cents
    FROM credit_memo_lines cl JOIN credit_memos m ON m.id = cl.credit_memo_id
    UNION ALL
    SELECT 'BILL', bill_id, account_id, class_id, product_service_id, customer_id, amount_cents FROM bill_lines
    UNION ALL
    SELECT 'VENDOR_CREDIT', vendor_credit_id, account_id, class_id, NULL, customer_id, amount_cents FROM vendor_credit_lines
    UNION ALL
    SELECT x.kind, el.expense_id, el.account_id, el.class_id, NULL, el.customer_id, el.amount_cents
    FROM expense_lines el JOIN expenses x ON x.id = el.expense_id";

/// Each sales document's customer (a payment's is its invoice's).
const DOC_CUSTOMERS: &str = "
    SELECT 'INVOICE', id, customer_id FROM invoices WHERE customer_id IS NOT NULL
    UNION ALL SELECT 'SALES_RECEIPT', id, customer_id FROM sales_receipts WHERE customer_id IS NOT NULL
    UNION ALL SELECT 'CREDIT_MEMO', id, customer_id FROM credit_memos
    UNION ALL SELECT 'RECEIVE_PAYMENT', p.id, i.customer_id FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id WHERE i.customer_id IS NOT NULL";

/// Shares `amount` in proportion to `weights` (by size); the shares add up to `amount` exactly.
fn split(amount: i64, weights: &[i64]) -> Vec<i64> {
    let mut sizes: Vec<i64> = weights.iter().map(|w| w.saturating_abs()).collect();
    let mut total: i64 = sizes.iter().sum();
    if total == 0 {
        sizes = vec![1; weights.len()];
        total = sizes.len() as i64;
    }
    let (mut running, mut assigned) = (0, 0);
    sizes
        .iter()
        .map(|size| {
            running += size;
            let upto = mul_div_round(amount, running, total).expect("a share is within the line's amount");
            let share = upto - assigned;
            assigned = upto;
            share
        })
        .collect()
}

/// Every income and expense line from `start` through `end`, given its customer or job and product.
async fn job_lines(pool: &SqlitePool, start: Option<NaiveDate>, end: NaiveDate) -> Result<Vec<JobLine>, String> {
    // A void's lines follow the document whose entry it reverses.
    let sql = format!(
        "WITH le AS (
             SELECT e.*, COALESCE(o.source_type, e.source_type) AS doc_type, COALESCE(o.source_id, e.source_id) AS doc_id,
                    COALESCE(o.id, e.id) AS doc_entry
             FROM journal_entries e LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
             WHERE e.status = 'POSTED' AND (?1 IS NULL OR e.entry_date >= ?1) AND e.entry_date <= ?2)
         SELECT e.id AS journal_entry_id, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type,
                e.reverses_entry_id IS NOT NULL AS is_reversal, e.doc_type, e.doc_id, e.doc_entry,
                COALESCE(CAST(inv.invoice_number AS TEXT), ip.reference, CAST(sr.receipt_number AS TEXT), CAST(cm.credit_number AS TEXT),
                         bl.bill_number, vc.reference, x.check_number,
                         CASE WHEN e.doc_type IN ('JOURNAL', 'OPENING_BALANCE') THEN CAST(e.entry_number AS TEXT) END) AS num,
                p.party_name AS name, COALESCE(l.memo, e.memo) AS memo,
                a.id AS account_id, a.full_name AS account_name, a.account_number, a.classification = 'INCOME' AS is_income,
                a.type_sort_order AS type_sort, l.class_id, l.debit_cents, l.credit_cents
         FROM journal_lines l
         JOIN le e ON e.id = l.journal_entry_id
         JOIN v_chart_of_accounts a ON a.id = l.account_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = e.doc_entry
         LEFT JOIN invoices inv ON e.doc_type = 'INVOICE' AND inv.id = e.doc_id
         LEFT JOIN invoice_payments ip ON e.doc_type = 'RECEIVE_PAYMENT' AND ip.id = e.doc_id
         LEFT JOIN sales_receipts sr ON e.doc_type = 'SALES_RECEIPT' AND sr.id = e.doc_id
         LEFT JOIN credit_memos cm ON e.doc_type = 'CREDIT_MEMO' AND cm.id = e.doc_id
         LEFT JOIN bills bl ON e.doc_type = 'BILL' AND bl.id = e.doc_id
         LEFT JOIN vendor_credits vc ON e.doc_type = 'VENDOR_CREDIT' AND vc.id = e.doc_id
         LEFT JOIN expenses x ON e.doc_type IN ('EXPENSE', 'CHECK') AND x.id = e.doc_id
         WHERE a.classification IN ('INCOME', 'EXPENSE')
         ORDER BY e.entry_date, e.entry_number, l.line_number"
    );
    let raw: Vec<RawLine> = sqlx::query_as(&sql).bind(start).bind(end).fetch_all(pool).await.map_err(db_err)?;
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let mut pieces: HashMap<(String, i64), Vec<Piece>> = HashMap::new();
    for p in sqlx::query_as::<_, Piece>(PIECES).fetch_all(pool).await.map_err(db_err)? {
        pieces.entry((p.doc_type.clone(), p.doc_id)).or_default().push(p);
    }
    let customers: HashMap<(String, i64), i64> =
        sqlx::query_as::<_, (String, i64, i64)>(DOC_CUSTOMERS).fetch_all(pool).await.map_err(db_err)?.into_iter().map(|(t, id, c)| ((t, id), c)).collect();
    // Cost of goods sold on a sale: the stock that left, by product, at its cost.
    let mut sold: HashMap<i64, Vec<(i64, i64, i64)>> = HashMap::new();
    for (entry, account, product, cost) in sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT s.journal_entry_id, i.cogs_account_id, i.product_id, -s.value_change_cents
         FROM stock_ledger_entries s JOIN inventory_items i ON i.id = s.item_id WHERE s.kind = 'SALE'",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    {
        sold.entry(entry).or_default().push((account, product, cost));
    }

    let mut out = Vec::with_capacity(raw.len());
    for r in raw {
        let amount = r.credit_cents - r.debit_cents;
        let doc = r.doc_id.map(|id| (r.doc_type.clone(), id));
        let default_job = doc.as_ref().and_then(|d| customers.get(d)).copied();
        // (job, item, weight) of the document lines this ledger line was posted from.
        let mut sources: Vec<(Option<i64>, Option<i64>, i64)> = Vec::new();
        if let Some(doc_pieces) = doc.as_ref().and_then(|d| pieces.get(d)) {
            let exact: Vec<&Piece> = doc_pieces.iter().filter(|p| p.account_id == r.account_id && p.class_id == r.class_id).collect();
            let matched = if exact.is_empty() { doc_pieces.iter().filter(|p| p.account_id == r.account_id).collect() } else { exact };
            sources = matched.iter().map(|p| (p.customer_id, p.item_id, p.weight)).collect();
        }
        if sources.is_empty() {
            if let Some(stock) = sold.get(&r.doc_entry) {
                sources = stock.iter().filter(|(account, _, _)| *account == r.account_id).map(|(_, product, cost)| (default_job, Some(*product), *cost)).collect();
            }
        }
        if sources.is_empty() {
            sources.push((default_job, None, 1));
        }
        // Document lines that share a job and product count as one.
        let mut merged: Vec<((Option<i64>, Option<i64>), i64)> = Vec::new();
        for (job, item, weight) in sources {
            match merged.iter_mut().find(|(k, _)| *k == (job, item)) {
                Some((_, w)) => *w += weight.saturating_abs(),
                None => merged.push(((job, item), weight.saturating_abs())),
            }
        }
        let weights: Vec<i64> = merged.iter().map(|(_, w)| *w).collect();
        let type_text = if r.is_reversal { "Void".to_string() } else { type_label(&r.source_type).to_string() };
        for (((job, item), _), share) in merged.into_iter().zip(split(amount, &weights)) {
            if share == 0 && amount != 0 {
                continue;
            }
            out.push(JobLine {
                journal_entry_id: r.journal_entry_id,
                date: r.date,
                type_text: type_text.clone(),
                num: r.num.clone(),
                name: r.name.clone(),
                memo: r.memo.clone(),
                account_id: r.account_id,
                account_name: r.account_name.clone(),
                account_number: r.account_number.clone(),
                type_sort: r.type_sort,
                is_income: r.is_income,
                job,
                item,
                amount: share,
            });
        }
    }
    Ok(out)
}

/// The filter, resolved against the customer list.
struct Matcher {
    jobs: Option<HashSet<i64>>,
    no_job: bool,
    items: Option<HashSet<i64>>,
    no_item: bool,
    accounts: Option<HashSet<i64>>,
}

impl Matcher {
    fn new(f: &JobFilter, customers: &Customers) -> Self {
        let jobs = (!f.job_ids.is_empty() || f.no_job).then(|| if f.with_subjobs { customers.with_descendants(&f.job_ids) } else { f.job_ids.iter().copied().collect() });
        let items = (!f.item_ids.is_empty() || f.no_item).then(|| f.item_ids.iter().copied().collect());
        let accounts = (!f.account_ids.is_empty()).then(|| f.account_ids.iter().copied().collect());
        Matcher { jobs, no_job: f.no_job, items, no_item: f.no_item, accounts }
    }

    fn job(&self, job: Option<i64>) -> bool {
        match (&self.jobs, job) {
            (None, _) => true,
            (Some(set), Some(j)) => set.contains(&j),
            (Some(_), None) => self.no_job,
        }
    }

    fn item(&self, item: Option<i64>) -> bool {
        match (&self.items, item) {
            (None, _) => true,
            (Some(set), Some(i)) => set.contains(&i),
            (Some(_), None) => self.no_item,
        }
    }

    fn line(&self, l: &JobLine) -> bool {
        self.job(l.job) && self.item(l.item) && self.accounts.as_ref().is_none_or(|a| a.contains(&l.account_id))
    }
}

// ------------------------------------------------------------------------------------------------
// Estimates
// ------------------------------------------------------------------------------------------------

/// An estimate line's revenue (a discount counts as a line with no product, taken off).
#[derive(Debug, Clone, sqlx::FromRow)]
struct EstimateLine {
    customer_id: i64,
    item_id: Option<i64>,
    amount_cents: i64,
}

/// Lines of the estimates dated in the range that weren't rejected.
async fn estimate_lines(pool: &SqlitePool, start: Option<NaiveDate>, end: NaiveDate) -> Result<Vec<EstimateLine>, String> {
    sqlx::query_as(
        "SELECT e.customer_id, l.product_service_id AS item_id, l.amount_cents
         FROM estimate_lines l JOIN estimates e ON e.id = l.estimate_id
         WHERE e.status <> 'REJECTED' AND (?1 IS NULL OR e.estimate_date >= ?1) AND e.estimate_date <= ?2
         UNION ALL
         SELECT customer_id, NULL, -discount_cents FROM estimates
         WHERE discount_cents <> 0 AND status <> 'REJECTED' AND (?1 IS NULL OR estimate_date >= ?1) AND estimate_date <= ?2",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

fn status_label(status: &str) -> &str {
    match status {
        "PENDING" => "Pending",
        "ACCEPTED" => "Accepted",
        "REJECTED" => "Rejected",
        "CONVERTED" => "Invoiced",
        other => other,
    }
}

// ------------------------------------------------------------------------------------------------
// Layout
// ------------------------------------------------------------------------------------------------

fn add(sums: &mut Vec<i64>, values: &[i64]) {
    if sums.is_empty() {
        sums.resize(values.len(), 0);
    }
    for (s, v) in sums.iter_mut().zip(values) {
        *s += v;
    }
}

/// Rows for customers and their jobs: a customer with job rows gets a heading, a row for its own
/// amounts ("(other)") and a total. Returns the grand total.
struct Tree<'a> {
    customers: &'a Customers,
    sums: &'a HashMap<Option<i64>, Vec<i64>>,
    cells: &'a dyn Fn(&[i64]) -> Vec<Value>,
    /// The drill-down for a row covering these jobs (or, with `true`, the lines with no job).
    drill: &'a dyn Fn(Vec<i64>, bool) -> Option<JobDrill>,
}

impl Tree<'_> {
    fn rows(&self, width: usize) -> (Vec<JobRow>, Vec<i64>) {
        let mut rows = Vec::new();
        let mut total = vec![0; width];
        for root in self.customers.children(None) {
            if let Some((sum, _)) = self.node(root, 0, &mut rows) {
                add(&mut total, &sum);
            }
        }
        if let Some(none) = self.sums.get(&None) {
            let mut r = row(RowKind::Detail, 0, self.labelled(NO_JOB, none));
            r.drill = (self.drill)(Vec::new(), true);
            rows.push(r);
            add(&mut total, none);
        }
        (rows, total)
    }

    fn labelled(&self, label: &str, sums: &[i64]) -> Vec<Value> {
        let mut cells = vec![json!(label)];
        cells.extend((self.cells)(sums));
        cells
    }

    fn node(&self, id: i64, depth: u32, rows: &mut Vec<JobRow>) -> Option<(Vec<i64>, Vec<i64>)> {
        let short = &self.customers.by_id[&id].short;
        let own = self.sums.get(&Some(id));
        let mut below = Vec::new();
        let mut sum: Vec<i64> = Vec::new();
        let mut ids = Vec::new();
        for child in self.customers.children(Some(id)) {
            if let Some((s, child_ids)) = self.node(child, depth + 1, &mut below) {
                add(&mut sum, &s);
                ids.extend(child_ids);
            }
        }
        if below.is_empty() {
            let own = own?;
            let mut r = row(RowKind::Detail, depth, self.labelled(short, own));
            r.drill = (self.drill)(vec![id], false);
            rows.push(r);
            return Some((own.clone(), vec![id]));
        }
        rows.push(row(RowKind::Section, depth, vec![json!(short)]));
        if let Some(own) = own {
            let mut r = row(RowKind::Detail, depth + 1, self.labelled(&format!("{short} (other)"), own));
            r.drill = (self.drill)(vec![id], false);
            rows.push(r);
            add(&mut sum, own);
            ids.push(id);
        }
        rows.extend(below);
        let mut r = row(RowKind::Subtotal, depth, self.labelled(&format!("Total {short}"), &sum));
        r.drill = (self.drill)(ids.clone(), false);
        rows.push(r);
        Some((sum, ids))
    }
}

/// Key of a product row: named products by name, then "no product".
fn item_key(item: Option<i64>, names: &HashMap<i64, String>) -> (bool, String, i64) {
    match item {
        Some(id) => (false, names.get(&id).map(|n| n.to_lowercase()).unwrap_or_default(), id),
        None => (true, String::new(), 0),
    }
}

fn item_label(item: Option<i64>, names: &HashMap<i64, String>) -> String {
    item.and_then(|i| names.get(&i).cloned()).unwrap_or_else(|| NO_ITEM.into())
}

async fn item_names(pool: &SqlitePool) -> Result<HashMap<i64, String>, String> {
    Ok(sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM products_services").fetch_all(pool).await.map_err(db_err)?.into_iter().collect())
}

/// Actual cost, revenue and difference.
fn profit_cells(s: &[i64]) -> Vec<Value> {
    vec![money(s[0]), money(s[1]), money(s[1] - s[0])]
}

fn profit_columns(first: &str) -> Vec<ColumnHead> {
    vec![col(first, ColumnKind::Text), col("Act. Cost", ColumnKind::Money), col("Act. Revenue", ColumnKind::Money), col("($) Diff.", ColumnKind::Money)]
}

/// Actual cost, then estimated and actual revenue with their difference (sums: estimate, actual
/// revenue, actual cost).
fn estimate_cells(s: &[i64]) -> Vec<Value> {
    vec![money(s[2]), money(s[0]), money(s[1]), money(s[1] - s[0]), percent(s[1] - s[0], s[0])]
}

fn estimate_columns(first: &str) -> Vec<ColumnHead> {
    vec![
        col(first, ColumnKind::Text),
        col("Act. Cost", ColumnKind::Money),
        col("Est. Revenue", ColumnKind::Money),
        col("Act. Revenue", ColumnKind::Money),
        col("($) Diff.", ColumnKind::Money),
        col("(%) Diff.", ColumnKind::Number),
    ]
}

// ------------------------------------------------------------------------------------------------
// Reports
// ------------------------------------------------------------------------------------------------

pub async fn run(pool: &SqlitePool, kind: JobReportKind, params: &JobReportParams, today: NaiveDate) -> Result<JobReport, String> {
    let end = params.end_date.unwrap_or(today);
    let start = params.start_date;
    if start.is_some_and(|s| s > end) {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let customers = Customers::load(pool).await?;
    let matcher = Matcher::new(&params.filter, &customers);
    let mut report = JobReport { kind, start_date: start, end_date: end, columns: Vec::new(), rows: Vec::new(), line_count: 0 };
    match kind {
        JobReportKind::JobProfitSummary => {
            let lines: Vec<JobLine> = job_lines(pool, start, end).await?.into_iter().filter(|l| matcher.line(l)).collect();
            let mut sums: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
            for l in &lines {
                add(sums.entry(l.job).or_default(), &[l.cost(), l.revenue()]);
            }
            let drill = |ids: Vec<i64>, none: bool| Some(JobDrill { report: JobReportKind::JobProfitDetail, filter: JobFilter { job_ids: ids, with_subjobs: false, no_job: none, ..params.filter.clone() } });
            let tree = Tree { customers: &customers, sums: &sums, cells: &profit_cells, drill: &drill };
            let (rows, total) = tree.rows(2);
            report.columns = profit_columns("Customer:Job");
            report.rows = rows;
            report.rows.push(row(RowKind::Total, 0, tree.labelled("TOTAL", &total)));
            report.line_count = lines.len();
        }
        JobReportKind::JobProfitDetail | JobReportKind::JobTransactions => {
            let lines: Vec<JobLine> = job_lines(pool, start, end).await?.into_iter().filter(|l| matcher.line(l)).collect();
            let names = item_names(pool).await?;
            report.line_count = lines.len();
            let by_job = group(&lines, |l| l.job);
            let mut total = vec![0, 0];
            report.columns = if kind == JobReportKind::JobTransactions {
                vec![
                    col("Type", ColumnKind::Text),
                    col("Date", ColumnKind::Date),
                    col("Num", ColumnKind::Text),
                    col("Name", ColumnKind::Text),
                    col("Memo", ColumnKind::Text),
                    col("Account", ColumnKind::Text),
                    col("Product/Service", ColumnKind::Text),
                    col("Act. Cost", ColumnKind::Money),
                    col("Act. Revenue", ColumnKind::Money),
                ]
            } else {
                profit_columns(if params.by == Breakdown::Item { "Product/Service" } else { "Account" })
            };
            let width = report.columns.len();
            let blank = |label: String, sums: &[i64]| {
                let mut cells = vec![Value::Null; width];
                cells[0] = json!(label);
                if width == 4 {
                    cells.splice(1.., profit_cells(sums));
                } else {
                    cells[width - 2] = money(sums[0]);
                    cells[width - 1] = money(sums[1]);
                }
                cells
            };
            for job in customers.sorted(by_job.keys().copied()) {
                let job_lines = &by_job[&job];
                let label = customers.name(job);
                report.rows.push(row(RowKind::Section, 0, vec![json!(label.clone())]));
                let mut sum = vec![0, 0];
                let job_filter = JobFilter { job_ids: job.into_iter().collect(), with_subjobs: false, no_job: job.is_none(), ..params.filter.clone() };
                if kind == JobReportKind::JobTransactions {
                    for l in job_lines {
                        let cells = vec![
                            json!(l.type_text),
                            json!(l.date),
                            json!(l.num),
                            json!(l.name),
                            json!(l.memo),
                            json!(l.account_name),
                            json!(item_label(l.item, &names)),
                            if l.is_income { Value::Null } else { money(l.cost()) },
                            if l.is_income { money(l.revenue()) } else { Value::Null },
                        ];
                        let mut r = row(RowKind::Detail, 1, cells);
                        r.journal_entry_id = Some(l.journal_entry_id);
                        report.rows.push(r);
                        add(&mut sum, &[l.cost(), l.revenue()]);
                    }
                } else {
                    // Rows by product (named ones by name, then none) or by account (chart order).
                    let mut groups: BTreeMap<(i64, String, String, i64), (String, Option<i64>, Option<i64>, Vec<i64>)> = BTreeMap::new();
                    for l in job_lines {
                        let (key, label, item, account) = match params.by {
                            Breakdown::Item => {
                                let (none, name, id) = item_key(l.item, &names);
                                ((i64::from(none), name, String::new(), id), item_label(l.item, &names), l.item, None)
                            }
                            Breakdown::Account => (
                                (l.type_sort, l.account_number.clone().unwrap_or_default(), l.account_name.to_lowercase(), l.account_id),
                                l.account_name.clone(),
                                None,
                                Some(l.account_id),
                            ),
                        };
                        add(&mut groups.entry(key).or_insert_with(|| (label, item, account, Vec::new())).3, &[l.cost(), l.revenue()]);
                    }
                    for (label, item, account, sums) in groups.into_values() {
                        let mut r = row(RowKind::Detail, 1, blank(label, &sums));
                        let mut filter = job_filter.clone();
                        match params.by {
                            Breakdown::Item => {
                                filter.item_ids = item.into_iter().collect();
                                filter.no_item = item.is_none();
                            }
                            Breakdown::Account => filter.account_ids = account.into_iter().collect(),
                        }
                        r.drill = Some(JobDrill { report: JobReportKind::JobTransactions, filter });
                        report.rows.push(r);
                        add(&mut sum, &sums);
                    }
                }
                let mut r = row(RowKind::Subtotal, 0, blank(format!("Total {label}"), &sum));
                if kind == JobReportKind::JobProfitDetail {
                    r.drill = Some(JobDrill { report: JobReportKind::JobTransactions, filter: job_filter });
                }
                report.rows.push(r);
                add(&mut total, &sum);
            }
            report.rows.push(row(RowKind::Total, 0, blank("TOTAL".into(), &total)));
        }
        JobReportKind::ItemProfit => {
            let lines: Vec<JobLine> = job_lines(pool, start, end).await?.into_iter().filter(|l| matcher.line(l)).collect();
            let names = item_names(pool).await?;
            report.line_count = lines.len();
            let mut sums: BTreeMap<(bool, String, i64), (Option<i64>, Vec<i64>)> = BTreeMap::new();
            for l in &lines {
                add(&mut sums.entry(item_key(l.item, &names)).or_insert_with(|| (l.item, Vec::new())).1, &[l.cost(), l.revenue()]);
            }
            report.columns = profit_columns("Product/Service");
            let mut total = vec![0, 0];
            for (item, s) in sums.into_values() {
                let mut cells = vec![json!(item_label(item, &names))];
                cells.extend(profit_cells(&s));
                let mut r = row(RowKind::Detail, 0, cells);
                let filter = JobFilter { item_ids: item.into_iter().collect(), no_item: item.is_none(), ..params.filter.clone() };
                r.drill = Some(JobDrill { report: JobReportKind::JobTransactions, filter });
                report.rows.push(r);
                add(&mut total, &s);
            }
            let mut cells = vec![json!("TOTAL")];
            cells.extend(profit_cells(&total));
            report.rows.push(row(RowKind::Total, 0, cells));
        }
        JobReportKind::JobEstimatesSummary | JobReportKind::JobEstimatesDetail | JobReportKind::ItemEstimates => {
            let lines: Vec<JobLine> = job_lines(pool, start, end).await?.into_iter().filter(|l| matcher.line(l)).collect();
            let estimates: Vec<EstimateLine> =
                estimate_lines(pool, start, end).await?.into_iter().filter(|e| matcher.job(Some(e.customer_id)) && matcher.item(e.item_id)).collect();
            let names = item_names(pool).await?;
            report.line_count = lines.len() + estimates.len();
            // (estimated revenue, actual revenue, actual cost) per (job, product).
            let mut sums: HashMap<(Option<i64>, Option<i64>), Vec<i64>> = HashMap::new();
            for e in &estimates {
                add(sums.entry((Some(e.customer_id), e.item_id)).or_default(), &[e.amount_cents, 0, 0]);
            }
            // Account filters narrow the actuals only; estimates have no accounts.
            for l in &lines {
                add(sums.entry((l.job, l.item)).or_default(), &[0, l.revenue(), l.cost()]);
            }
            match kind {
                JobReportKind::JobEstimatesSummary => {
                    let mut by_job: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
                    for ((job, _), s) in &sums {
                        add(by_job.entry(*job).or_default(), s);
                    }
                    let drill = |ids: Vec<i64>, none: bool| Some(JobDrill { report: JobReportKind::JobEstimatesDetail, filter: JobFilter { job_ids: ids, with_subjobs: false, no_job: none, ..params.filter.clone() } });
                    let tree = Tree { customers: &customers, sums: &by_job, cells: &estimate_cells, drill: &drill };
                    let (rows, total) = tree.rows(3);
                    report.columns = estimate_columns("Customer:Job");
                    report.rows = rows;
                    report.rows.push(row(RowKind::Total, 0, tree.labelled("TOTAL", &total)));
                }
                JobReportKind::JobEstimatesDetail => {
                    report.columns = estimate_columns("Product/Service");
                    let mut total = vec![0; 3];
                    for job in customers.sorted(sums.keys().map(|(j, _)| *j)) {
                        let label = customers.name(job);
                        report.rows.push(row(RowKind::Section, 0, vec![json!(label.clone())]));
                        let mut items: Vec<(Option<i64>, &Vec<i64>)> = sums.iter().filter(|((j, _), _)| *j == job).map(|((_, i), s)| (*i, s)).collect();
                        items.sort_by_key(|(i, _)| item_key(*i, &names));
                        let mut sum = vec![0; 3];
                        for (item, s) in items {
                            let mut cells = vec![json!(item_label(item, &names))];
                            cells.extend(estimate_cells(s));
                            let mut r = row(RowKind::Detail, 1, cells);
                            let filter = JobFilter { job_ids: job.into_iter().collect(), with_subjobs: false, no_job: job.is_none(), item_ids: item.into_iter().collect(), no_item: item.is_none(), ..params.filter.clone() };
                            r.drill = Some(JobDrill { report: JobReportKind::JobTransactions, filter });
                            report.rows.push(r);
                            add(&mut sum, s);
                        }
                        let mut cells = vec![json!(format!("Total {label}"))];
                        cells.extend(estimate_cells(&sum));
                        report.rows.push(row(RowKind::Subtotal, 0, cells));
                        add(&mut total, &sum);
                    }
                    let mut cells = vec![json!("TOTAL")];
                    cells.extend(estimate_cells(&total));
                    report.rows.push(row(RowKind::Total, 0, cells));
                }
                _ => {
                    let mut by_item: BTreeMap<(bool, String, i64), (Option<i64>, Vec<i64>)> = BTreeMap::new();
                    for ((_, item), s) in &sums {
                        add(&mut by_item.entry(item_key(*item, &names)).or_insert_with(|| (*item, Vec::new())).1, s);
                    }
                    report.columns = estimate_columns("Product/Service");
                    let mut total = vec![0; 3];
                    for (item, s) in by_item.into_values() {
                        let mut cells = vec![json!(item_label(item, &names))];
                        cells.extend(estimate_cells(&s));
                        let mut r = row(RowKind::Detail, 0, cells);
                        let filter = JobFilter { item_ids: item.into_iter().collect(), no_item: item.is_none(), ..params.filter.clone() };
                        r.drill = Some(JobDrill { report: JobReportKind::JobTransactions, filter });
                        report.rows.push(r);
                        add(&mut total, &s);
                    }
                    let mut cells = vec![json!("TOTAL")];
                    cells.extend(estimate_cells(&total));
                    report.rows.push(row(RowKind::Total, 0, cells));
                }
            }
        }
        JobReportKind::EstimatesByJob | JobReportKind::ProgressInvoices => {
            estimates_report(pool, &mut report, &customers, &matcher).await?;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct EstimateRow {
    id: i64,
    estimate_number: i64,
    customer_id: i64,
    estimate_date: NaiveDate,
    expiration_date: Option<NaiveDate>,
    status: String,
    memo: Option<String>,
    total_cents: i64,
    invoice_id: Option<i64>,
}

/// Estimates by Job and Progress Invoices vs. Estimates: one row per estimate dated in the range,
/// grouped by customer and job.
async fn estimates_report(pool: &SqlitePool, report: &mut JobReport, customers: &Customers, matcher: &Matcher) -> Result<(), String> {
    let estimates: Vec<EstimateRow> = sqlx::query_as(
        "SELECT id, estimate_number, customer_id, estimate_date, expiration_date, status, memo, total_cents, converted_to_invoice_id AS invoice_id
         FROM estimates WHERE (?1 IS NULL OR estimate_date >= ?1) AND estimate_date <= ?2 ORDER BY estimate_date, estimate_number",
    )
    .bind(report.start_date)
    .bind(report.end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let estimates: Vec<EstimateRow> = estimates.into_iter().filter(|e| matcher.job(Some(e.customer_id))).collect();
    report.line_count = estimates.len();

    // The invoice an estimate became, following edits (an edit voids the invoice and posts a
    // replacement): (invoice number, total, live entry), or None once voided.
    let invoices: HashMap<i64, (i64, i64, Option<i64>, bool)> = sqlx::query_as::<_, (i64, i64, i64, Option<i64>, bool)>(
        "SELECT id, invoice_number, total_cents, journal_entry_id, is_voided FROM invoices",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    .into_iter()
    .map(|(id, n, t, e, v)| (id, (n, t, e, v)))
    .collect();
    let by_entry: HashMap<i64, i64> = invoices.iter().filter_map(|(id, (_, _, e, _))| e.map(|e| (e, *id))).collect();
    let edits: HashMap<i64, i64> =
        sqlx::query_as::<_, (i64, i64)>("SELECT old_entry_id, new_entry_id FROM transaction_edits").fetch_all(pool).await.map_err(db_err)?.into_iter().collect();
    let live_invoice = |mut id: i64| -> Option<(i64, i64, i64)> {
        for _ in 0..64 {
            let (number, total, entry, voided) = *invoices.get(&id)?;
            if !voided {
                return entry.map(|e| (number, total, e));
            }
            id = *by_entry.get(edits.get(&entry?)?)?;
        }
        None
    };

    let progress = report.kind == JobReportKind::ProgressInvoices;
    report.columns = if progress {
        vec![
            col("Date", ColumnKind::Date),
            col("Num", ColumnKind::Text),
            col("Status", ColumnKind::Text),
            col("Est. Total", ColumnKind::Money),
            col("Invoiced", ColumnKind::Money),
            col("(%) Invoiced", ColumnKind::Number),
            col("Invoice", ColumnKind::Text),
        ]
    } else {
        vec![
            col("Date", ColumnKind::Date),
            col("Num", ColumnKind::Text),
            col("Status", ColumnKind::Text),
            col("Expires", ColumnKind::Date),
            col("Memo", ColumnKind::Text),
            col("Amount", ColumnKind::Money),
        ]
    };
    let width = report.columns.len();
    let totals = |label: String, estimated: i64, invoiced: i64| {
        let mut cells = vec![Value::Null; width];
        cells[0] = json!(label);
        if progress {
            cells[3] = money(estimated);
            cells[4] = money(invoiced);
            cells[5] = percent(invoiced, estimated);
        } else {
            cells[5] = money(estimated);
        }
        cells
    };
    let mut by_job: HashMap<Option<i64>, Vec<&EstimateRow>> = HashMap::new();
    for e in &estimates {
        by_job.entry(Some(e.customer_id)).or_default().push(e);
    }
    let (mut all_estimated, mut all_invoiced) = (0, 0);
    for job in customers.sorted(by_job.keys().copied()) {
        let label = customers.name(job);
        report.rows.push(row(RowKind::Section, 0, vec![json!(label.clone())]));
        let (mut estimated, mut invoiced) = (0, 0);
        for e in &by_job[&job] {
            let invoice = e.invoice_id.and_then(live_invoice);
            let billed = invoice.map_or(0, |(_, total, _)| total);
            let cells = if progress {
                vec![
                    json!(e.estimate_date),
                    json!(e.estimate_number.to_string()),
                    json!(status_label(&e.status)),
                    money(e.total_cents),
                    money(billed),
                    percent(billed, e.total_cents),
                    invoice.map_or(Value::Null, |(n, _, _)| json!(n.to_string())),
                ]
            } else {
                vec![
                    json!(e.estimate_date),
                    json!(e.estimate_number.to_string()),
                    json!(status_label(&e.status)),
                    json!(e.expiration_date),
                    json!(e.memo),
                    money(e.total_cents),
                ]
            };
            let mut r = row(RowKind::Detail, 1, cells);
            r.estimate_id = Some(e.id);
            if progress {
                r.journal_entry_id = invoice.map(|(_, _, entry)| entry);
            }
            report.rows.push(r);
            estimated += e.total_cents;
            invoiced += billed;
        }
        report.rows.push(row(RowKind::Subtotal, 0, totals(format!("Total {label}"), estimated, invoiced)));
        all_estimated += estimated;
        all_invoiced += invoiced;
    }
    report.rows.push(row(RowKind::Total, 0, totals("TOTAL".into(), all_estimated, all_invoiced)));
    Ok(())
}

fn group<K: std::hash::Hash + Eq + Copy>(lines: &[JobLine], key: impl Fn(&JobLine) -> K) -> HashMap<K, Vec<&JobLine>> {
    let mut out: HashMap<K, Vec<&JobLine>> = HashMap::new();
    for l in lines {
        out.entry(key(l)).or_default().push(l);
    }
    out
}

/// Profit & Loss with a column per customer and job, a "No customer/job" column, and a Total column
/// equal to the ordinary Profit & Loss (accrual basis).
pub async fn profit_and_loss_by_job(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<ProfitAndLossByClass, String> {
    if end_date < start_date {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let customers = Customers::load(pool).await?;
    let rows: Vec<(Option<i64>, i64, i64)> = job_lines(pool, Some(start_date), end_date).await?.into_iter().map(|l| (l.job, l.account_id, -l.amount)).collect();
    crate::statements::profit_and_loss_by_columns(pool, start_date, end_date, rows, &customers.tree(), NO_JOB).await
}

#[cfg(test)]
mod tests;
