//! Preferences batch 4: the company defaults for sales, payments and bills (`docs/plans/preferences.md`).
//!
//! The keys live in `preferences::DEFAULTS` like every other company preference; this module holds
//! what they change:
//!
//! - **Receive Payments:** how an amount received is spread over a customer's open invoices
//!   ([`apply_payment`]): an invoice whose balance matches exactly, otherwise the oldest first, or
//!   not at all (the person ticks invoices).
//! - **Enter Bills:** the due date of a new bill when its vendor has no terms ([`bill_due_date`]).
//! - **Pay Bills:** the vendor's open credits used on the bills chosen, oldest credit first
//!   ([`plan_vendor_credits`]), so only the rest is paid.
//! - **Estimates:** zero-amount lines left off when an estimate becomes an invoice
//!   ([`without_zero_lines`]).
//! - **Jobs:** the names shown for each job status ([`job_status_labels`]).
//!
//! The overdue-customer warning is a guard (`guards::overdue_customer_warning`).

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::preferences;

pub const PRICE_LEVELS: &str = "price_levels_enabled";
pub const ESTIMATES: &str = "estimates_enabled";
/// Sales orders on or off (Preferences › Sales & Customers); off hides them, and keeps those saved.
pub const SALES_ORDERS: &str = "sales_orders_enabled";
pub const DEFAULT_SHIP_METHOD: &str = "default_ship_method_id";
pub const DEFAULT_FOB: &str = "default_fob";
pub const DEFAULT_MARKUP: &str = "default_markup_percent";
pub const DROP_ZERO_LINES: &str = "estimate_drop_zero_lines";
pub const AUTO_APPLY: &str = "payment_auto_apply";
pub const AUTO_CALCULATE: &str = "payment_auto_calculate";
pub const UNDEPOSITED_DEFAULT: &str = "payment_undeposited_default";
pub const BILL_DUE_DAYS: &str = "bill_due_days";
pub const BILL_AUTO_CREDITS: &str = "bill_auto_apply_credits";
pub const BILL_AUTO_DISCOUNTS: &str = "bill_auto_apply_discounts";
pub const BILL_DISCOUNT_ACCOUNT: &str = "bill_discount_account_id";
pub const PAYMENT_AUTO_DISCOUNTS: &str = "payment_auto_apply_discounts";
pub const PAYMENT_DISCOUNT_ACCOUNT: &str = "payment_discount_account_id";
pub const DEFAULT_DELIVERY: &str = "default_delivery_method";
/// The longest usual F.O.B.: what a sales form's F.O.B. box takes (`sales_header`), so the default
/// it fills in can always be saved.
pub const MAX_FOB_CHARS: usize = crate::sales_header::MAX_SHORT_FIELD;
/// Most days a bill can be due after its date, as for vendor terms.
pub const MAX_BILL_DUE_DAYS: u32 = 365;
/// A markup above 1,000% is almost certainly a typing slip.
pub const MAX_MARKUP_MILLI: i64 = 1_000_000;

/// Job statuses as stored on a job (`customers.job_status`), the preference that renames each, and
/// its default name.
pub const JOB_STATUSES: &[(&str, &str, &str)] = &[
    ("PENDING", "job_status_pending", "Pending"),
    ("AWARDED", "job_status_awarded", "Awarded"),
    ("IN_PROGRESS", "job_status_in_progress", "In progress"),
    ("CLOSED", "job_status_closed", "Closed"),
    ("NOT_AWARDED", "job_status_not_awarded", "Not awarded"),
];
pub const MAX_STATUS_LABEL_CHARS: usize = 30;

/// Checks and normalizes a batch 4 value; `None` when `key` isn't one of this batch's.
pub fn validate(key: &str, value: &str) -> Option<Result<String, String>> {
    let v = value.trim();
    let flag = || match v {
        "true" | "false" => Ok(v.to_string()),
        _ => Err("PREF_BAD_FLAG: use true or false".to_string()),
    };
    Some(match key {
        PRICE_LEVELS | ESTIMATES | SALES_ORDERS | DROP_ZERO_LINES | AUTO_CALCULATE | UNDEPOSITED_DEFAULT | BILL_AUTO_CREDITS | BILL_AUTO_DISCOUNTS
        | PAYMENT_AUTO_DISCOUNTS | crate::guards::WARN_OVERDUE_CUSTOMER => flag(),
        // An expense (or asset) account, checked in `check_discount_account`; blank means none.
        BILL_DISCOUNT_ACCOUNT => match v {
            "" => Ok(String::new()),
            _ => v.parse::<i64>().map(|id| id.to_string()).map_err(|_| "PREF_BAD_DISCOUNT_ACCOUNT: choose an expense account for discounts".into()),
        },
        // An income (or expense) account, checked in `check_payment_discount_account`; blank means none.
        PAYMENT_DISCOUNT_ACCOUNT => match v {
            "" => Ok(String::new()),
            _ => v.parse::<i64>().map(|id| id.to_string()).map_err(|_| "PREF_BAD_DISCOUNT_ACCOUNT: choose an income account for discounts, such as Sales Discounts".into()),
        },
        // A shipping method, checked against the list in `check_ship_method`; blank means none.
        DEFAULT_SHIP_METHOD => match v {
            "" => Ok(String::new()),
            _ => v.parse::<i64>().map(|id| id.to_string()).map_err(|_| "PREF_BAD_SHIP_METHOD: choose a shipping method from the Ship Via list".into()),
        },
        DEFAULT_FOB if v.chars().count() > MAX_FOB_CHARS => Err(format!("PREF_TOO_LONG: F.O.B. must be at most {MAX_FOB_CHARS} characters")),
        DEFAULT_FOB => Ok(v.to_string()),
        DEFAULT_MARKUP => match crate::price_levels::parse_percent_milli(if v.is_empty() { "0" } else { v }) {
            Some(milli) if (0..=MAX_MARKUP_MILLI).contains(&milli) => Ok(format_percent_milli(milli)),
            _ => Err("PREF_BAD_MARKUP: enter a markup from 0 to 1000 percent".into()),
        },
        AUTO_APPLY => match v {
            "exact" | "oldest" | "off" => Ok(v.to_string()),
            _ => Err("PREF_BAD_AUTO_APPLY: apply payments to an exact match, the oldest invoices, or not automatically".into()),
        },
        BILL_DUE_DAYS => match v.parse::<u32>() {
            Ok(days) if days <= MAX_BILL_DUE_DAYS => Ok(days.to_string()),
            _ => Err(format!("PREF_BAD_DUE_DAYS: enter 0 (due on receipt) to {MAX_BILL_DUE_DAYS} days")),
        },
        DEFAULT_DELIVERY => match v {
            "none" | "email" | "print" => Ok(v.to_string()),
            _ => Err("PREF_BAD_DELIVERY: send forms by email, print, or none".into()),
        },
        _ => match JOB_STATUSES.iter().find(|(_, k, _)| *k == key) {
            Some(_) if v.is_empty() => Err("PREF_REQUIRED: a job status needs a name".into()),
            Some(_) if v.chars().count() > MAX_STATUS_LABEL_CHARS => Err(format!("PREF_TOO_LONG: a job status name must be at most {MAX_STATUS_LABEL_CHARS} characters")),
            Some(_) => Ok(v.to_string()),
            None => return None,
        },
    })
}

/// 12500 → "12.5" (thousandths of a percent, without trailing zeros).
pub fn format_percent_milli(milli: i64) -> String {
    let (whole, frac) = (milli / 1000, (milli % 1000).abs());
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{frac:03}").trim_end_matches('0'))
    }
}

/// A default shipping method must be one on the Ship Via list and active.
pub async fn check_ship_method(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM ship_methods WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?;
    match active {
        Some(true) => Ok(()),
        Some(false) => Err("PREF_BAD_SHIP_METHOD: that shipping method is inactive".into()),
        None => Err("PREF_BAD_SHIP_METHOD: choose a shipping method from the Ship Via list".into()),
    }
}

/// Discounts taken are credited to this account, so it must be one a vendor credit can use: an
/// active expense or asset category.
pub async fn check_discount_account(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let row: Option<(String, String, bool, bool)> = sqlx::query_as(
        "SELECT t.classification, a.account_type, a.is_active, a.is_system FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        Some((class, kind, true, false)) if class == "EXPENSE" || matches!(kind.as_str(), "FIXED_ASSET" | "OTHER_ASSET" | "OTHER_CURRENT_ASSET") => Ok(()),
        _ => Err("PREF_BAD_DISCOUNT_ACCOUNT: discounts taken go to an active expense account (such as Purchase Discounts or Cost of Goods Sold)".into()),
    }
}

/// Discounts customers take are debited to this account: an active income account that reduces
/// sales (such as Sales Discounts), or an expense account for a company that books them as a cost.
pub async fn check_payment_discount_account<'e, E: sqlx::SqliteExecutor<'e>>(db: E, id: i64) -> Result<(), String> {
    let row: Option<(String, bool, bool)> = sqlx::query_as(
        "SELECT t.classification, a.is_active, a.is_system FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(db_err)?;
    match row {
        Some((class, true, false)) if class == "INCOME" || class == "EXPENSE" => Ok(()),
        _ => Err("PREF_BAD_DISCOUNT_ACCOUNT: discounts customers take go to an active income account (such as Sales Discounts) or an expense account".into()),
    }
}

// ------------------------------------------------------------------------------------------------
// Receive Payments
// ------------------------------------------------------------------------------------------------

/// How a payment is spread over open invoices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoApply {
    /// An invoice whose open balance equals the amount takes all of it; otherwise oldest first.
    Exact,
    /// Oldest first (by due date, then number), each paid in full before the next.
    Oldest,
    /// Nothing is applied until the person ticks invoices.
    Off,
}

impl AutoApply {
    pub fn parse(value: &str) -> AutoApply {
        match value {
            "oldest" => AutoApply::Oldest,
            "off" => AutoApply::Off,
            _ => AutoApply::Exact,
        }
    }
}

/// One open document and what it still owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    pub id: i64,
    pub balance_cents: i64,
}

/// What each invoice receives, in the order given (oldest first). Never more than an invoice's
/// balance; whatever is left over is not applied (the screen shows it as unapplied).
pub fn apply_payment(open: &[Open], amount_cents: i64, mode: AutoApply) -> Vec<(i64, i64)> {
    if amount_cents <= 0 || mode == AutoApply::Off {
        return Vec::new();
    }
    if mode == AutoApply::Exact {
        if let Some(inv) = open.iter().find(|i| i.balance_cents == amount_cents) {
            return vec![(inv.id, amount_cents)];
        }
    }
    let mut left = amount_cents;
    let mut out = Vec::new();
    for inv in open {
        if left == 0 {
            break;
        }
        let take = left.min(inv.balance_cents.max(0));
        if take > 0 {
            out.push((inv.id, take));
            left -= take;
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAmount {
    pub document_id: i64,
    /// Money applied.
    pub amount_cents: i64,
    /// Early-payment discount taken along with it (Receive Payments only).
    pub discount_cents: i64,
}

/// A customer's open invoices in one currency (`None` = home), oldest first: by due date, then
/// number, as Receive Payments lists them.
pub async fn open_invoices(pool: &SqlitePool, customer_name: &str, currency: Option<&str>) -> Result<Vec<Open>, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT i.id, i.total_cents - i.amount_paid_cents FROM invoices i JOIN customers c ON c.id = i.customer_id
         WHERE c.name = ?1 COLLATE NOCASE AND i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL') AND i.currency_code IS ?2
           AND i.total_cents > i.amount_paid_cents
         ORDER BY i.due_date, i.invoice_number",
    )
    .bind(customer_name.trim())
    .bind(currency.map(str::trim).filter(|c| !c.is_empty()))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|(id, balance_cents)| Open { id, balance_cents }).collect())
}

/// Receive Payments: how `amount_cents` from the customer is applied under the company preference,
/// or under `mode` (the Auto Apply button applies even when the preference is off). With a
/// `payment_date` and discounts taken automatically, an invoice whose terms still offer a discount
/// owes that much less, and takes the discount when the money pays the rest in full.
pub async fn payment_application(
    pool: &SqlitePool,
    customer_name: &str,
    currency: Option<&str>,
    amount_cents: i64,
    mode: Option<&str>,
    payment_date: Option<NaiveDate>,
) -> Result<Vec<AppliedAmount>, String> {
    let mode = AutoApply::parse(&match mode {
        Some(mode) => mode.to_string(),
        None => preferences::get(pool, AUTO_APPLY).await?,
    });
    let mut open = open_invoices(pool, customer_name, currency).await?;
    let mut discounts = BTreeMap::new();
    if let Some(date) = payment_date {
        if auto_customer_discounts(pool).await? {
            for inv in &mut open {
                let discount = offered_invoice_discount(pool, inv.id, date).await?.min(inv.balance_cents);
                if discount > 0 {
                    inv.balance_cents -= discount;
                    discounts.insert(inv.id, (discount, inv.balance_cents));
                }
            }
        }
    }
    Ok(apply_payment(&open, amount_cents, mode)
        .into_iter()
        .map(|(document_id, amount_cents)| {
            let discount_cents = match discounts.get(&document_id) {
                Some(&(discount, rest)) if amount_cents == rest => discount,
                _ => 0,
            };
            AppliedAmount { document_id, amount_cents, discount_cents }
        })
        .collect())
}

/// Whether Receive Payments takes early-payment discounts without being asked: the preference is
/// on and there is an account for them.
pub async fn auto_customer_discounts(pool: &SqlitePool) -> Result<bool, String> {
    Ok(preferences::flag(pool, PAYMENT_AUTO_DISCOUNTS).await? && !preferences::get(pool, PAYMENT_DISCOUNT_ACCOUNT).await?.is_empty())
}

/// The discount an invoice's terms offer when paid on `payment_date`, in the invoice's currency: on
/// its open balance (all of it, or what is left of a part-paid one), once, through the discount's
/// last day.
pub async fn offered_invoice_discount<'e, E: sqlx::SqliteExecutor<'e>>(db: E, invoice_id: i64, payment_date: NaiveDate) -> Result<i64, String> {
    // A part-paid invoice offers the discount on what is left; one that already took a discount
    // offers no more.
    let row: Option<(i64, i64, i64, Option<String>, bool)> = sqlx::query_as(
        "SELECT i.total_cents, i.amount_paid_cents, COALESCE(tm.discount_ppm, 0),
                CASE WHEN tm.discount_days IS NOT NULL THEN date(i.issue_date, '+' || tm.discount_days || ' days') END,
                EXISTS (SELECT 1 FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0
                        AND p.discount_cents + p.foreign_discount_cents > 0
                        AND NOT EXISTS (SELECT 1 FROM bounced_checks b WHERE b.invoice_payment_id = p.id AND b.is_voided = 0))
         FROM invoices i LEFT JOIN terms tm ON tm.id = i.terms_id
         WHERE i.id = ?1 AND i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL')",
    )
    .bind(invoice_id)
    .fetch_optional(db)
    .await
    .map_err(db_err)?;
    let Some((total, paid, ppm, until, discounted)) = row else { return Ok(0) };
    let until = until.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    let open = total - paid;
    if discounted || open <= 1 || ppm == 0 || until.is_none_or(|u| payment_date > u) {
        return Ok(0);
    }
    // Never the whole balance: some money must change hands.
    Ok(crate::profile_lists::early_discount_cents(open, ppm).unwrap_or(0).clamp(0, open - 1))
}

/// An early-payment discount one invoice's terms offer on the payment date.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OfferedDiscount {
    pub invoice_id: i64,
    pub amount_cents: i64,
}

/// What Receive Payments may take off the invoices listed when paid on `payment_date`.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentDiscounts {
    /// A discount account is set, so a discount can be taken at all.
    pub available: bool,
    /// Take them as invoices are paid in full, without being asked.
    pub automatic: bool,
    /// Each invoice whose terms still offer a discount.
    pub offered: Vec<OfferedDiscount>,
}

/// Receive Payments: the early-payment discounts `invoice_ids` still offer on `payment_date`.
pub async fn payment_discounts(pool: &SqlitePool, invoice_ids: &[i64], payment_date: NaiveDate) -> Result<PaymentDiscounts, String> {
    let available = !preferences::get(pool, PAYMENT_DISCOUNT_ACCOUNT).await?.is_empty();
    let mut out = PaymentDiscounts { available, automatic: available && preferences::flag(pool, PAYMENT_AUTO_DISCOUNTS).await?, offered: Vec::new() };
    if !available {
        return Ok(out);
    }
    for &invoice_id in invoice_ids {
        let amount_cents = offered_invoice_discount(pool, invoice_id, payment_date).await?;
        if amount_cents > 0 {
            out.offered.push(OfferedDiscount { invoice_id, amount_cents });
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// Bills
// ------------------------------------------------------------------------------------------------

/// A new bill's due date: the vendor's terms when it has them (a term from the Terms list, or a
/// number of days), otherwise the company's "bills are due N days after the bill date". A vendor
/// not yet on the list has no terms.
pub async fn bill_due_date(pool: &SqlitePool, vendor_name: &str, bill_date: NaiveDate) -> Result<NaiveDate, String> {
    let terms: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE((SELECT t.net_days FROM terms t WHERE t.id = v.terms_id), v.terms_days) FROM vendors v WHERE v.name = ?1 COLLATE NOCASE",
    )
    .bind(vendor_name.trim())
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .flatten();
    let days = match terms {
        Some(days) => days,
        None => preferences::get(pool, BILL_DUE_DAYS).await?.parse::<i64>().unwrap_or(0),
    };
    Ok(bill_date + Duration::days(days.clamp(0, i64::from(MAX_BILL_DUE_DAYS))))
}

/// One vendor credit used on one bill.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreditUse {
    pub credit_id: i64,
    pub bill_id: i64,
    pub amount_cents: i64,
}

/// Spreads one vendor's credits (oldest first) over that vendor's chosen bills (in the order
/// given), each bill taking as much as it owes. Credits and bills must belong to the same vendor.
pub fn plan_vendor_credits(credits: &[Open], bills: &[Open]) -> Vec<CreditUse> {
    let mut left: Vec<i64> = credits.iter().map(|c| c.balance_cents.max(0)).collect();
    let mut out = Vec::new();
    for bill in bills {
        let mut owed = bill.balance_cents.max(0);
        for (credit, remaining) in credits.iter().zip(left.iter_mut()) {
            if owed == 0 {
                break;
            }
            let take = owed.min(*remaining);
            if take > 0 {
                out.push(CreditUse { credit_id: credit.id, bill_id: bill.id, amount_cents: take });
                *remaining -= take;
                owed -= take;
            }
        }
    }
    out
}

/// An early-payment discount taken on one bill.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscountUse {
    pub bill_id: i64,
    pub amount_cents: i64,
}

/// What Pay Bills takes off the chosen bills before paying them.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PayBillsPlan {
    /// Early-payment discounts the bills' terms still offer on the payment date.
    pub discounts: Vec<DiscountUse>,
    /// Vendor credits used on what the discounts leave.
    pub credits: Vec<CreditUse>,
}

/// The discount a bill's terms offer when paid on `payment_date`: only on a bill nothing has been
/// paid or credited on yet, and only through the discount's last day.
async fn offered_discount(pool: &SqlitePool, bill_id: i64, payment_date: NaiveDate) -> Result<i64, String> {
    let row: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT b.total_cents, b.amount_paid_cents, COALESCE(tm.discount_ppm, 0),
                CASE WHEN tm.discount_days IS NOT NULL THEN date(b.bill_date, '+' || tm.discount_days || ' days') END
         FROM bills b LEFT JOIN terms tm ON tm.id = b.terms_id
         WHERE b.id = ?1 AND b.is_voided = 0 AND b.status IN ('UNPAID', 'PARTIAL')",
    )
    .bind(bill_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((total, paid, ppm, until)) = row else { return Ok(0) };
    let until = until.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    if paid != 0 || ppm == 0 || until.is_none_or(|u| payment_date > u) {
        return Ok(0);
    }
    Ok(crate::profile_lists::early_discount_cents(total, ppm).unwrap_or(0).clamp(0, total))
}

/// Pay Bills: the discounts and vendor credits to use on `bill_ids` (in the order the screen lists
/// them) when paying on `payment_date`, as far as the company takes them automatically.
pub async fn pay_bills_plan(pool: &SqlitePool, bill_ids: &[i64], payment_date: NaiveDate) -> Result<PayBillsPlan, String> {
    let mut plan = PayBillsPlan::default();
    if bill_ids.is_empty() {
        return Ok(plan);
    }
    let discounts_on = preferences::flag(pool, BILL_AUTO_DISCOUNTS).await? && !preferences::get(pool, BILL_DISCOUNT_ACCOUNT).await?.is_empty();
    let credits_on = preferences::flag(pool, BILL_AUTO_CREDITS).await?;
    let mut by_vendor: BTreeMap<i64, Vec<Open>> = BTreeMap::new();
    for &id in bill_ids {
        let bill: Option<(i64, i64)> = sqlx::query_as(
            "SELECT vendor_id, total_cents - amount_paid_cents FROM bills WHERE id = ?1 AND is_voided = 0 AND status IN ('UNPAID', 'PARTIAL')",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        let Some((vendor, balance_cents)) = bill else { continue };
        let discount = if discounts_on { offered_discount(pool, id, payment_date).await?.min(balance_cents) } else { 0 };
        if discount > 0 {
            plan.discounts.push(DiscountUse { bill_id: id, amount_cents: discount });
        }
        by_vendor.entry(vendor).or_default().push(Open { id, balance_cents: balance_cents - discount });
    }
    if !credits_on {
        return Ok(plan);
    }
    for (vendor, bills) in by_vendor {
        let credits: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT c.id, b.remaining_cents FROM vendor_credits c JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id
             WHERE c.vendor_id = ?1 AND c.is_voided = 0 AND b.remaining_cents > 0 ORDER BY c.credit_date, c.id",
        )
        .bind(vendor)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let credits: Vec<Open> = credits.into_iter().map(|(id, balance_cents)| Open { id, balance_cents }).collect();
        plan.credits.extend(plan_vendor_credits(&credits, &bills));
    }
    Ok(plan)
}

/// Pay Bills with discounts and credits, in one transaction: each discount is posted as a vendor
/// credit to the discount account (Dr Accounts Payable, Cr the account) and used on its bill, then
/// the vendor credits are used, then the payments recorded. A bill paid off without money needs no
/// payment. A discount must be one the bill's terms still offer on `payment_date`.
pub async fn pay_bills_with_credits(
    pool: &SqlitePool,
    payments: Vec<crate::expenses::BillPaymentInput>,
    credits: Vec<crate::credits::ApplyCreditInput>,
    discounts: Vec<DiscountUse>,
    payment_date: NaiveDate,
) -> Result<usize, String> {
    if payments.is_empty() && credits.is_empty() && discounts.is_empty() {
        return Err("BILL_NOTHING_TO_PAY: select at least one bill".into());
    }
    let account = preferences::get(pool, BILL_DISCOUNT_ACCOUNT).await?.parse::<i64>().ok();
    let mut taken = Vec::with_capacity(discounts.len());
    for d in &discounts {
        let offered = offered_discount(pool, d.bill_id, payment_date).await?;
        let Some(account) = account.filter(|_| offered > 0) else {
            return Err("BILL_NO_DISCOUNT: that bill's terms offer no discount on this date, or no discount account is set in Preferences › Bills".into());
        };
        if !(1..=offered).contains(&d.amount_cents) {
            return Err(format!("BILL_BAD_DISCOUNT: the discount on that bill is at most {}", crate::ledger::format_cents(offered)));
        }
        let (vendor, number): (String, Option<String>) =
            sqlx::query_as("SELECT v.name, b.bill_number FROM bills b JOIN vendors v ON v.id = b.vendor_id WHERE b.id = ?1").bind(d.bill_id).fetch_one(pool).await.map_err(db_err)?;
        let memo = format!("Early-payment discount{}", number.map(|n| format!(" on bill {n}")).unwrap_or_default());
        taken.push((d, crate::credits::VendorCreditInput { form_lines: Vec::new(),
            vendor_name: vendor,
            reference: Some("Discount".into()),
            credit_date: payment_date,
            memo: Some(memo.clone()),
            lines: vec![crate::credits::VendorCreditLineInput { account_id: account, amount_cents: d.amount_cents, memo: Some(memo), class_id: None, customer_id: None }],
        }));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        for (d, credit) in &taken {
            let credit_id = crate::credits::write_vendor_credit(&mut tx, credit).await?;
            let apply = crate::credits::ApplyCreditInput { credit_id, document_id: d.bill_id, amount_cents: d.amount_cents, applied_date: payment_date };
            crate::credits::apply_vendor_credit_in_tx(&mut tx, &apply).await?;
        }
        for credit in &credits {
            crate::credits::apply_vendor_credit_in_tx(&mut tx, credit).await?;
        }
        let mut queued = Vec::new();
        for payment in &payments {
            let id = crate::expenses::write_bill_payment(&mut tx, payment, "STANDARD").await?;
            if payment.to_be_printed {
                queued.push(id);
            }
        }
        crate::expenses::group_queued_checks(&mut tx, &queued).await?;
        Ok::<(), String>(())
    }
    .await;
    match result {
        Ok(()) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            let mut bills: Vec<i64> =
                payments.iter().map(|p| p.bill_id).chain(credits.iter().map(|c| c.document_id)).chain(discounts.iter().map(|d| d.bill_id)).collect();
            bills.sort_unstable();
            bills.dedup();
            Ok(bills.len())
        }
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

// ------------------------------------------------------------------------------------------------
// Estimates and jobs
// ------------------------------------------------------------------------------------------------

/// The lines an estimate's invoice keeps: all of them, or (with the preference on) those with an
/// amount. An estimate whose lines are all zero keeps them, so the invoice is never empty.
pub fn without_zero_lines<T>(lines: Vec<T>, drop_zero: bool, amount: impl Fn(&T) -> i64) -> Vec<T> {
    if !drop_zero || lines.iter().all(|l| amount(l) == 0) {
        return lines;
    }
    lines.into_iter().filter(|l| amount(l) != 0).collect()
}

pub async fn drop_zero_lines(pool: &SqlitePool) -> Result<bool, String> {
    preferences::flag(pool, DROP_ZERO_LINES).await
}

/// The name shown for each job status, by stored code.
#[cfg(test)]
pub async fn job_status_labels(pool: &SqlitePool) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (code, key, _) in JOB_STATUSES {
        out.insert(code.to_string(), preferences::get(pool, key).await?);
    }
    Ok(out)
}

/// A job status code as stored, or `None` for blank.
pub fn parse_job_status(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if JOB_STATUSES.iter().any(|(code, _, _)| *code == v) => Ok(Some(v.to_string())),
        Some(v) => Err(format!("PARTY_BAD_JOB_STATUS: '{v}' is not a job status")),
    }
}

/// A customer's preferred way to send forms, as stored, or `None` for blank.
pub fn parse_delivery(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v @ ("EMAIL" | "PRINT" | "NONE")) => Ok(Some(v.to_string())),
        Some(v) => Err(format!("PARTY_BAD_DELIVERY: '{v}' is not a way to send forms")),
    }
}

#[cfg(test)]
mod tests;
