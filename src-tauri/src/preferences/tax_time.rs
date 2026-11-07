//! Company preferences of batch 5 in `docs/plans/preferences.md` (tax and time): Sales Tax,
//! Finance Charge, Time & Expenses, Tax: 1099, Items & Inventory and Checking.
//!
//! The keys live here with their validation so the shared `preferences.rs` only hooks them in.
//! Each key is read where the behaviour it changes lives; the doc comment says where.

use std::collections::BTreeMap;

use chrono::Weekday;
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// `accrual` (owe tax when invoiced) or `cash` (when paid): `sales_tax::tax_liability`.
pub const SALES_TAX_BASIS: &str = "sales_tax_basis";
/// `monthly`, `quarterly` or `yearly`: the Sales Tax report's default period and the return reminder.
pub const SALES_TAX_FREQUENCY: &str = "sales_tax_frequency";
/// Print a T beside taxable lines on invoices, sales receipts, credit memos and estimates (`pdf`).
pub const SALES_TAX_MARK_TAXABLE: &str = "sales_tax_mark_taxable";
/// `due_date` or `invoice_date`: where finance charge interest starts (`finance_charges`).
pub const FINANCE_CHARGE_FROM: &str = "finance_charge_from";
/// `monday` … `sunday`: the first day of a weekly timesheet (`timesheet::week_of`).
pub const WEEK_START: &str = "week_start_day";
/// New time entries start billable.
pub const TIME_BILLABLE: &str = "time_billable_default";
/// New expense and bill lines with a customer start billable (`expenses`).
pub const EXPENSES_BILLABLE: &str = "expenses_billable_default";
/// Billed expenses are credited to an income account instead of back to the expense account.
pub const REIMBURSED_AS_INCOME: &str = "reimbursed_as_income";
pub const REIMBURSED_ACCOUNT: &str = "reimbursed_income_account_id";
/// The markup added when billable expenses are invoiced, in percent, and the income account it goes to.
pub const MARKUP_PERCENT: &str = "billable_markup_percent";
pub const MARKUP_ACCOUNT: &str = "billable_markup_account_id";
/// "Do you file 1099s?": off hides the 1099 fields, reports and deadline.
pub const TAX_1099: &str = "tax_1099_enabled";
/// JSON object: expense account id → 1099 box code (`workflows::BOXES_1099`).
pub const TAX_1099_ACCOUNTS: &str = "tax_1099_accounts";
/// Inventory and purchase orders on or off.
pub const INVENTORY: &str = "inventory_enabled";
/// `off`, `warn` or `stop` when a sale would take stock below zero (`guards::stock_warnings`).
pub const NEGATIVE_STOCK: &str = "negative_stock";
/// A check left without a memo gets the vendor's account number (`expenses::write_expense`).
pub const CHECK_MEMO_ACCOUNT: &str = "check_memo_account_number";
/// A check printed from the queue takes the print date (`print_queue::mark_checks_printed`).
pub const DATE_CHECKS_PRINTED: &str = "date_checks_when_printed";

/// The largest markup accepted: 1,000%.
pub const MAX_MARKUP_MILLI_PERCENT: i64 = 1_000_000;

const FLAGS: &[&str] = &[SALES_TAX_MARK_TAXABLE, TIME_BILLABLE, EXPENSES_BILLABLE, REIMBURSED_AS_INCOME, TAX_1099, INVENTORY, CHECK_MEMO_ACCOUNT, DATE_CHECKS_PRINTED];

fn one_of(v: &str, allowed: &[&str], code: &str, what: &str) -> Result<String, String> {
    if allowed.contains(&v) {
        Ok(v.to_string())
    } else {
        Err(format!("{code}: {what}"))
    }
}

/// `Some` when `key` is one of these preferences: the normalized value, or why it is refused.
pub(super) fn validate(key: &str, v: &str) -> Option<Result<String, String>> {
    let result = match key {
        k if FLAGS.contains(&k) => one_of(v, &["true", "false"], "PREF_BAD_FLAG", "use true or false"),
        SALES_TAX_BASIS => one_of(v, &["accrual", "cash"], "PREF_BAD_TAX_BASIS", "owe sales tax as of invoice (accrual) or as of payment (cash)"),
        SALES_TAX_FREQUENCY => one_of(v, &["monthly", "quarterly", "yearly"], "PREF_BAD_TAX_FREQUENCY", "file sales tax monthly, quarterly or yearly"),
        FINANCE_CHARGE_FROM => one_of(v, &["due_date", "invoice_date"], "PREF_BAD_FC_FROM", "charge from the due date or the invoice date"),
        WEEK_START => parse_weekday(v).map(|_| v.to_string()),
        NEGATIVE_STOCK => one_of(v, &["off", "warn", "stop"], "PREF_BAD_NEGATIVE_STOCK", "when stock would go below zero: off, warn or stop"),
        MARKUP_PERCENT => parse_markup(v).map(crate::finance_charges::format_rate),
        REIMBURSED_ACCOUNT | MARKUP_ACCOUNT => match v {
            "" => Ok(String::new()),
            _ => v.parse::<i64>().map(|id| id.to_string()).map_err(|_| "PREF_BAD_INCOME_ACCOUNT: choose an Income account".into()),
        },
        TAX_1099_ACCOUNTS => parse_1099_accounts(v).and_then(|map| serde_json::to_string(&map).map_err(|e| e.to_string())),
        _ => return None,
    };
    Some(result)
}

/// Checks a value against the company file: the income accounts must exist, be active and be Income.
pub(super) async fn check(pool: &SqlitePool, key: &str, normalized: &str) -> Result<(), String> {
    match key {
        REIMBURSED_ACCOUNT | MARKUP_ACCOUNT if !normalized.is_empty() => {
            let account: Option<(String, bool)> = sqlx::query_as(
                "SELECT t.classification, a.is_active FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
            )
            .bind(normalized.parse::<i64>().unwrap_or_default())
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
            if matches!(account, Some((classification, true)) if classification == "INCOME") {
                Ok(())
            } else {
                Err("PREF_BAD_INCOME_ACCOUNT: choose an active Income account".into())
            }
        }
        TAX_1099_ACCOUNTS => {
            for id in parse_1099_accounts(normalized)?.keys() {
                let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chart_of_accounts WHERE id = ?1)").bind(id).fetch_one(pool).await.map_err(db_err)?;
                if !ok {
                    return Err(format!("PREF_BAD_1099_ACCOUNT: account {id} does not exist"));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn parse_weekday(v: &str) -> Result<Weekday, String> {
    match v {
        "monday" => Ok(Weekday::Mon),
        "tuesday" => Ok(Weekday::Tue),
        "wednesday" => Ok(Weekday::Wed),
        "thursday" => Ok(Weekday::Thu),
        "friday" => Ok(Weekday::Fri),
        "saturday" => Ok(Weekday::Sat),
        "sunday" => Ok(Weekday::Sun),
        _ => Err("PREF_BAD_WEEK_START: the work week starts on a day of the week (monday … sunday)".into()),
    }
}

/// A markup percent such as `15`, `12.5` or `7.125%` → thousandths of a percent, 0 to 1,000%.
pub fn parse_markup(v: &str) -> Result<i64, String> {
    let bad = || "PREF_BAD_MARKUP: enter a markup from 0 to 1000 percent, with up to 3 decimals".to_string();
    let t = v.trim().trim_end_matches('%').trim();
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if (whole.is_empty() && frac.is_empty()) || whole.len() > 4 || frac.len() > 3 || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| bad())? };
    let frac: i64 = format!("{frac:0<3}").parse().map_err(|_| bad())?;
    let milli = whole * 1000 + frac;
    if milli > MAX_MARKUP_MILLI_PERCENT {
        return Err(bad());
    }
    Ok(milli)
}

/// The 1099 account map: account id → box code. Unknown boxes are refused.
pub fn parse_1099_accounts(v: &str) -> Result<BTreeMap<i64, String>, String> {
    let bad = || "PREF_BAD_1099_ACCOUNTS: map accounts to 1099 boxes".to_string();
    let raw: BTreeMap<String, String> = serde_json::from_str(if v.trim().is_empty() { "{}" } else { v }).map_err(|_| bad())?;
    let mut out = BTreeMap::new();
    for (account, code) in raw {
        let id: i64 = account.parse().map_err(|_| bad())?;
        if code.is_empty() {
            continue;
        }
        if !crate::workflows::BOXES_1099.iter().any(|b| b.code == code) {
            return Err(format!("PREF_BAD_1099_BOX: '{code}' is not a 1099 box"));
        }
        out.insert(id, code);
    }
    Ok(out)
}

/// The 1099 account map as saved.
pub async fn accounts_1099(pool: &SqlitePool) -> Result<BTreeMap<i64, String>, String> {
    parse_1099_accounts(&super::get(pool, TAX_1099_ACCOUNTS).await?)
}
