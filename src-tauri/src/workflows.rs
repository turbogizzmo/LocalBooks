//! Everyday workflows from classic desktop accounting software: customer statements, the 1099
//! report, recurring (memorized) transactions, purchase orders, finding transactions, and files
//! attached to transactions.

use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::expenses::{BillInput, BillLineInput, ExpenseInput};
use crate::invoices::InvoiceInput;
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, DISPLAY_SOURCE_TYPE, MAX_LINE_CENTS};
use crate::money::Cents;

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
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
    /// The customer's P.O. number on an invoice or credit memo.
    pub po_number: Option<String>,
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
         JOIN v_entry_party p ON p.journal_entry_id = l.journal_entry_id
         JOIN journal_entries je ON je.id = l.journal_entry_id";
    let opening: i64 = sqlx::query_scalar(&format!("SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) {ar_lines} WHERE {mine} AND l.entry_date < ?2"))
        .bind(&name)
        .bind(start_date)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let mut rows: Vec<StatementRow> = sqlx::query_as(&format!(
        "SELECT l.entry_date AS date,
                CASE WHEN je.reverses_entry_id IS NOT NULL THEN 'VOID'
                     WHEN l.source_type = 'INVOICE' AND EXISTS (SELECT 1 FROM finance_charge_invoices WHERE invoice_id = l.source_id)
                     THEN 'FINANCE_CHARGE'
                     WHEN l.source_type = 'INVOICE' AND EXISTS (SELECT 1 FROM statement_charge_invoices WHERE invoice_id = l.source_id)
                     THEN 'STATEMENT_CHARGE' ELSE l.source_type END AS source_type,
                p.party_name,
                CASE l.source_type
                    -- A statement charge has no invoice number of its own to show.
                    WHEN 'INVOICE' THEN (SELECT CAST(invoice_number AS TEXT) FROM invoices WHERE id = l.source_id
                                         AND NOT EXISTS (SELECT 1 FROM statement_charge_invoices WHERE invoice_id = l.source_id))
                    WHEN 'CREDIT_MEMO' THEN (SELECT CAST(credit_number AS TEXT) FROM credit_memos WHERE id = l.source_id)
                    WHEN 'RECEIVE_PAYMENT' THEN (SELECT reference FROM invoice_payments WHERE id = l.source_id)
                END AS number,
                CASE l.source_type
                    WHEN 'INVOICE' THEN (SELECT po_number FROM invoices WHERE id = l.source_id)
                    WHEN 'CREDIT_MEMO' THEN (SELECT po_number FROM credit_memos WHERE id = l.source_id)
                END AS po_number,
                CASE WHEN l.source_type = 'INVOICE' AND EXISTS (SELECT 1 FROM statement_charge_invoices WHERE invoice_id = l.source_id)
                     THEN (SELECT description FROM invoice_items WHERE invoice_id = l.source_id ORDER BY line_number LIMIT 1)
                     ELSE l.memo END AS memo,
                l.debit_cents - l.credit_cents AS amount_cents
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

/// A box on Form 1099-NEC or 1099-MISC that expense accounts can be mapped to
/// (Preferences › Tax: 1099).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Box1099 {
    pub code: &'static str,
    pub form: &'static str,
    pub label: &'static str,
}

pub const BOXES_1099: &[Box1099] = &[
    Box1099 { code: "NEC-1", form: "1099-NEC", label: "Box 1: Nonemployee compensation" },
    Box1099 { code: "MISC-1", form: "1099-MISC", label: "Box 1: Rents" },
    Box1099 { code: "MISC-2", form: "1099-MISC", label: "Box 2: Royalties" },
    Box1099 { code: "MISC-3", form: "1099-MISC", label: "Box 3: Other income" },
    Box1099 { code: "MISC-6", form: "1099-MISC", label: "Box 6: Medical and health care payments" },
    Box1099 { code: "MISC-10", form: "1099-MISC", label: "Box 10: Gross proceeds paid to an attorney" },
];

/// The 1099-NEC reporting threshold for payments made before 2026.
pub const NEC_THRESHOLD_CENTS: i64 = 60_000;
/// From payments made in 2026 the NEC and most MISC thresholds are $2,000 (Public Law 119-21,
/// section 70433, amending 26 U.S.C. 6041(a) and 6041A(a)); later years are indexed for inflation,
/// which the IRS has not yet published, so they use $2,000 too.
pub const THRESHOLD_FROM_2026_CENTS: i64 = 200_000;

/// The amount at which a box is reported for payments in `year`: royalties at $10, attorneys'
/// gross proceeds at $600 (26 U.S.C. 6045(f), unchanged), the rest at $600 before 2026 and $2,000 from 2026.
pub fn threshold_1099_cents(code: &str, year: i32) -> i64 {
    match code {
        "MISC-2" => 1_000,
        "MISC-10" => 60_000,
        _ if year >= 2026 => THRESHOLD_FROM_2026_CENTS,
        _ => NEC_THRESHOLD_CENTS,
    }
}

/// What a vendor was paid in one box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxAmount1099 {
    pub code: String,
    pub label: String,
    pub paid_cents: i64,
    pub threshold_cents: i64,
    pub over_threshold: bool,
}

/// One payment's share for one expense account: a line of the 1099 Detail report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail1099 {
    pub date: NaiveDate,
    /// `CHECK`, `EXPENSE` or `BILL_PAYMENT`.
    pub kind: String,
    pub number: Option<String>,
    pub account_name: String,
    pub box_code: String,
    pub amount_cents: i64,
}

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
    /// Everything reportable in the year, across boxes.
    #[sqlx(skip)]
    pub paid_cents: i64,
    /// Over the threshold in at least one box.
    #[sqlx(skip)]
    pub over_threshold: bool,
    #[sqlx(skip)]
    pub boxes: Vec<BoxAmount1099>,
    #[sqlx(skip)]
    pub details: Vec<Detail1099>,
}

/// A payment piece before mapping: vendor, date, kind, number, payment id, account, account name,
/// line amount, document total, amount paid in home currency.
type Piece = (i64, NaiveDate, String, Option<String>, i64, i64, String, i64, i64, i64);

/// Payments to vendors marked for a 1099 in a calendar year, from bank accounts (card payments are
/// reported by the card company, so they are left out): checks and expenses paid from a bank, and
/// bill payments from a bank, by payment date.
///
/// Each payment is split over the expense accounts of what it paid (an expense's own lines, or the
/// lines of the bill a payment paid, in proportion), and each account goes to the 1099 box it is
/// mapped to in Preferences › Tax: 1099. Accounts not mapped are left out. With no account mapped
/// at all, everything counts as nonemployee compensation (1099-NEC box 1), as before mapping existed.
pub async fn vendors_1099(pool: &SqlitePool, year: i32, access: crate::tax_ids::Access<'_>) -> Result<Vec<Vendor1099Row>, String> {
    let (start, end) = (NaiveDate::from_ymd_opt(year, 1, 1).ok_or("RPT_BAD_YEAR: invalid year")?, NaiveDate::from_ymd_opt(year, 12, 31).ok_or("RPT_BAD_YEAR: invalid year")?);
    let mapping = crate::preferences::tax_time::accounts_1099(pool).await?;
    let mut rows: Vec<Vendor1099Row> = sqlx::query_as("SELECT v.id AS vendor_id, v.name AS vendor_name, v.tax_id, v.tax_id_masked, v.address FROM vendors v WHERE v.is_1099 = 1 ORDER BY v.name COLLATE NOCASE")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let pieces: Vec<Piece> = sqlx::query_as(
        "SELECT x.vendor_id, x.expense_date, x.kind, x.check_number, x.id, l.account_id, la.full_name, l.amount_cents, x.total_cents, COALESCE(x.home_total_cents, x.total_cents)
         FROM expenses x JOIN chart_of_accounts a ON a.id = x.payment_account_id JOIN expense_lines l ON l.expense_id = x.id JOIN v_chart_of_accounts la ON la.id = l.account_id
         WHERE x.vendor_id IN (SELECT id FROM vendors WHERE is_1099 = 1) AND x.is_voided = 0 AND a.account_type = 'BANK' AND x.expense_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT b.vendor_id, p.payment_date, 'BILL_PAYMENT', p.reference, -p.id, l.account_id, la.full_name, l.amount_cents, b.total_cents, p.amount_cents
         FROM bill_payments p JOIN bills b ON b.id = p.bill_id JOIN chart_of_accounts a ON a.id = p.payment_account_id JOIN bill_lines l ON l.bill_id = b.id JOIN v_chart_of_accounts la ON la.id = l.account_id
         WHERE b.vendor_id IN (SELECT id FROM vendors WHERE is_1099 = 1) AND p.is_voided = 0 AND a.account_type = 'BANK' AND p.payment_date BETWEEN ?1 AND ?2
         ORDER BY 1, 2, 5, 6",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    // Split each payment over its lines with running totals, so the shares add up to the payment.
    let overflow = || "RPT_OVERFLOW: the 1099 amounts are out of range".to_string();
    let mut details: std::collections::BTreeMap<i64, Vec<Detail1099>> = std::collections::BTreeMap::new();
    let (mut current, mut cumulative, mut allocated) = (None, 0i64, 0i64);
    for (vendor, date, kind, number, payment, account, account_name, line, total, paid) in pieces {
        if current != Some(payment) {
            (current, cumulative, allocated) = (Some(payment), 0, 0);
        }
        cumulative = cumulative.checked_add(line).ok_or_else(overflow)?;
        let upto = if total == 0 { 0 } else { crate::ledger::mul_div_round(paid, cumulative, total).ok_or_else(overflow)? };
        let amount = upto - allocated;
        allocated = upto;
        let code = if mapping.is_empty() { Some("NEC-1".to_string()) } else { mapping.get(&account).cloned() };
        let Some(box_code) = code else { continue };
        if amount != 0 {
            details.entry(vendor).or_default().push(Detail1099 { date, kind, number, account_name, box_code, amount_cents: amount });
        }
    }

    for row in &mut rows {
        let lines = details.remove(&row.vendor_id).unwrap_or_default();
        for b in BOXES_1099 {
            let paid = lines.iter().filter(|d| d.box_code == b.code).try_fold(0i64, |s, d| s.checked_add(d.amount_cents)).ok_or_else(overflow)?;
            if paid != 0 {
                let threshold = threshold_1099_cents(b.code, year);
                row.boxes.push(BoxAmount1099 { code: b.code.into(), label: format!("{} {}", b.form, b.label), paid_cents: paid, threshold_cents: threshold, over_threshold: paid >= threshold });
            }
        }
        row.paid_cents = row.boxes.iter().try_fold(0i64, |s, b| s.checked_add(b.paid_cents)).ok_or_else(overflow)?;
        row.over_threshold = row.boxes.iter().any(|b| b.over_threshold);
        row.details = lines;
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
    /// The day of the month later dates fall on: the memorized transaction's own day, when the next
    /// date is that day moved to a shorter month (a Jan 31 transaction first repeats on Feb 28).
    /// Left out, or not matching the next date, it is the next date's day.
    #[serde(default)]
    pub anchor_day: Option<u32>,
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

/// The date after `date` on a schedule. A monthly, quarterly or yearly schedule falls on its
/// `anchor_day` (the day of the month it started on), or on the last day of a shorter month: from
/// January 31 it goes to February 28 and then back to March 31, never staying on the 28th.
pub fn advance(date: NaiveDate, frequency: &str, every: i64, anchor_day: Option<u32>) -> Option<NaiveDate> {
    let n = u32::try_from(every).ok()?;
    let months = match frequency {
        "WEEKLY" => return date.checked_add_days(chrono::Days::new(7 * u64::from(n))),
        "MONTHLY" => n,
        "QUARTERLY" => n.checked_mul(3)?,
        "YEARLY" => n.checked_mul(12)?,
        _ => return None,
    };
    let moved = date.checked_add_months(Months::new(months))?;
    let Some(day) = anchor_day else { return Some(moved) };
    let first = moved.with_day(1)?;
    let last = first.checked_add_months(Months::new(1))?.pred_opt()?.day();
    first.with_day(day.clamp(1, last))
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
        Ok(Dated::Journal(e)) => (e.lines.iter().map(|l| l.debit.get()).sum(), None),
        Ok(Dated::Expense(e)) => (e.lines.iter().map(|l| l.amount_cents).sum(), e.payee_name),
        Ok(Dated::Bill(b)) => (b.total_cents, Some(b.vendor_name)),
        Ok(Dated::Invoice(i)) => (
            Cents::checked_sum(i.lines.iter().map(|l| Cents(crate::invoices::line_amount(l.quantity_e4, l.rate_cents.get()).unwrap_or(0))))
                .and_then(|t| t.checked_sub(i.discount_cents))
                .map_or(0, Cents::get),
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
    if advance(input.next_date, &input.frequency, input.every, None).is_none() {
        return Err("REC_BAD_SCHEDULE: choose weekly, monthly, quarterly or yearly".into());
    }
    let payload = input.payload.to_string();
    dated(&input.kind, &payload, input.next_date)?;
    // Later dates fall on the next date's day of the month (or the month's last day). Saving a
    // template without moving its next date keeps the day it already has (a schedule from the 31st
    // sitting on February 28 stays a month-end schedule).
    let anchor = i64::from(anchor_day(input.next_date, input.anchor_day));
    let unique = |e: sqlx::Error| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("REC_DUPLICATE: there is already a recurring transaction named '{name}'") } else { msg }
    };
    match id {
        None => sqlx::query_scalar("INSERT INTO recurring_templates (name, kind, payload, frequency, every, next_date, end_date, anchor_day) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id")
            .bind(name)
            .bind(&input.kind)
            .bind(&payload)
            .bind(&input.frequency)
            .bind(input.every)
            .bind(input.next_date)
            .bind(input.end_date)
            .bind(anchor)
            .fetch_one(pool)
            .await
            .map_err(unique),
        Some(id) => sqlx::query("UPDATE recurring_templates SET name = ?1, kind = ?2, payload = ?3, frequency = ?4, every = ?5, next_date = ?6, end_date = ?7,
                    anchor_day = CASE WHEN next_date = ?6 AND anchor_day IS NOT NULL THEN anchor_day ELSE ?9 END WHERE id = ?8")
            .bind(name)
            .bind(&input.kind)
            .bind(&payload)
            .bind(&input.frequency)
            .bind(input.every)
            .bind(input.next_date)
            .bind(input.end_date)
            .bind(id)
            .bind(anchor)
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

/// The day of the month a schedule falls on: `chosen` when `next` is that day, or the last day of a
/// month too short for it; otherwise `next`'s own day.
fn anchor_day(next: NaiveDate, chosen: Option<u32>) -> u32 {
    let last = next.with_day(1).and_then(|first| first.checked_add_months(Months::new(1))).and_then(|d| d.pred_opt()).map_or(31, |d| d.day());
    match chosen {
        Some(day) if (1..=31).contains(&day) && day.min(last) == next.day() => day,
        _ => next.day(),
    }
}

/// A template that may be due: id, kind, payload, frequency, every, next date, end date, anchor day.
type DueTemplate = (i64, String, String, String, i64, NaiveDate, Option<NaiveDate>, Option<i64>);

/// Enters every occurrence that is due on or before `as_of` for active templates (at most 120 in
/// one go), moving each template's next date along its schedule. Returns how many were entered.
pub async fn enter_due(pool: &SqlitePool, as_of: NaiveDate, only: Option<i64>) -> Result<usize, String> {
    run(pool, async |tx| {
        let templates: Vec<DueTemplate> = sqlx::query_as(
            "SELECT id, kind, payload, frequency, every, next_date, end_date, anchor_day FROM recurring_templates
             WHERE is_active = 1 AND next_date <= ?1 AND (?2 IS NULL OR id = ?2)",
        )
        .bind(as_of)
        .bind(only)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
        let mut entered = 0;
        for (id, kind, payload, frequency, every, mut next, end, anchor) in templates {
            let anchor = anchor.and_then(|d| u32::try_from(d).ok());
            while next <= as_of && end.is_none_or(|e| next <= e) && entered < 120 {
                let entry = post_occurrence(tx, &kind, &payload, next).await?;
                sqlx::query("INSERT INTO recurring_runs (template_id, run_date, journal_entry_id) VALUES (?1, ?2, ?3)").bind(id).bind(next).bind(entry).execute(&mut **tx).await.map_err(db_err)?;
                entered += 1;
                next = advance(next, &frequency, every, anchor).ok_or("REC_BAD_SCHEDULE: the schedule cannot advance")?;
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
    /// The customer or job the line is for; it carries over to the bill.
    #[serde(default)]
    pub customer_id: Option<i64>,
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
    /// Discount and charge lines among the split lines (`purchase_lines`).
    #[serde(default)]
    pub form_lines: Vec<crate::purchase_lines::PurchaseFormLineInput>,
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
    pub customer_id: Option<i64>,
}

const PO_SELECT: &str = "SELECT o.id, o.po_number, o.vendor_id, v.name AS vendor_name, o.po_date, o.expected_date, o.memo, o.total_cents, o.status, o.bill_id
 FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id";

pub async fn list_purchase_orders(pool: &SqlitePool) -> Result<Vec<PurchaseOrderView>, String> {
    sqlx::query_as::<_, PurchaseOrderView>(&format!("{PO_SELECT} ORDER BY o.status = 'OPEN' DESC, o.po_number DESC")).fetch_all(pool).await.map_err(db_err)
}

pub async fn purchase_order_lines(pool: &SqlitePool, id: i64) -> Result<Vec<PurchaseOrderLineView>, String> {
    sqlx::query_as::<_, PurchaseOrderLineView>(
        "SELECT account_id, product_service_id, description, quantity_e4, rate_cents, amount_cents, class_id, customer_id FROM purchase_order_lines WHERE purchase_order_id = ?1 ORDER BY line_number",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Records a purchase order. It posts nothing; the bill does when the order arrives.
pub async fn create_purchase_order(pool: &SqlitePool, input: PurchaseOrderInput) -> Result<i64, String> {
    crate::inventory::require_enabled(pool).await?;
    let total = validate_purchase_order(&input)?;
    run(pool, async |tx| write_purchase_order(tx, &input, total).await).await
}

/// A purchase order's total, or why it can't be recorded.
pub fn validate_purchase_order(input: &PurchaseOrderInput) -> Result<i64, String> {
    if input.vendor_name.trim().is_empty() {
        return Err("PO_NO_VENDOR: enter a vendor".into());
    }
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
    let splits: Vec<i64> = input.lines.iter().map(|l| l.amount_cents).collect();
    let (_, total) = crate::purchase_lines::lay_out(&splits, &input.form_lines, "PO")?;
    Ok(total)
}

/// Writes a validated purchase order (`total` from [`validate_purchase_order`]) in `tx`.
pub(crate) async fn write_purchase_order(tx: &mut Transaction<'_, Sqlite>, input: &PurchaseOrderInput, total: i64) -> Result<i64, String> {
    {
        let splits: Vec<i64> = input.lines.iter().map(|l| l.amount_cents).collect();
        let (form_amounts, _) = crate::purchase_lines::lay_out(&splits, &input.form_lines, "PO")?;
        crate::purchase_lines::check(tx, &input.form_lines, "PO").await?;
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
        write_purchase_order_lines(tx, id, input, &form_amounts).await?;
        Ok(id)
    }
}

/// Writes an order's lines and its discount and charge lines.
async fn write_purchase_order_lines(tx: &mut Transaction<'_, Sqlite>, id: i64, input: &PurchaseOrderInput, form_amounts: &[i64]) -> Result<(), String> {
    {
        for (idx, line) in input.lines.iter().enumerate() {
            sqlx::query(
                "INSERT INTO purchase_order_lines (purchase_order_id, line_number, account_id, product_service_id, description, quantity_e4, rate_cents, amount_cents, class_id, customer_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
            .bind(line.customer_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        }
        crate::purchase_lines::write(tx, crate::purchase_lines::PurchaseDoc::PurchaseOrder, id, &input.form_lines, form_amounts).await?;
        Ok(())
    }
}

/// Whether an order can still be changed or deleted: open (or closed by hand), with nothing
/// received against it and not billed. Its number comes back for the messages.
async fn changeable_order(tx: &mut Transaction<'_, Sqlite>, id: i64, verb: &str) -> Result<i64, String> {
    let head: Option<(i64, String)> = sqlx::query_as("SELECT po_number, status FROM purchase_orders WHERE id = ?1").bind(id).fetch_optional(&mut **tx).await.map_err(db_err)?;
    let (number, status) = head.ok_or("PO_NOT_FOUND: that purchase order does not exist")?;
    if status == "BILLED" {
        return Err(format!("PO_BILLED: purchase order #{number} has been billed and can't be {verb}; void the bill first"));
    }
    // Any receipt against it, even a voided one, still points at the order and its lines.
    let received: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM item_receipts WHERE purchase_order_id = ?1)").bind(id).fetch_one(&mut **tx).await.map_err(db_err)?;
    if received {
        return Err(format!("PO_HAS_RECEIPTS: items have been received against purchase order #{number} (a voided receipt counts), so it can't be {verb}; close it, or make a new order"));
    }
    Ok(number)
}

/// Saves changes to an open purchase order: vendor, dates, memo and lines. A purchase order posts
/// nothing, so it is changed in place and keeps its number.
pub async fn update_purchase_order(pool: &SqlitePool, id: i64, input: PurchaseOrderInput) -> Result<(), String> {
    crate::inventory::require_enabled(pool).await?;
    let total = validate_purchase_order(&input)?;
    run(pool, async |tx| {
        let number = changeable_order(tx, id, "changed").await?;
        let status: String = sqlx::query_scalar("SELECT status FROM purchase_orders WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)?;
        if status != "OPEN" {
            return Err(format!("PO_NOT_OPEN: purchase order #{number} is closed; reopen it to change it"));
        }
        let splits: Vec<i64> = input.lines.iter().map(|l| l.amount_cents).collect();
        let (form_amounts, _) = crate::purchase_lines::lay_out(&splits, &input.form_lines, "PO")?;
        crate::purchase_lines::check(tx, &input.form_lines, "PO").await?;
        let vendor = crate::sales::find_or_create_named(tx, crate::sales::Party::Vendor, &input.vendor_name).await?;
        sqlx::query("UPDATE purchase_orders SET vendor_id = ?1, po_date = ?2, expected_date = ?3, memo = ?4, total_cents = ?5 WHERE id = ?6")
            .bind(vendor)
            .bind(input.po_date)
            .bind(input.expected_date)
            .bind(clean(input.memo.as_deref()))
            .bind(total)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM purchase_form_lines WHERE purchase_order_id = ?1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        sqlx::query("DELETE FROM purchase_order_lines WHERE purchase_order_id = ?1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        write_purchase_order_lines(tx, id, &input, &form_amounts).await
    })
    .await
}

/// Deletes a purchase order that nothing was received or billed against. Files attached to it
/// must be deleted first, so none is lost with it.
pub async fn delete_purchase_order(pool: &SqlitePool, id: i64) -> Result<(), String> {
    run(pool, async |tx| {
        let number = changeable_order(tx, id, "deleted").await?;
        let files: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM record_attachments WHERE record_kind = 'PURCHASE_ORDER' AND record_id = ?1 AND deleted_at IS NULL)")
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
        if files {
            return Err(format!("PO_HAS_FILES: purchase order #{number} has attached files; delete them first"));
        }
        sqlx::query("DELETE FROM purchase_orders WHERE id = ?1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        Ok(())
    })
    .await
}

/// Closes an open order without billing it, or reopens a closed one.
pub async fn set_purchase_order_closed(pool: &SqlitePool, id: i64, closed: bool) -> Result<(), String> {
    // An order closed because everything on it arrived stays closed: reopened, it would show as
    // open with nothing to receive, and Receive Items would take more than was ordered.
    if !closed {
        let mut conn = pool.acquire().await.map_err(db_err)?;
        if crate::item_receipts::received_in_full(&mut conn, id).await? {
            let number: i64 = sqlx::query_scalar("SELECT po_number FROM purchase_orders WHERE id = ?1").bind(id).fetch_one(&mut *conn).await.map_err(db_err)?;
            return Err(format!("PO_RECEIVED_IN_FULL: everything on purchase order #{number} has been received; void a receipt to receive it again"));
        }
    }
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

/// A purchase order's vendor, total, status, memo, number and the vendor's terms.
type OrderHead = (String, i64, String, Option<String>, i64, Option<i64>);

/// The bill an open purchase order becomes, with the same lines, discounts and charges, and the
/// vendor's terms (so Pay Bills offers its early-payment discount). Nothing is written: the screen's
/// command runs the bill checks and approval rules on it first, as Enter Bills does.
pub async fn purchase_order_bill(pool: &SqlitePool, id: i64, bill_date: NaiveDate, due_date: NaiveDate, bill_number: Option<String>) -> Result<BillInput, String> {
    let head: Option<OrderHead> = sqlx::query_as(
        "SELECT v.name, o.total_cents, o.status, o.memo, o.po_number, v.terms_id FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id WHERE o.id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (vendor, total, status, memo, number, terms_id) = head.ok_or("PO_NOT_FOUND: that purchase order does not exist")?;
    if status != "OPEN" {
        return Err("PO_NOT_OPEN: only an open purchase order can be billed".into());
    }
    let lines = purchase_order_lines(pool, id).await?;
    // Its discounts and charges come over too, so the bill totals what the order did.
    let form_lines = crate::purchase_lines::inputs_of(&crate::purchase_lines::lines_of(pool, crate::purchase_lines::PurchaseDoc::PurchaseOrder, id).await?);
    Ok(BillInput {
        terms_id,
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
                customer_id: l.customer_id,
                billable: None,
            })
            .collect(),
        form_lines,
    })
}

/// Turns an open purchase order into a posted bill with the same lines; returns the bill id. The
/// screen's command builds, checks and posts in steps instead.
#[cfg(test)]
pub async fn bill_purchase_order(pool: &SqlitePool, id: i64, bill_date: NaiveDate, due_date: NaiveDate, bill_number: Option<String>) -> Result<i64, String> {
    let bill = purchase_order_bill(pool, id, bill_date, due_date, bill_number).await?;
    post_purchase_order_bill(pool, id, &bill).await
}

/// Posts `bill` (from [`purchase_order_bill`]) and marks the order billed, if it is still open
/// with nothing received.
pub async fn post_purchase_order_bill(pool: &SqlitePool, id: i64, bill: &BillInput) -> Result<i64, String> {
    run(pool, async |tx| {
        let head: Option<(String, i64)> = sqlx::query_as("SELECT status, po_number FROM purchase_orders WHERE id = ?1").bind(id).fetch_optional(&mut **tx).await.map_err(db_err)?;
        let (status, number) = head.ok_or("PO_NOT_FOUND: that purchase order does not exist")?;
        if status != "OPEN" {
            return Err("PO_NOT_OPEN: only an open purchase order can be billed".into());
        }
        if crate::item_receipts::order_has_receipts(tx, id).await? {
            return Err(format!("PO_HAS_RECEIPTS: items on purchase order #{number} have been received; enter the bill from Enter Bill for Received Items"));
        }
        let bill_id = crate::expenses::write_bill(tx, bill).await?;
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

pub(crate) fn media_type(name: &str) -> &'static str {
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

/// How many files each transaction has attached, by journal entry, for the registers' paperclip marks.
pub async fn attachment_counts(pool: &SqlitePool) -> Result<std::collections::HashMap<i64, i64>, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as("SELECT journal_entry_id, COUNT(*) FROM attachments WHERE deleted_at IS NULL GROUP BY journal_entry_id")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

pub async fn list_attachments(pool: &SqlitePool, journal_entry_id: i64) -> Result<Vec<AttachmentView>, String> {
    sqlx::query_as::<_, AttachmentView>(
        "SELECT id, journal_entry_id, file_name, media_type, size_bytes, created_at FROM attachments WHERE journal_entry_id = ?1 AND deleted_at IS NULL ORDER BY id",
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

/// Deletes an attachment reversibly: it is hidden and kept, and the attachments library can restore
/// it or remove it for good (`attachments_library`).
pub async fn delete_attachment(pool: &SqlitePool, id: i64) -> Result<(), String> {
    crate::attachments_library::delete(pool, crate::attachments_library::FileRef { source: crate::attachments_library::Source::Entry, id }).await
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
