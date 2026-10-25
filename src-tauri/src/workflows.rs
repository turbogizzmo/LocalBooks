//! Everyday workflows from classic desktop accounting software: customer statements, the 1099
//! report, recurring (memorized) transactions, purchase orders, finding transactions, and files
//! attached to transactions.

use chrono::{Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::expenses::{BillInput, BillLineInput, ExpenseInput};
use crate::invoices::InvoiceInput;
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, DISPLAY_SOURCE_TYPE, MAX_LINE_CENTS};

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------------------------------------
// Customer statements
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StatementRow {
    pub date: NaiveDate,
    pub source_type: String,
    pub number: Option<String>,
    pub party_name: String,
    pub memo: Option<String>,
    /// Charges positive, payments and credits negative.
    pub amount_cents: i64,
    #[sqlx(skip)]
    pub balance_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerStatement {
    pub customer_id: i64,
    pub customer_name: String,
    pub company_name: Option<String>,
    pub billing_address: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub opening_balance_cents: i64,
    pub rows: Vec<StatementRow>,
    pub closing_balance_cents: i64,
    /// Current, 1–30, 31–60, 61–90, 91+ days past due at the end date.
    pub aging_cents: [i64; 5],
}

/// A customer's statement for a period, including its jobs: the balance brought forward, each
/// invoice, payment, credit and refund in Accounts Receivable, and the balance due by age.
pub async fn customer_statement(pool: &SqlitePool, customer_id: i64, start_date: NaiveDate, end_date: NaiveDate) -> Result<CustomerStatement, String> {
    let head: Option<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT name, company_name, billing_address FROM customers WHERE id = ?1").bind(customer_id).fetch_optional(pool).await.map_err(db_err)?;
    let (name, company_name, billing_address) = head.ok_or("STMT_NOT_FOUND: that customer does not exist")?;
    let mine = "(p.party_name = ?1 OR substr(p.party_name, 1, length(?1) + 1) = ?1 || ':')";
    let ar_lines = "FROM v_posted_lines l
         JOIN v_chart_of_accounts a ON a.id = l.account_id AND a.account_type = 'ACCOUNTS_RECEIVABLE'
         JOIN v_entry_party p ON p.journal_entry_id = l.journal_entry_id";
    let opening: i64 = sqlx::query_scalar(&format!("SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) {ar_lines} WHERE {mine} AND l.entry_date < ?2"))
        .bind(&name)
        .bind(start_date)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let mut rows: Vec<StatementRow> = sqlx::query_as(&format!(
        "SELECT l.entry_date AS date,
                CASE WHEN l.source_type = 'INVOICE' AND EXISTS (SELECT 1 FROM finance_charge_invoices WHERE invoice_id = l.source_id)
                     THEN 'FINANCE_CHARGE' ELSE l.source_type END AS source_type,
                p.party_name,
                CASE l.source_type
                    WHEN 'INVOICE' THEN (SELECT CAST(invoice_number AS TEXT) FROM invoices WHERE id = l.source_id)
                    WHEN 'CREDIT_MEMO' THEN (SELECT CAST(credit_number AS TEXT) FROM credit_memos WHERE id = l.source_id)
                    WHEN 'RECEIVE_PAYMENT' THEN (SELECT reference FROM invoice_payments WHERE id = l.source_id)
                END AS number,
                l.memo, l.debit_cents - l.credit_cents AS amount_cents
         {ar_lines}
         WHERE {mine} AND l.entry_date BETWEEN ?2 AND ?3
         ORDER BY l.entry_date, l.entry_number, l.line_number"
    ))
    .bind(&name)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut balance = opening;
    for row in &mut rows {
        balance += row.amount_cents;
        row.balance_cents = balance;
    }
    let aging = crate::report_center::aging(pool, false, end_date).await?;
    let mut buckets = [0i64; 5];
    for row in aging.rows.iter().filter(|r| r.party_name == name || r.party_name.starts_with(&format!("{name}:"))) {
        for (b, cents) in buckets.iter_mut().zip(row.buckets_cents) {
            *b += cents;
        }
    }
    Ok(CustomerStatement {
        customer_id,
        customer_name: name,
        company_name,
        billing_address,
        start_date,
        end_date,
        opening_balance_cents: opening,
        rows,
        closing_balance_cents: balance,
        aging_cents: buckets,
    })
}

/// Customers with a balance or activity in the period (top-level customers; jobs are included in
/// their customer's statement).
pub async fn statement_customers(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<(i64, String, i64)>, String> {
    sqlx::query_as(
        "SELECT c.id, c.name,
                COALESCE((SELECT SUM(l.debit_cents - l.credit_cents) FROM v_posted_lines l
                          JOIN v_chart_of_accounts a ON a.id = l.account_id AND a.account_type = 'ACCOUNTS_RECEIVABLE'
                          JOIN v_entry_party p ON p.journal_entry_id = l.journal_entry_id
                          WHERE (p.party_name = c.name OR substr(p.party_name, 1, length(c.name) + 1) = c.name || ':') AND l.entry_date <= ?2), 0) AS balance
         FROM customers c
         WHERE c.parent_id IS NULL AND (
             EXISTS (SELECT 1 FROM v_posted_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id AND a.account_type = 'ACCOUNTS_RECEIVABLE'
                     JOIN v_entry_party p ON p.journal_entry_id = l.journal_entry_id
                     WHERE (p.party_name = c.name OR substr(p.party_name, 1, length(c.name) + 1) = c.name || ':') AND l.entry_date BETWEEN ?1 AND ?2)
             OR balance <> 0)
         ORDER BY c.name COLLATE NOCASE",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)
    .map(|rows: Vec<(i64, String, i64)>| rows)
}

// ------------------------------------------------------------------------------------------------
// 1099
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Vendor1099Row {
    pub vendor_id: i64,
    pub vendor_name: String,
    /// The full tax ID, when this user may see it.
    pub tax_id: Option<String>,
    /// The tax ID with all but the last four digits starred; set whenever the vendor has a tax ID.
    pub tax_id_masked: Option<String>,
    pub address: Option<String>,
    pub paid_cents: i64,
    #[sqlx(skip)]
    pub over_threshold: bool,
}

/// The 1099-NEC reporting threshold.
pub const NEC_THRESHOLD_CENTS: i64 = 60_000;

/// Payments to vendors marked for a 1099 in a calendar year, from bank accounts (card payments are
/// reported by the card company, so they are left out): checks and expenses paid from a bank, and
/// bill payments from a bank, by payment date.
pub async fn vendors_1099(pool: &SqlitePool, year: i32, access: crate::tax_ids::Access<'_>) -> Result<Vec<Vendor1099Row>, String> {
    let (start, end) = (NaiveDate::from_ymd_opt(year, 1, 1).ok_or("RPT_BAD_YEAR: invalid year")?, NaiveDate::from_ymd_opt(year, 12, 31).ok_or("RPT_BAD_YEAR: invalid year")?);
    let mut rows: Vec<Vendor1099Row> = sqlx::query_as(
        "SELECT v.id AS vendor_id, v.name AS vendor_name, v.tax_id, v.tax_id_masked, v.address,
                COALESCE((SELECT SUM(COALESCE(x.home_total_cents, x.total_cents)) FROM expenses x JOIN chart_of_accounts a ON a.id = x.payment_account_id
                          WHERE x.vendor_id = v.id AND x.is_voided = 0 AND a.account_type = 'BANK' AND x.expense_date BETWEEN ?1 AND ?2), 0)
              + COALESCE((SELECT SUM(p.amount_cents) FROM bill_payments p JOIN bills b ON b.id = p.bill_id JOIN chart_of_accounts a ON a.id = p.payment_account_id
                          WHERE b.vendor_id = v.id AND p.is_voided = 0 AND a.account_type = 'BANK' AND p.payment_date BETWEEN ?1 AND ?2), 0) AS paid_cents
         FROM vendors v WHERE v.is_1099 = 1 ORDER BY v.name COLLATE NOCASE",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for row in &mut rows {
        row.over_threshold = row.paid_cents >= NEC_THRESHOLD_CENTS;
        let view = crate::tax_ids::show(access, row.vendor_id, row.tax_id.as_deref(), row.tax_id_masked.as_deref())?;
        (row.tax_id, row.tax_id_masked) = (view.full, view.masked);
    }
    Ok(rows)
}

// ------------------------------------------------------------------------------------------------
// Recurring transactions
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringInput {
    pub name: String,
    /// `JOURNAL`, `EXPENSE`, `BILL` or `INVOICE`.
    pub kind: String,
    /// The transaction as its form posts it; its date is replaced by each scheduled date.
    pub payload: serde_json::Value,
    /// `WEEKLY`, `MONTHLY`, `QUARTERLY` or `YEARLY`, every `every` of them.
    pub frequency: String,
    #[serde(default = "one")]
    pub every: i64,
    pub next_date: NaiveDate,
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecurringView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub frequency: String,
    pub every: i64,
    pub next_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub is_active: bool,
    pub last_run_date: Option<String>,
    pub run_count: i64,
    #[sqlx(skip)]
    pub amount_cents: i64,
    #[sqlx(skip)]
    pub payee: Option<String>,
    #[serde(skip)]
    pub payload: String,
}

/// The date after `date` on a schedule.
pub fn advance(date: NaiveDate, frequency: &str, every: i64) -> Option<NaiveDate> {
    let n = u32::try_from(every).ok()?;
    match frequency {
        "WEEKLY" => date.checked_add_days(chrono::Days::new(7 * u64::from(n))),
        "MONTHLY" => date.checked_add_months(Months::new(n)),
        "QUARTERLY" => date.checked_add_months(Months::new(3 * n)),
        "YEARLY" => date.checked_add_months(Months::new(12 * n)),
        _ => None,
    }
}

fn shift(date: NaiveDate, from: NaiveDate, to: NaiveDate) -> NaiveDate {
    date + (to - from)
}

/// A template's transaction dated `date` (keeping its due date the same distance after it).
enum Dated {
    Journal(JournalEntry),
    Expense(ExpenseInput),
    Bill(BillInput),
    Invoice(InvoiceInput),
}

fn dated(kind: &str, payload: &str, date: NaiveDate) -> Result<Dated, String> {
    let bad = |e: serde_json::Error| format!("REC_BAD_TEMPLATE: the saved transaction cannot be read ({e})");
    Ok(match kind {
        "JOURNAL" => {
            let mut e: JournalEntry = serde_json::from_str(payload).map_err(bad)?;
            e.id = None;
            e.date = date;
            Dated::Journal(e)
        }
        "EXPENSE" => {
            let mut e: ExpenseInput = serde_json::from_str(payload).map_err(bad)?;
            e.expense_date = date;
            Dated::Expense(e)
        }
        "BILL" => {
            let mut b: BillInput = serde_json::from_str(payload).map_err(bad)?;
            b.due_date = shift(b.due_date, b.bill_date, date);
            b.bill_date = date;
            Dated::Bill(b)
        }
        "INVOICE" => {
            let mut i: InvoiceInput = serde_json::from_str(payload).map_err(bad)?;
            i.due_date = shift(i.due_date, i.issue_date, date);
            i.issue_date = date;
            Dated::Invoice(i)
        }
        other => return Err(format!("REC_BAD_KIND: '{other}' cannot recur")),
    })
}

fn summary(kind: &str, payload: &str) -> (i64, Option<String>) {
    let today = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    match dated(kind, payload, today) {
        Ok(Dated::Journal(e)) => (e.lines.iter().map(|l| l.debit).sum(), None),
        Ok(Dated::Expense(e)) => (e.lines.iter().map(|l| l.amount_cents).sum(), e.payee_name),
        Ok(Dated::Bill(b)) => (b.total_cents, Some(b.vendor_name)),
        Ok(Dated::Invoice(i)) => (
            i.lines.iter().map(|l| crate::invoices::line_amount(l.quantity_e4, l.rate_cents).unwrap_or(0)).sum::<i64>() - i.discount_cents,
            Some(i.customer_name),
        ),
        Err(_) => (0, None),
    }
}

pub async fn list_recurring(pool: &SqlitePool) -> Result<Vec<RecurringView>, String> {
    let mut rows: Vec<RecurringView> = sqlx::query_as(
        "SELECT t.id, t.name, t.kind, t.frequency, t.every, t.next_date, t.end_date, t.is_active, t.payload,
                (SELECT MAX(run_date) FROM recurring_runs WHERE template_id = t.id) AS last_run_date,
                (SELECT COUNT(*) FROM recurring_runs WHERE template_id = t.id) AS run_count
         FROM recurring_templates t ORDER BY t.is_active DESC, t.next_date, t.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for row in &mut rows {
        (row.amount_cents, row.payee) = summary(&row.kind, &row.payload);
    }
    Ok(rows)
}

pub async fn save_recurring(pool: &SqlitePool, id: Option<i64>, input: RecurringInput) -> Result<i64, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("REC_NAME_REQUIRED: name the recurring transaction".into());
    }
    if advance(input.next_date, &input.frequency, input.every).is_none() {
        return Err("REC_BAD_SCHEDULE: choose weekly, monthly, quarterly or yearly".into());
    }
    let payload = input.payload.to_string();
    dated(&input.kind, &payload, input.next_date)?;
    let unique = |e: sqlx::Error| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("REC_DUPLICATE: there is already a recurring transaction named '{name}'") } else { msg }
    };
    match id {
        None => sqlx::query_scalar("INSERT INTO recurring_templates (name, kind, payload, frequency, every, next_date, end_date) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id")
            .bind(name)
            .bind(&input.kind)
            .bind(&payload)
            .bind(&input.frequency)
            .bind(input.every)
            .bind(input.next_date)
            .bind(input.end_date)
            .fetch_one(pool)
            .await
            .map_err(unique),
        Some(id) => sqlx::query("UPDATE recurring_templates SET name = ?1, kind = ?2, payload = ?3, frequency = ?4, every = ?5, next_date = ?6, end_date = ?7 WHERE id = ?8")
            .bind(name)
            .bind(&input.kind)
            .bind(&payload)
            .bind(&input.frequency)
            .bind(input.every)
            .bind(input.next_date)
            .bind(input.end_date)
            .bind(id)
            .execute(pool)
            .await
            .map(|_| id)
            .map_err(unique),
    }
}

pub async fn set_recurring_active(pool: &SqlitePool, id: i64, active: bool) -> Result<(), String> {
    sqlx::query("UPDATE recurring_templates SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// Deletes a template; transactions it already entered stay in the books.
pub async fn delete_recurring(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM recurring_templates WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// Posts one occurrence dated `date` through its document's normal path; returns its entry id.
async fn post_occurrence(tx: &mut Transaction<'_, Sqlite>, kind: &str, payload: &str, date: NaiveDate) -> Result<i64, String> {
    match dated(kind, payload, date)? {
        Dated::Journal(e) => post_entry_in_tx(tx, &e, EntrySource::Journal).await,
        Dated::Expense(e) => Ok(crate::expenses::write_expense(tx, &e).await?.1),
        Dated::Bill(b) => {
            let id = crate::expenses::write_bill(tx, &b).await?;
            sqlx::query_scalar("SELECT journal_entry_id FROM bills WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)
        }
        Dated::Invoice(i) => {
            let id = crate::invoices::write_invoice(tx, &i).await?;
            sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)
        }
    }
}

/// Enters every occurrence that is due on or before `as_of` for active templates (at most 120 in
/// one go), moving each template's next date along its schedule. Returns how many were entered.
pub async fn enter_due(pool: &SqlitePool, as_of: NaiveDate, only: Option<i64>) -> Result<usize, String> {
    run(pool, async |tx| {
        let templates: Vec<(i64, String, String, String, i64, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
            "SELECT id, kind, payload, frequency, every, next_date, end_date FROM recurring_templates
             WHERE is_active = 1 AND next_date <= ?1 AND (?2 IS NULL OR id = ?2)",
        )
        .bind(as_of)
        .bind(only)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
        let mut entered = 0;
        for (id, kind, payload, frequency, every, mut next, end) in templates {
            while next <= as_of && end.is_none_or(|e| next <= e) && entered < 120 {
                let entry = post_occurrence(tx, &kind, &payload, next).await?;
                sqlx::query("INSERT INTO recurring_runs (template_id, run_date, journal_entry_id) VALUES (?1, ?2, ?3)").bind(id).bind(next).bind(entry).execute(&mut **tx).await.map_err(db_err)?;
                entered += 1;
                next = advance(next, &frequency, every).ok_or("REC_BAD_SCHEDULE: the schedule cannot advance")?;
            }
            let finished = end.is_some_and(|e| next > e);
            sqlx::query("UPDATE recurring_templates SET next_date = ?1, is_active = CASE WHEN ?2 THEN 0 ELSE is_active END WHERE id = ?3")
                .bind(next)
                .bind(finished)
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
        Ok(entered)
    })
    .await
}

// ------------------------------------------------------------------------------------------------
// Purchase orders
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLineInput {
    pub account_id: i64,
    #[serde(default)]
    pub product_service_id: Option<i64>,
    pub description: Option<String>,
    #[serde(default)]
    pub quantity_e4: Option<i64>,
    #[serde(default)]
    pub rate_cents: Option<i64>,
    pub amount_cents: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderInput {
    pub vendor_name: String,
    pub po_date: NaiveDate,
    #[serde(default)]
    pub expected_date: Option<NaiveDate>,
    pub memo: Option<String>,
    pub lines: Vec<PurchaseOrderLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderView {
    pub id: i64,
    pub po_number: i64,
    pub vendor_id: i64,
    pub vendor_name: String,
    pub po_date: String,
    pub expected_date: Option<String>,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub status: String,
    pub bill_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLineView {
    pub account_id: i64,
    pub product_service_id: Option<i64>,
    pub description: Option<String>,
    pub quantity_e4: Option<i64>,
    pub rate_cents: Option<i64>,
    pub amount_cents: i64,
    pub class_id: Option<i64>,
}

const PO_SELECT: &str = "SELECT o.id, o.po_number, o.vendor_id, v.name AS vendor_name, o.po_date, o.expected_date, o.memo, o.total_cents, o.status, o.bill_id
 FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id";

pub async fn list_purchase_orders(pool: &SqlitePool) -> Result<Vec<PurchaseOrderView>, String> {
    sqlx::query_as::<_, PurchaseOrderView>(&format!("{PO_SELECT} ORDER BY o.status = 'OPEN' DESC, o.po_number DESC")).fetch_all(pool).await.map_err(db_err)
}

pub async fn purchase_order_lines(pool: &SqlitePool, id: i64) -> Result<Vec<PurchaseOrderLineView>, String> {
    sqlx::query_as::<_, PurchaseOrderLineView>(
        "SELECT account_id, product_service_id, description, quantity_e4, rate_cents, amount_cents, class_id FROM purchase_order_lines WHERE purchase_order_id = ?1 ORDER BY line_number",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Records a purchase order. It posts nothing; the bill does when the order arrives.
pub async fn create_purchase_order(pool: &SqlitePool, input: PurchaseOrderInput) -> Result<i64, String> {
    if input.lines.is_empty() {
        return Err("PO_NO_ITEMS: add at least one line".into());
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&line.amount_cents) {
            return Err(format!("PO_BAD_AMOUNT: line {} needs an amount above zero", idx + 1));
        }
        total = total.checked_add(line.amount_cents).filter(|t| *t <= MAX_LINE_CENTS).ok_or_else(|| format!("PO_TOO_LARGE: the order exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    run(pool, async |tx| {
        let vendor = crate::sales::find_or_create_named(tx, crate::sales::Party::Vendor, &input.vendor_name).await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO purchase_orders (po_number, vendor_id, po_date, expected_date, memo, total_cents)
             VALUES ((SELECT COALESCE(MAX(po_number), 0) + 1 FROM purchase_orders), ?1, ?2, ?3, ?4, ?5) RETURNING id",
        )
        .bind(vendor)
        .bind(input.po_date)
        .bind(input.expected_date)
        .bind(clean(input.memo.as_deref()))
        .bind(total)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        for (idx, line) in input.lines.iter().enumerate() {
            sqlx::query(
                "INSERT INTO purchase_order_lines (purchase_order_id, line_number, account_id, product_service_id, description, quantity_e4, rate_cents, amount_cents, class_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .bind(id)
            .bind(idx as i64 + 1)
            .bind(line.account_id)
            .bind(line.product_service_id)
            .bind(clean(line.description.as_deref()))
            .bind(line.quantity_e4)
            .bind(line.rate_cents)
            .bind(line.amount_cents)
            .bind(line.class_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        }
        Ok(id)
    })
    .await
}

/// Closes an open order without billing it, or reopens a closed one.
pub async fn set_purchase_order_closed(pool: &SqlitePool, id: i64, closed: bool) -> Result<(), String> {
    let done = sqlx::query("UPDATE purchase_orders SET status = ?1 WHERE id = ?2 AND status = ?3")
        .bind(if closed { "CLOSED" } else { "OPEN" })
        .bind(id)
        .bind(if closed { "OPEN" } else { "CLOSED" })
        .execute(pool)
        .await
        .map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("PO_NOT_CHANGEABLE: only open orders can be closed, and only closed ones reopened".into());
    }
    Ok(())
}

/// Turns an open purchase order into a posted bill with the same lines; returns the bill id.
pub async fn bill_purchase_order(pool: &SqlitePool, id: i64, bill_date: NaiveDate, due_date: NaiveDate, bill_number: Option<String>) -> Result<i64, String> {
    let lines = purchase_order_lines(pool, id).await?;
    run(pool, async |tx| {
        let head: Option<(String, i64, String, Option<String>, i64)> = sqlx::query_as(
            "SELECT v.name, o.total_cents, o.status, o.memo, o.po_number FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id WHERE o.id = ?1",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (vendor, total, status, memo, number) = head.ok_or("PO_NOT_FOUND: that purchase order does not exist")?;
        if status != "OPEN" {
            return Err("PO_NOT_OPEN: only an open purchase order can be billed".into());
        }
        let bill = BillInput {
            vendor_name: vendor,
            bill_number: clean(bill_number.as_deref()).map(str::to_string),
            bill_date,
            due_date,
            memo: Some(memo.unwrap_or_else(|| format!("PO #{number}"))),
            total_cents: total,
            lines: lines
                .iter()
                .map(|l| BillLineInput {
                    account_id: l.account_id,
                    amount_cents: l.amount_cents,
                    memo: l.description.clone(),
                    quantity_e4: l.quantity_e4,
                    rate_cents: l.rate_cents,
                    product_service_id: l.product_service_id,
                    class_id: l.class_id,
                })
                .collect(),
        };
        let bill_id = crate::expenses::write_bill(tx, &bill).await?;
        sqlx::query("UPDATE purchase_orders SET status = 'BILLED', bill_id = ?1 WHERE id = ?2").bind(bill_id).bind(id).execute(&mut **tx).await.map_err(db_err)?;
        Ok(bill_id)
    })
    .await
}

// ------------------------------------------------------------------------------------------------
// Search
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// `TRANSACTION`, `CUSTOMER`, `VENDOR`, `PRODUCT` or `ACCOUNT`.
    pub kind: String,
    pub id: i64,
    pub label: String,
    pub detail: Option<String>,
    pub date: Option<String>,
    pub amount_cents: Option<i64>,
    pub source_type: Option<String>,
}

/// Finds transactions (by number, name, memo, reference or exact amount) and list entries (by
/// name) matching `query`; at most 60 results, newest transactions first.
pub async fn search(pool: &SqlitePool, query: &str) -> Result<Vec<SearchHit>, String> {
    let q = query.trim();
    if q.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let amount = q.trim_start_matches('$').replace(',', "");
    let amount_cents = parse_amount(&amount);
    let number: Option<i64> = q.trim_start_matches('#').parse().ok();
    sqlx::query_as::<_, SearchHit>(&format!(
        "SELECT * FROM (
            SELECT 'TRANSACTION' AS kind, e.id, COALESCE(p.party_name, e.memo, 'Journal entry') AS label, e.memo AS detail, e.entry_date AS date,
                   (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id) AS amount_cents, {DISPLAY_SOURCE_TYPE} AS source_type
            FROM journal_entries e LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
            WHERE e.status = 'POSTED' AND e.reverses_entry_id IS NULL AND (
                   e.memo LIKE ?1 ESCAPE '\\' OR p.party_name LIKE ?1 ESCAPE '\\' OR e.entry_number = ?3
                OR (e.source_type = 'INVOICE' AND (SELECT invoice_number FROM invoices WHERE id = e.source_id) = ?3)
                OR (e.source_type IN ('CHECK', 'EXPENSE') AND (SELECT check_number FROM expenses WHERE id = e.source_id) = ?4)
                OR (e.source_type = 'BILL' AND (SELECT bill_number FROM bills WHERE id = e.source_id) LIKE ?1 ESCAPE '\\')
                OR (?2 IS NOT NULL AND (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id) = ?2)
                OR EXISTS (SELECT 1 FROM journal_lines l WHERE l.journal_entry_id = e.id AND l.memo LIKE ?1 ESCAPE '\\'))
            ORDER BY e.entry_date DESC, e.id DESC LIMIT 40)
         UNION ALL SELECT * FROM (SELECT 'CUSTOMER', id, name, email, NULL, NULL, NULL FROM customers WHERE name LIKE ?1 ESCAPE '\\' OR company_name LIKE ?1 ESCAPE '\\' LIMIT 8)
         UNION ALL SELECT * FROM (SELECT 'VENDOR', id, name, email, NULL, NULL, NULL FROM vendors WHERE name LIKE ?1 ESCAPE '\\' OR company_name LIKE ?1 ESCAPE '\\' LIMIT 8)
         UNION ALL SELECT * FROM (SELECT 'PRODUCT', id, name, description, NULL, rate_cents, NULL FROM products_services WHERE name LIKE ?1 ESCAPE '\\' LIMIT 6)
         UNION ALL SELECT * FROM (SELECT 'ACCOUNT', id, full_name, account_type_name, NULL, NULL, NULL FROM v_chart_of_accounts
                                  WHERE full_name LIKE ?1 ESCAPE '\\' OR account_number = ?4 LIMIT 6)"
    ))
    .bind(&like)
    .bind(amount_cents)
    .bind(number)
    .bind(q.trim_start_matches('#'))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Attachments
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub id: i64,
    pub journal_entry_id: i64,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub created_at: String,
}

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

fn media_type(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("csv") => "text/csv",
        Some("txt") => "text/plain",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

/// Keeps a copy of a file (a receipt, a signed contract) in the company file, attached to a
/// posted transaction.
pub async fn add_attachment(pool: &SqlitePool, journal_entry_id: i64, file_name: &str, content: &[u8]) -> Result<AttachmentView, String> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let name = std::path::Path::new(file_name).file_name().and_then(|n| n.to_str()).unwrap_or("attachment");
    let posted: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journal_entries WHERE id = ?1 AND status = 'POSTED')")
        .bind(journal_entry_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !posted {
        return Err("ATTACH_NOT_POSTED: attach files to a posted transaction".into());
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO attachments (journal_entry_id, file_name, media_type, size_bytes, content) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id")
        .bind(journal_entry_id)
        .bind(name)
        .bind(media_type(name))
        .bind(content.len() as i64)
        .bind(content)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    list_attachments(pool, journal_entry_id).await?.into_iter().find(|a| a.id == id).ok_or_else(|| "ATTACH_NOT_SAVED: the file was not saved".into())
}

pub async fn list_attachments(pool: &SqlitePool, journal_entry_id: i64) -> Result<Vec<AttachmentView>, String> {
    sqlx::query_as::<_, AttachmentView>(
        "SELECT id, journal_entry_id, file_name, media_type, size_bytes, created_at FROM attachments WHERE journal_entry_id = ?1 ORDER BY id",
    )
    .bind(journal_entry_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn attachment_content(pool: &SqlitePool, id: i64) -> Result<(String, Vec<u8>), String> {
    sqlx::query_as("SELECT file_name, content FROM attachments WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "ATTACH_NOT_FOUND: that attachment does not exist".into())
}

pub async fn delete_attachment(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM attachments WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// "1234.5" → 123450 cents; anything else is not an amount.
fn parse_amount(text: &str) -> Option<i64> {
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 2 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let cents: i64 = format!("{frac:0<2}").parse().ok()?;
    whole.parse::<i64>().ok()?.checked_mul(100)?.checked_add(cents)
}

#[cfg(test)]
mod tests;
