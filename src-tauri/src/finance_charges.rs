//! Finance charges: interest on overdue invoice balances, assessed as invoices of their own.
//!
//! The preferences (Settings › Preferences Console › Finance charges) set an annual rate, a minimum
//! charge, grace days, the income account credited, and whether earlier finance charges themselves
//! earn finance charges.
//!
//! An invoice is overdue on the assessment date ("as of") when its due date is more than the grace
//! days before it and it still has a balance as of that date (its total less the payments and
//! credits dated on or before it). Interest runs from the due date, or from the end of the period the
//! last finance charge on that invoice covered, to the as-of date. Home-currency invoices only.
//!
//! Rounding, all in integer arithmetic: for each customer,
//!
//! ```text
//! charge = round_half_up( Σ balance_cents × days × rate_milli_percent / (365 × 100,000) )
//! ```
//!
//! where `rate_milli_percent` is the annual rate in thousandths of a percent (18% → 18,000) and every
//! year counts as 365 days. The sum is taken before rounding, so a customer's charge is rounded once.
//! A charge below the minimum is raised to the minimum.
//!
//! Assessing writes one invoice per customer, dated as of the assessment, debiting A/R and crediting
//! the chosen income account, and records the period it covered for each overdue invoice.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{InvoiceInput, InvoiceLineInput, InvoiceSummary};
use crate::ledger::{db_err, format_cents, rollback, MAX_LINE_CENTS, QUANTITY_SCALE};

/// Every year counts as 365 days; rates are in thousandths of a percent.
const DAYS_PER_YEAR: i128 = 365;
const MILLI_PERCENT: i128 = 100_000;
/// The largest annual rate accepted: 100%.
pub const MAX_RATE_MILLI_PERCENT: i64 = 100_000;
/// The largest minimum charge accepted: 10,000.00.
pub const MAX_MINIMUM_CENTS: i64 = 1_000_000;
pub const MAX_GRACE_DAYS: i64 = 365;

/// Parses an annual percentage such as `18`, `18.5` or `1.375%` to thousandths of a percent.
pub fn parse_rate(text: &str) -> Result<i64, String> {
    let bad = || "PREF_BAD_FC_RATE: enter an annual rate from 0 to 100 percent, with up to 3 decimals".to_string();
    let t = text.trim().trim_end_matches('%').trim();
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if (whole.is_empty() && frac.is_empty())
        || whole.len() > 3
        || frac.len() > 3
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| bad())? };
    let frac: i64 = format!("{frac:0<3}").parse().map_err(|_| bad())?;
    let rate = whole * 1000 + frac;
    if rate > MAX_RATE_MILLI_PERCENT {
        return Err(bad());
    }
    Ok(rate)
}

/// 18_000 → "18", 18_500 → "18.5", 1_375 → "1.375".
pub fn format_rate(milli_percent: i64) -> String {
    let frac = format!("{:03}", milli_percent % 1000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { (milli_percent / 1000).to_string() } else { format!("{}.{frac}", milli_percent / 1000) }
}

/// Parses the minimum charge (`2`, `2.50`, `$2.50`) to cents.
pub fn parse_minimum(text: &str) -> Result<i64, String> {
    let bad = || format!("PREF_BAD_FC_MINIMUM: enter a minimum charge from 0.00 to {}", format_cents(MAX_MINIMUM_CENTS));
    let cents = crate::bank::parse_amount_cents(text).map_err(|_| bad())?;
    if !(0..=MAX_MINIMUM_CENTS).contains(&cents) {
        return Err(bad());
    }
    Ok(cents)
}

/// The finance charge preferences, parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub annual_rate_milli_percent: i64,
    pub minimum_cents: i64,
    pub grace_days: i64,
    pub income_account_id: Option<i64>,
    pub charge_finance_charges: bool,
}

pub async fn settings(pool: &SqlitePool) -> Result<Settings, String> {
    use crate::preferences::get;
    let account = get(pool, "finance_charge_account_id").await?;
    Ok(Settings {
        annual_rate_milli_percent: parse_rate(&get(pool, "finance_charge_annual_rate").await?)?,
        minimum_cents: parse_minimum(&get(pool, "finance_charge_minimum").await?)?,
        grace_days: get(pool, "finance_charge_grace_days").await?.parse().map_err(|_| "PREF_BAD_FC_GRACE: grace days are not a number")?,
        income_account_id: account.parse().ok(),
        charge_finance_charges: get(pool, "finance_charge_on_finance_charges").await? == "true",
    })
}

/// One overdue invoice and the period it is charged for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueInvoice {
    pub invoice_id: i64,
    pub invoice_number: i64,
    pub due_date: NaiveDate,
    /// Open balance as of the assessment date.
    pub balance_cents: i64,
    /// Interest runs from this date (the due date, or the end of the last charge on this invoice).
    pub from_date: NaiveDate,
    pub days: i64,
    pub is_finance_charge: bool,
}

/// A customer with overdue invoices and the charge worked out for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerCharge {
    pub customer_id: Option<i64>,
    pub customer_name: String,
    pub overdue_balance_cents: i64,
    /// Interest before the minimum is applied.
    pub interest_cents: i64,
    /// The charge to assess: the interest, or the minimum if that is more.
    pub charge_cents: i64,
    pub last_finance_charge_date: Option<NaiveDate>,
    pub invoices: Vec<OverdueInvoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub as_of: NaiveDate,
    pub settings: Settings,
    pub customers: Vec<CustomerCharge>,
}

/// `round_half_up(Σ balance × days × rate / (365 × 100,000))` over one customer's overdue invoices.
pub fn interest_cents(invoices: &[OverdueInvoice], rate_milli_percent: i64) -> Result<i64, String> {
    let numerator: i128 = invoices.iter().map(|i| i.balance_cents as i128 * i.days as i128 * rate_milli_percent as i128).sum();
    let denominator = DAYS_PER_YEAR * MILLI_PERCENT;
    let rounded = (numerator + denominator / 2) / denominator;
    i64::try_from(rounded).ok().filter(|c| *c <= MAX_LINE_CENTS).ok_or_else(|| "FC_TOO_LARGE: the finance charge is out of range".to_string())
}

/// The charge for a customer's overdue invoices: their interest, raised to the minimum.
pub fn charge_cents(invoices: &[OverdueInvoice], settings: &Settings) -> Result<(i64, i64), String> {
    let interest = interest_cents(invoices, settings.annual_rate_milli_percent)?;
    Ok((interest, interest.max(settings.minimum_cents)))
}

type OpenRow = (i64, i64, Option<i64>, String, NaiveDate, i64, bool, Option<NaiveDate>);

/// Customers with overdue invoices as of `as_of`, and the charge for each.
async fn compute(conn: &mut sqlx::SqliteConnection, as_of: NaiveDate, settings: &Settings) -> Result<Vec<CustomerCharge>, String> {
    // Home-currency invoices issued by the as-of date, with their balance as of that date and the end
    // of the last period a live finance charge covered.
    let rows: Vec<OpenRow> = sqlx::query_as(
        "SELECT i.id, i.invoice_number, i.customer_id, COALESCE(c.name, i.customer_name), i.due_date,
                i.total_cents
                  - COALESCE((SELECT SUM(p.amount_cents) FROM invoice_payments p
                              WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
                  - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a
                              WHERE a.invoice_id = i.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0) AS balance,
                EXISTS (SELECT 1 FROM finance_charge_invoices f WHERE f.invoice_id = i.id) AS is_fc,
                (SELECT MAX(b.to_date) FROM finance_charge_basis b JOIN invoices fi ON fi.id = b.finance_charge_invoice_id
                 WHERE b.source_invoice_id = i.id AND fi.is_voided = 0) AS charged_to
         FROM invoices i LEFT JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 AND i.status <> 'DRAFT' AND i.currency_code IS NULL
           AND i.issue_date <= ?1 AND julianday(?1) - julianday(i.due_date) > ?2
         ORDER BY COALESCE(c.name, i.customer_name) COLLATE NOCASE, i.due_date, i.invoice_number",
    )
    .bind(as_of)
    .bind(settings.grace_days)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;

    let mut customers: Vec<CustomerCharge> = Vec::new();
    for (invoice_id, invoice_number, customer_id, customer_name, due_date, balance, is_fc, charged_to) in rows {
        if balance <= 0 || (is_fc && !settings.charge_finance_charges) {
            continue;
        }
        let from_date = charged_to.map_or(due_date, |c| c.max(due_date));
        let days = (as_of - from_date).num_days();
        if days <= 0 {
            continue;
        }
        let invoice = OverdueInvoice { invoice_id, invoice_number, due_date, balance_cents: balance, from_date, days, is_finance_charge: is_fc };
        match customers.last_mut().filter(|c| c.customer_name == customer_name) {
            Some(c) => {
                c.overdue_balance_cents += balance;
                c.invoices.push(invoice);
            }
            None => customers.push(CustomerCharge {
                customer_id,
                customer_name,
                overdue_balance_cents: balance,
                interest_cents: 0,
                charge_cents: 0,
                last_finance_charge_date: None,
                invoices: vec![invoice],
            }),
        }
    }
    let last: BTreeMap<String, NaiveDate> = sqlx::query_as::<_, (String, NaiveDate)>(
        "SELECT COALESCE(c.name, i.customer_name), MAX(i.issue_date) FROM finance_charge_invoices f
         JOIN invoices i ON i.id = f.invoice_id LEFT JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 GROUP BY 1",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?
    .into_iter()
    .collect();
    for c in &mut customers {
        (c.interest_cents, c.charge_cents) = charge_cents(&c.invoices, settings)?;
        c.last_finance_charge_date = last.get(&c.customer_name).copied();
    }
    Ok(customers)
}

/// The Assess Finance Charges list: customers with overdue invoices as of `as_of` and their charges.
pub async fn preview(pool: &SqlitePool, as_of: NaiveDate) -> Result<Preview, String> {
    let settings = settings(pool).await?;
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let customers = compute(&mut conn, as_of, &settings).await?;
    Ok(Preview { as_of, settings, customers })
}

/// One customer ticked on the Assess Finance Charges screen, with the charge (as shown or edited).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessCharge {
    pub customer_name: String,
    pub charge_cents: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessInput {
    pub as_of: NaiveDate,
    pub charges: Vec<AssessCharge>,
}

/// Writes a finance-charge invoice for each customer in `input`, all or none. The overdue invoices
/// are worked out again inside the write, so the periods recorded match the books at that moment.
pub async fn assess(pool: &SqlitePool, input: AssessInput) -> Result<Vec<InvoiceSummary>, String> {
    if input.charges.is_empty() {
        return Err("FC_NOTHING_SELECTED: tick at least one customer to assess".into());
    }
    let settings = settings(pool).await?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let ids = match write_charges(&mut tx, &input, &settings).await {
        Ok(ids) => ids,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    let mut invoices = Vec::with_capacity(ids.len());
    for id in ids {
        invoices.push(crate::invoices::get_summary(pool, id).await?);
    }
    Ok(invoices)
}

async fn write_charges(tx: &mut Transaction<'_, Sqlite>, input: &AssessInput, settings: &Settings) -> Result<Vec<i64>, String> {
    let account = settings
        .income_account_id
        .ok_or("FC_NO_ACCOUNT: choose the income account for finance charges in Settings › Preferences Console › Finance charges")?;
    let customers = compute(tx, input.as_of, settings).await?;
    let mut seen: Vec<&str> = Vec::new();
    let mut ids = Vec::with_capacity(input.charges.len());
    for charge in &input.charges {
        let name = charge.customer_name.as_str();
        if seen.contains(&name) {
            return Err(format!("FC_DUPLICATE: {name} is listed twice"));
        }
        seen.push(name);
        let customer = customers
            .iter()
            .find(|c| c.customer_name == name)
            .ok_or_else(|| format!("FC_NOT_OVERDUE: {name} has no overdue balance to charge as of {}", input.as_of))?;
        if !(1..=MAX_LINE_CENTS).contains(&charge.charge_cents) {
            return Err(format!("FC_BAD_AMOUNT: the finance charge for {name} must be more than zero"));
        }
        let invoice = InvoiceInput {
            customer_name: name.to_string(),
            issue_date: input.as_of,
            due_date: input.as_of,
            lines: vec![InvoiceLineInput {
                description: format!("Finance charge on overdue balance of {} through {}", format_cents(customer.overdue_balance_cents), input.as_of.format("%m/%d/%Y")),
                quantity_e4: QUANTITY_SCALE,
                rate_cents: charge.charge_cents,
                income_account_id: account,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        };
        let invoice_id = crate::invoices::write_invoice(tx, &invoice).await.map_err(|e| e.replacen("INV_", "FC_", 1))?;
        sqlx::query("INSERT INTO finance_charge_invoices (invoice_id, assessed_as_of, computed_cents) VALUES (?1, ?2, ?3)")
            .bind(invoice_id)
            .bind(input.as_of)
            .bind(customer.charge_cents)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        for overdue in &customer.invoices {
            sqlx::query(
                "INSERT INTO finance_charge_basis (finance_charge_invoice_id, source_invoice_id, balance_cents, from_date, to_date)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(invoice_id)
            .bind(overdue.invoice_id)
            .bind(overdue.balance_cents)
            .bind(overdue.from_date)
            .bind(input.as_of)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        }
        ids.push(invoice_id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
