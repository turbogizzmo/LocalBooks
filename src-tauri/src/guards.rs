//! Data-entry guards: the company preferences that warn before a transaction is saved.
//!
//! A guard never blocks. It returns a `NEEDS_CONFIRMATION` error carrying typed warnings, and the
//! form answers by saving again with the warnings' codes in `confirmed` ("Save anyway"). A warning
//! the person has not seen is never skipped: a code confirmed for one save does not cover a
//! different warning that appears on the retry.
//!
//! The error text is `NEEDS_CONFIRMATION: ` followed by JSON, `{"warnings":[{"code","message"}]}`.
//! `src/lib/confirmable.js` reads it, asks, and retries.
//!
//! Hard rules stay where they are: debits equal credits and every line has an account (the
//! `account_id` of a line is required by its type and by the schema), and invoice, estimate and
//! purchase-order numbers are assigned by LocalBooks and unique in the schema, so they cannot repeat.

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::preferences;

pub const WARN_DUPLICATE_CHECKS: &str = "warn_duplicate_check_numbers";
pub const WARN_DUPLICATE_BILLS: &str = "warn_duplicate_bill_numbers";
pub const WARN_PAST_DAYS: &str = "warn_date_past_days";
pub const WARN_FUTURE_DAYS: &str = "warn_date_future_days";
pub const WARN_RETAINED_EARNINGS: &str = "warn_retained_earnings";
pub const WARN_OVERDUE_CUSTOMER: &str = "warn_customer_overdue";
pub const WARN_VENDOR_OPEN_BILLS: &str = "warn_vendor_open_bills";
pub const WARN_CUSTOMER_OPEN_INVOICES: &str = "warn_customer_open_invoices";
/// About ten years; 0 turns the warning off.
pub const MAX_WARN_DAYS: u32 = 3650;

pub const NEEDS_CONFIRMATION: &str = "NEEDS_CONFIRMATION";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// Stable: the form sends it back to confirm this warning.
    pub code: String,
    /// For the person, in full sentences.
    pub message: String,
}

impl Warning {
    fn new(code: &str, message: String) -> Self {
        Self { code: code.to_string(), message }
    }
}

/// `Ok` when every warning was confirmed; otherwise the `NEEDS_CONFIRMATION` error listing the
/// warnings still to answer.
pub fn confirm(warnings: Vec<Warning>, confirmed: Option<&[String]>) -> Result<(), String> {
    let confirmed = confirmed.unwrap_or_default();
    let open: Vec<Warning> = warnings.into_iter().filter(|w| !confirmed.contains(&w.code)).collect();
    if open.is_empty() {
        return Ok(());
    }
    let body = serde_json::json!({ "warnings": open });
    Err(format!("{NEEDS_CONFIRMATION}: {body}"))
}

/// The warnings in a `NEEDS_CONFIRMATION` error, or `None` for any other error.
#[cfg(test)]
pub fn warnings_in(err: &str) -> Option<Vec<Warning>> {
    let json = err.strip_prefix(NEEDS_CONFIRMATION)?.strip_prefix(": ")?;
    #[derive(Deserialize)]
    struct Body {
        warnings: Vec<Warning>,
    }
    serde_json::from_str::<Body>(json).ok().map(|b| b.warnings)
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

async fn days_pref(pool: &SqlitePool, key: &str) -> Result<i64, String> {
    Ok(preferences::get(pool, key).await?.parse::<i64>().unwrap_or(0))
}

/// A date more than the set number of days before or after `today`.
pub async fn date_warning(pool: &SqlitePool, date: NaiveDate, today: NaiveDate) -> Result<Option<Warning>, String> {
    let days = (date - today).num_days();
    let past = days_pref(pool, WARN_PAST_DAYS).await?;
    let future = days_pref(pool, WARN_FUTURE_DAYS).await?;
    let shown = date.format("%m/%d/%Y");
    if past > 0 && -days > past {
        return Ok(Some(Warning::new(
            "DATE_FAR_PAST",
            format!("The date {shown} is {} days in the past (more than the {past} days set in Preferences › Accounting).", -days),
        )));
    }
    if future > 0 && days > future {
        return Ok(Some(Warning::new(
            "DATE_FAR_FUTURE",
            format!("The date {shown} is {days} days in the future (more than the {future} days set in Preferences › Accounting)."),
        )));
    }
    Ok(None)
}

/// A check number already used on another live check from the same account.
pub async fn duplicate_check_warning(pool: &SqlitePool, payment_account_id: i64, check_number: Option<&str>, except_expense_id: Option<i64>) -> Result<Option<Warning>, String> {
    let Some(number) = check_number.map(str::trim).filter(|n| !n.is_empty()) else { return Ok(None) };
    if !preferences::flag(pool, WARN_DUPLICATE_CHECKS).await? {
        return Ok(None);
    }
    let used: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT x.expense_date, COALESCE(v.name, o.name) FROM expenses x
         JOIN journal_entries e ON e.id = x.journal_entry_id
         LEFT JOIN vendors v ON v.id = x.vendor_id LEFT JOIN other_names o ON o.id = x.other_name_id
         WHERE x.kind = 'CHECK' AND x.payment_account_id = ?1 AND trim(x.check_number) = ?2
           AND e.is_voided = 0 AND x.id IS NOT ?3
         ORDER BY x.expense_date LIMIT 1",
    )
    .bind(payment_account_id)
    .bind(number)
    .bind(except_expense_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(used.map(|(date, payee)| {
        let to = payee.map(|p| format!(" to {p}")).unwrap_or_default();
        Warning::new("DUPLICATE_CHECK_NUMBER", format!("Check number {number} is already used on this account (a check{to} dated {date})."))
    }))
}

/// A bill number already used on another live bill from the same vendor.
pub async fn duplicate_bill_warning(pool: &SqlitePool, vendor_name: &str, bill_number: Option<&str>, except_bill_id: Option<i64>) -> Result<Option<Warning>, String> {
    let Some(number) = bill_number.map(str::trim).filter(|n| !n.is_empty()) else { return Ok(None) };
    if !preferences::flag(pool, WARN_DUPLICATE_BILLS).await? {
        return Ok(None);
    }
    let used: Option<String> = sqlx::query_scalar(
        "SELECT b.bill_date FROM bills b
         JOIN vendors v ON v.id = b.vendor_id
         JOIN journal_entries e ON e.id = b.journal_entry_id
         WHERE v.name = ?1 COLLATE NOCASE AND trim(b.bill_number) = ?2 COLLATE NOCASE
           AND e.is_voided = 0 AND b.id IS NOT ?3
         ORDER BY b.bill_date LIMIT 1",
    )
    .bind(vendor_name.trim())
    .bind(number)
    .bind(except_bill_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(used.map(|date| {
        Warning::new("DUPLICATE_BILL_NUMBER", format!("{} already has a bill numbered {number} (dated {date}).", vendor_name.trim()))
    }))
}

/// A line posting straight to Retained Earnings, which normally only the year-end close changes.
pub async fn retained_earnings_warning(pool: &SqlitePool, account_ids: &[i64]) -> Result<Option<Warning>, String> {
    if account_ids.is_empty() || !preferences::flag(pool, WARN_RETAINED_EARNINGS).await? {
        return Ok(None);
    }
    let retained: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, name FROM chart_of_accounts
         WHERE is_system = 1 AND account_type = 'EQUITY' AND (name = 'Retained Earnings' OR account_number IN ('3900', '32000'))
         ORDER BY name = 'Retained Earnings' DESC, id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(retained.filter(|(id, _)| account_ids.contains(id)).map(|(_, name)| {
        Warning::new(
            "POSTS_TO_RETAINED_EARNINGS",
            format!("This transaction posts to {name}. LocalBooks moves each year's profit there on its own; posting to it directly is usually a correction of a prior year."),
        )
    }))
}

/// Units a sale would take out of stock, against what is on hand (Preferences › Items & Inventory
/// › "when stock would go below zero": off, warn or stop). `sold` is `(product, quantity × 10,000)`
/// per line; lines of untracked products are ignored. `returned_by` is the journal entry of the
/// document being replaced, whose units come back to stock when the edit voids it.
///
/// `warn` returns one warning listing each product; `stop` refuses the save with
/// `INV_NOT_ENOUGH_STOCK`, which Save anyway cannot answer.
pub async fn stock_warning(pool: &SqlitePool, sold: &[(i64, i64)], returned_by: Option<i64>) -> Result<Option<Warning>, String> {
    let mode = preferences::get(pool, preferences::tax_time::NEGATIVE_STOCK).await?;
    if mode == "off" || sold.is_empty() {
        return Ok(None);
    }
    let mut wanted: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    for (product, qty) in sold {
        let total = wanted.entry(*product).or_default();
        *total = total.checked_add(*qty).ok_or("INV_QTY_TOO_LARGE: the quantity is out of range")?;
    }
    let mut short = Vec::new();
    for (product, qty) in wanted {
        let item: Option<(i64, String, i64)> = sqlx::query_as(
            "SELECT i.id, p.name, i.qty_on_hand_e4 FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE i.product_id = ?1",
        )
        .bind(product)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        let Some((item_id, name, on_hand)) = item else { continue };
        let back: i64 = match returned_by {
            Some(entry) => sqlx::query_scalar("SELECT COALESCE(-SUM(qty_change_e4), 0) FROM stock_ledger_entries WHERE item_id = ?1 AND journal_entry_id = ?2 AND kind = 'SALE'")
                .bind(item_id)
                .bind(entry)
                .fetch_one(pool)
                .await
                .map_err(db_err)?,
            None => 0,
        };
        let available = on_hand.saturating_add(back);
        if qty > 0 && available.saturating_sub(qty) < 0 {
            short.push(format!("{name} ({} on hand, {} on this sale)", crate::forms::format_quantity(available), crate::forms::format_quantity(qty)));
        }
    }
    if short.is_empty() {
        return Ok(None);
    }
    let list = short.join("; ");
    if mode == "stop" {
        return Err(format!("INV_NOT_ENOUGH_STOCK: not enough on hand to sell: {list}. Record the purchase or an adjustment first (Preferences › Items & Inventory stops sales below zero)."));
    }
    Ok(Some(Warning::new("NEGATIVE_STOCK", format!("This sale takes stock below zero: {list}. It is costed at the last purchase cost until the stock arrives."))))
}

/// Selling to a customer (or one of its jobs, or the customer a job belongs to) that has invoices
/// past their due date as of `today`. Only an existing customer can have any.
pub async fn overdue_customer_warning(pool: &SqlitePool, customer_name: &str, today: NaiveDate) -> Result<Option<Warning>, String> {
    let name = customer_name.trim();
    if name.is_empty() || !preferences::flag(pool, WARN_OVERDUE_CUSTOMER).await? {
        return Ok(None);
    }
    let overdue: (i64, Option<i64>, Option<String>) = sqlx::query_as(
        "WITH me AS (SELECT id, parent_id FROM customers WHERE name = ?1 COLLATE NOCASE),
              family AS (SELECT id FROM me UNION SELECT parent_id FROM me WHERE parent_id IS NOT NULL
                         UNION SELECT c.id FROM customers c JOIN me ON c.parent_id = me.id)
         SELECT COUNT(*), SUM(b.home_open_cents), MIN(i.due_date)
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
         WHERE i.customer_id IN (SELECT id FROM family) AND i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL')
           AND b.home_open_cents > 0 AND i.due_date < ?2",
    )
    .bind(name)
    .bind(today)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let (count, open, oldest) = overdue;
    if count == 0 {
        return Ok(None);
    }
    let oldest = oldest.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()).map(|d| d.format("%m/%d/%Y").to_string()).unwrap_or_default();
    let invoices = if count == 1 { "1 overdue invoice".to_string() } else { format!("{count} overdue invoices") };
    Ok(Some(Warning::new(
        "CUSTOMER_OVERDUE",
        format!(
            "{name} has {invoices} with {} still open (the oldest was due {oldest}).",
            crate::ledger::format_cents(open.unwrap_or_default())
        ),
    )))
}

fn count_text(count: i64, one: &str, many: &str) -> String {
    if count == 1 { format!("1 {one}") } else { format!("{count} {many}") }
}

/// Paying a vendor directly (a check or expense) while the vendor has unpaid bills: the payment
/// won't mark them paid, so the bills stay open and the money is often counted twice.
pub async fn vendor_open_bills_warning(pool: &SqlitePool, vendor_name: &str) -> Result<Option<Warning>, String> {
    let name = vendor_name.trim();
    if name.is_empty() || !preferences::flag(pool, WARN_VENDOR_OPEN_BILLS).await? {
        return Ok(None);
    }
    let (count, open): (i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), SUM(b.total_cents - b.amount_paid_cents)
         FROM bills b JOIN vendors v ON v.id = b.vendor_id
         WHERE v.name = ?1 COLLATE NOCASE AND b.is_voided = 0 AND b.status IN ('UNPAID', 'PARTIAL') AND b.total_cents > b.amount_paid_cents",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if count == 0 {
        return Ok(None);
    }
    Ok(Some(Warning::new(
        "VENDOR_OPEN_BILLS",
        format!(
            "{name} has {} with {} still to pay. A check or expense here doesn't pay them; to pay a bill, use Pay Bills.",
            count_text(count, "open bill", "open bills"),
            crate::ledger::format_cents(open.unwrap_or_default())
        ),
    )))
}

/// Taking money in a sales receipt from a customer (or one of its jobs) that has open invoices: the
/// receipt won't pay them, so they stay open.
pub async fn customer_open_invoices_warning(pool: &SqlitePool, customer_name: &str) -> Result<Option<Warning>, String> {
    let name = customer_name.trim();
    if name.is_empty() || !preferences::flag(pool, WARN_CUSTOMER_OPEN_INVOICES).await? {
        return Ok(None);
    }
    let (count, open): (i64, Option<i64>) = sqlx::query_as(
        "WITH me AS (SELECT id, parent_id FROM customers WHERE name = ?1 COLLATE NOCASE),
              family AS (SELECT id FROM me UNION SELECT parent_id FROM me WHERE parent_id IS NOT NULL
                         UNION SELECT c.id FROM customers c JOIN me ON c.parent_id = me.id)
         SELECT COUNT(*), SUM(b.home_open_cents)
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
         WHERE i.customer_id IN (SELECT id FROM family) AND i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL') AND b.home_open_cents > 0",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if count == 0 {
        return Ok(None);
    }
    Ok(Some(Warning::new(
        "CUSTOMER_OPEN_INVOICES",
        format!(
            "{name} has {} with {} still owed. A sales receipt doesn't pay them; to record a payment on an invoice, use Receive Payments.",
            count_text(count, "open invoice", "open invoices"),
            crate::ledger::format_cents(open.unwrap_or_default())
        ),
    )))
}

/// What a save is about to post, for the guards to look at.
#[derive(Debug, Default)]
pub struct Posting<'a> {
    pub date: Option<NaiveDate>,
    pub account_ids: Vec<i64>,
    /// `(payment account, check number, expense being replaced)`.
    pub check: Option<(i64, Option<&'a str>, Option<i64>)>,
    /// `(vendor name, bill number, bill being replaced)`.
    pub bill: Option<(&'a str, Option<&'a str>, Option<i64>)>,
    /// `(product, quantity × 10,000)` per sold line, and the entry of a sale being replaced.
    pub stock: Vec<(i64, i64)>,
    pub stock_returned_by: Option<i64>,
    /// The customer a new sale is to (an invoice or sales receipt; not an edit).
    pub customer: Option<&'a str>,
    /// The vendor a new check or expense pays directly (not an edit, not a card credit).
    pub paid_vendor: Option<&'a str>,
    /// The customer a new sales receipt takes money from.
    pub receipt_customer: Option<&'a str>,
}

/// Every warning for `posting`, as of `today`.
pub async fn warnings_on(pool: &SqlitePool, posting: &Posting<'_>, today: NaiveDate) -> Result<Vec<Warning>, String> {
    let mut out = Vec::new();
    if let Some(date) = posting.date {
        out.extend(date_warning(pool, date, today).await?);
    }
    if let Some((account, number, except)) = posting.check {
        out.extend(duplicate_check_warning(pool, account, number, except).await?);
    }
    if let Some((vendor, number, except)) = posting.bill {
        out.extend(duplicate_bill_warning(pool, vendor, number, except).await?);
    }
    out.extend(retained_earnings_warning(pool, &posting.account_ids).await?);
    out.extend(stock_warning(pool, &posting.stock, posting.stock_returned_by).await?);
    if let Some(customer) = posting.customer {
        out.extend(overdue_customer_warning(pool, customer, today).await?);
    }
    if let Some(vendor) = posting.paid_vendor {
        out.extend(vendor_open_bills_warning(pool, vendor).await?);
    }
    if let Some(customer) = posting.receipt_customer {
        out.extend(customer_open_invoices_warning(pool, customer).await?);
    }
    Ok(out)
}

/// Runs every guard on `posting`: `Ok` to save, or `NEEDS_CONFIRMATION` with what to ask.
pub async fn check(pool: &SqlitePool, posting: Posting<'_>, confirmed: Option<&[String]>) -> Result<(), String> {
    confirm(warnings_on(pool, &posting, today()).await?, confirmed)
}

pub fn journal(entry: &crate::ledger::JournalEntry) -> Posting<'static> {
    Posting { date: Some(entry.date), account_ids: entry.lines.iter().map(|l| l.account_id).collect(), ..Default::default() }
}

pub fn expense(input: &crate::expenses::ExpenseInput, replacing: Option<i64>) -> Posting<'_> {
    Posting {
        date: Some(input.expense_date),
        account_ids: input.lines.iter().map(|l| l.account_id).collect(),
        check: (input.kind == "CHECK").then_some((input.payment_account_id, input.check_number.as_deref(), replacing)),
        paid_vendor: if replacing.is_none() && input.kind != "CARD_CREDIT" { input.payee_name.as_deref() } else { None },
        ..Default::default()
    }
}

pub fn bill(input: &crate::expenses::BillInput, replacing: Option<i64>) -> Posting<'_> {
    Posting {
        date: Some(input.bill_date),
        account_ids: input.lines.iter().map(|l| l.account_id).collect(),
        bill: Some((input.vendor_name.as_str(), input.bill_number.as_deref(), replacing)),
        ..Default::default()
    }
}

/// Units a sale takes out of stock, for the stock guard: `(product, quantity × 10,000)` per line;
/// `replacing` is the journal entry of the sale an edit replaces.
pub fn stock_sale(date: NaiveDate, lines: impl Iterator<Item = (Option<i64>, i64)>, replacing: Option<i64>) -> Posting<'static> {
    Posting { date: Some(date), stock: lines.filter_map(|(p, q)| p.map(|p| (p, q))).collect(), stock_returned_by: replacing, ..Default::default() }
}

/// An invoice: its date, the stock its lines take out, and (for a new one, not an edit) its
/// customer, for the overdue-customer warning. `replacing` is the journal entry an edit replaces.
pub fn invoice(input: &crate::invoices::InvoiceInput, replacing: Option<i64>) -> Posting<'_> {
    Posting {
        customer: replacing.is_none().then_some(input.customer_name.as_str()),
        ..stock_sale(input.issue_date, input.lines.iter().map(|l| (l.product_service_id, l.quantity_e4)), replacing)
    }
}

/// A new sales receipt: its date, customer and the stock its lines take out.
pub fn sales_receipt(input: &crate::sales::SalesReceiptInput) -> Posting<'_> {
    Posting {
        customer: input.customer_name.as_deref(),
        receipt_customer: input.customer_name.as_deref(),
        ..stock_sale(input.receipt_date, input.lines.iter().map(|l| (l.product_service_id, l.quantity_e4)), None)
    }
}

/// Estimates, purchase orders and edits of sales documents: only the date is guarded.
pub fn dated(date: NaiveDate) -> Posting<'static> {
    Posting { date: Some(date), ..Default::default() }
}

/// A new invoice or sales receipt: its date, and whether the customer has overdue invoices.
#[cfg(test)]
pub fn sale(date: NaiveDate, customer_name: &str) -> Posting<'_> {
    Posting { date: Some(date), customer: Some(customer_name), ..Default::default() }
}

#[cfg(test)]
mod tests;
