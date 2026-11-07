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
/// Warn before an invoice or sales receipt for a customer who has invoices past due.
pub const WARN_OVERDUE_CUSTOMERS: &str = "warn_overdue_customers";
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
        "SELECT x.expense_date, v.name FROM expenses x
         JOIN journal_entries e ON e.id = x.journal_entry_id
         LEFT JOIN vendors v ON v.id = x.vendor_id
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

/// A customer with open invoices past their due date on `today` (Sales & Customers).
pub async fn overdue_customer_warning(pool: &SqlitePool, customer_name: &str, today: NaiveDate) -> Result<Option<Warning>, String> {
    let name = customer_name.trim();
    if name.is_empty() || !preferences::flag(pool, WARN_OVERDUE_CUSTOMERS).await? {
        return Ok(None);
    }
    let (count, open_cents, oldest): (i64, i64, Option<NaiveDate>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(b.home_open_cents), 0), MIN(i.due_date) FROM invoices i
         JOIN v_invoice_balances b ON b.invoice_id = i.id
         LEFT JOIN customers c ON c.id = i.customer_id
         WHERE COALESCE(c.name, i.customer_name) = ?1 COLLATE NOCASE
           AND i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND b.home_open_cents > 0 AND i.due_date < ?2",
    )
    .bind(name)
    .bind(today)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let Some(oldest) = oldest.filter(|_| count > 0) else { return Ok(None) };
    let invoices = if count == 1 { "1 invoice".to_string() } else { format!("{count} invoices") };
    Ok(Some(Warning::new(
        "CUSTOMER_OVERDUE",
        format!(
            "{name} has {invoices} past due, {} in all, the oldest due {}.",
            crate::ledger::format_cents(open_cents),
            oldest.format("%m/%d/%Y")
        ),
    )))
}

/// Company preference: what happens when a sale would take an item's stock below zero: `allow`
/// (the default; the oversold units are costed later), `warn` (ask, Save anyway) or `stop`.
pub const NEGATIVE_STOCK: &str = "negative_stock";

/// Tracked items whose stock on hand is less than what a sale takes. `sold` is
/// `(product, quantity × 10,000)` per line. With the preference at `stop` this is an error that
/// Save anyway cannot pass.
pub async fn negative_stock_warning(pool: &SqlitePool, sold: &[(i64, i64)]) -> Result<Option<Warning>, String> {
    if sold.is_empty() {
        return Ok(None);
    }
    let mode = preferences::get(pool, NEGATIVE_STOCK).await?;
    if mode == "allow" {
        return Ok(None);
    }
    let mut wanted: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    for (product, qty) in sold {
        let total = wanted.entry(*product).or_default();
        *total = total.checked_add(*qty).ok_or("STOCK_OVERFLOW: quantity out of range")?;
    }
    let mut short: Vec<String> = Vec::new();
    for (product, qty) in wanted {
        let item: Option<(String, i64)> =
            sqlx::query_as("SELECT p.name, i.qty_on_hand_e4 FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE i.product_id = ?1")
                .bind(product)
                .fetch_optional(pool)
                .await
                .map_err(db_err)?;
        if let Some((name, on_hand)) = item.filter(|(_, on_hand)| *on_hand < qty) {
            short.push(format!("{name} (selling {}, {} on hand)", crate::forms::format_quantity(qty), crate::forms::format_quantity(on_hand)));
        }
    }
    if short.is_empty() {
        return Ok(None);
    }
    let list = short.join(", ");
    if mode == "stop" {
        return Err(format!("STOCK_NEGATIVE: not enough in stock: {list}. Enter what you received first, or change Preferences › Items & Inventory."));
    }
    Ok(Some(Warning::new("NEGATIVE_STOCK", format!("This sale takes stock below zero: {list}."))))
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
    /// The customer a new sale is for.
    pub customer: Option<&'a str>,
    /// Tracked items a new sale takes: `(product, quantity × 10,000)`.
    pub sold: Vec<(i64, i64)>,
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
    if let Some(customer) = posting.customer {
        out.extend(overdue_customer_warning(pool, customer, today).await?);
    }
    out.extend(negative_stock_warning(pool, &posting.sold).await?);
    out.extend(retained_earnings_warning(pool, &posting.account_ids).await?);
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
        ..Default::default()
    }
}

pub fn bill(input: &crate::expenses::BillInput, replacing: Option<i64>) -> Posting<'_> {
    Posting {
        date: Some(input.bill_date),
        account_ids: input.lines.iter().map(|l| l.account_id).collect(),
        check: None,
        bill: Some((input.vendor_name.as_str(), input.bill_number.as_deref(), replacing)),
        ..Default::default()
    }
}

/// Estimates, purchase orders and changes to a sale: only the date is guarded.
pub fn dated(date: NaiveDate) -> Posting<'static> {
    Posting { date: Some(date), ..Default::default() }
}

/// A new invoice or sales receipt: its date, whether the customer is behind on paying, and the
/// stock it takes (`(product, quantity × 10,000)` per line with an item).
pub fn sale(date: NaiveDate, customer: Option<&str>, sold: impl IntoIterator<Item = (Option<i64>, i64)>) -> Posting<'_> {
    Posting {
        date: Some(date),
        customer: customer.filter(|c| !c.trim().is_empty()),
        sold: sold.into_iter().filter_map(|(product, qty)| product.map(|p| (p, qty))).collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
