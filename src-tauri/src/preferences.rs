//! Company preferences (key/value), fiscal-year arithmetic, and the federal tax calendar.

use std::collections::BTreeMap;

use chrono::{Datelike, Months, NaiveDate, Weekday};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

pub const TAX_FORMS: &[(&str, &str)] = &[
    ("SCHEDULE_C", "Form 1040, Schedule C (sole proprietor / single-member LLC)"),
    ("1065", "Form 1065 (partnership / multi-member LLC)"),
    ("1120S", "Form 1120-S (S corporation)"),
    ("1120", "Form 1120 (C corporation)"),
];

/// Validates a preference value; unknown keys are rejected so typos never persist.
fn validate(key: &str, value: &str) -> Result<String, String> {
    let v = value.trim();
    let text = |max: usize, label: &str| -> Result<String, String> {
        if v.chars().count() > max {
            Err(format!("PREF_TOO_LONG: {label} must be at most {max} characters"))
        } else {
            Ok(v.to_string())
        }
    };
    match key {
        "company_name" => {
            if v.is_empty() {
                return Err("PREF_REQUIRED: company name cannot be blank".into());
            }
            text(100, "company name")
        }
        "company_legal_name" => text(150, "legal name"),
        "auto_lock_minutes" => match v.parse::<u32>() {
            Ok(minutes) if minutes <= 240 => Ok(minutes.to_string()),
            _ => Err("PREF_BAD_AUTO_LOCK: enter 0 (never) to 240 minutes".into()),
        },
        "company_address" => text(300, "address"),
        "company_phone" => text(40, "phone"),
        "company_email" => {
            if !v.is_empty() && !(v.contains('@') && v.rsplit('@').next().is_some_and(|d| d.contains('.'))) {
                return Err("PREF_BAD_EMAIL: enter a valid email address".into());
            }
            text(150, "email")
        }
        "company_ein" => {
            let digits: String = v.chars().filter(char::is_ascii_digit).collect();
            if v.is_empty() {
                Ok(String::new())
            } else if digits.len() == 9 && v.chars().all(|c| c.is_ascii_digit() || c == '-') {
                Ok(format!("{}-{}", &digits[..2], &digits[2..]))
            } else {
                Err("PREF_BAD_EIN: an EIN has 9 digits (XX-XXXXXXX)".into())
            }
        }
        "fiscal_year_start" => {
            parse_fiscal_start(v)?;
            Ok(v.to_string())
        }
        "home_currency_code" => {
            let code = v.to_ascii_uppercase();
            if code.len() != 3 || !code.chars().all(|c| c.is_ascii_uppercase()) {
                return Err("PREF_BAD_CURRENCY_CODE: use a three-letter ISO currency code such as USD".into());
            }
            Ok(code)
        }
        "currency_symbol" => {
            if v.is_empty() || v.chars().count() > 4 {
                return Err("PREF_BAD_CURRENCY: use a currency symbol of 1 to 4 characters".into());
            }
            Ok(v.to_string())
        }
        "tax_form" => {
            if TAX_FORMS.iter().any(|(code, _)| *code == v) {
                Ok(v.to_string())
            } else {
                Err(format!("PREF_BAD_TAX_FORM: unknown tax form '{v}'"))
            }
        }
        "class_tracking" | "class_prompt" | "sales_tax_enabled" => match v {
            "true" | "false" => Ok(v.to_string()),
            _ => Err("PREF_BAD_FLAG: use true or false".into()),
        },
        "class_assign" => match v {
            "line" | "transaction" => Ok(v.to_string()),
            _ => Err("PREF_BAD_CLASS_ASSIGN: assign classes per line or per transaction".into()),
        },
        "auto_backup_enabled" => match v {
            "true" | "false" => Ok(v.to_string()),
            _ => Err("PREF_BAD_FLAG: use true or false".into()),
        },
        "auto_backup_keep" => match v.parse::<u32>() {
            Ok(n) if (1..=60).contains(&n) => Ok(n.to_string()),
            _ => Err("PREF_BAD_KEEP: keep between 1 and 60 automatic backups".into()),
        },
        LOGO_KEY => Err("PREF_LOGO: set the logo with the logo upload, which validates the image".into()),
        crate::smtp::SETTINGS_KEY => Err("PREF_SMTP: change email server settings in Settings › Email".into()),
        other => Err(format!("PREF_UNKNOWN: '{other}' is not a preference")),
    }
}

/// "MM-01" → month number.
pub fn parse_fiscal_start(value: &str) -> Result<u32, String> {
    let bad = || "PREF_BAD_FISCAL_START: fiscal year must start on the first day of a month (MM-01)".to_string();
    let (mm, dd) = value.split_once('-').ok_or_else(bad)?;
    let month: u32 = mm.parse().map_err(|_| bad())?;
    if mm.len() != 2 || dd != "01" || !(1..=12).contains(&month) {
        return Err(bad());
    }
    Ok(month)
}

/// All preferences except the logo, which is large and fetched separately, and the email server
/// settings, which have their own commands.
pub async fn get_all(pool: &SqlitePool) -> Result<BTreeMap<String, String>, String> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM system_preferences WHERE key NOT IN (?1, ?2)")
        .bind(LOGO_KEY)
        .bind(crate::smtp::SETTINGS_KEY)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

pub async fn get(pool: &SqlitePool, key: &str) -> Result<String, String> {
    sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("PREF_MISSING: preference '{key}' is not set"))
}

/// Validates and saves one preference immediately; returns the stored (normalized) value.
pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<String, String> {
    let normalized = validate(key, value)?;
    if key == "home_currency_code" && normalized != get(pool, key).await? {
        // Every posted amount is stored in the home currency; it cannot be relabelled afterwards.
        let has_foreign: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journal_entries WHERE foreign_currency_code IS NOT NULL)")
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
        if has_foreign {
            return Err("PREF_HOME_CURRENCY_LOCKED: the home currency cannot change once foreign-currency transactions exist".into());
        }
    }
    sqlx::query(
        "INSERT INTO system_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(key)
    .bind(&normalized)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(normalized)
}

// ------------------------------------------------------------------------------------------------
// Company logo
// ------------------------------------------------------------------------------------------------

pub const LOGO_KEY: &str = "company_logo_base64";
/// Logos are embedded in every printed page, so keep them small.
pub const MAX_LOGO_BYTES: usize = 512 * 1024;

/// Identifies a raster image by its signature bytes (the file extension is not trusted). SVG is
/// deliberately not accepted: it can carry scripts and external references.
pub fn logo_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Validates image bytes and stores them as a data URL; returns the stored URL.
pub async fn set_logo(pool: &SqlitePool, bytes: &[u8]) -> Result<String, String> {
    use base64::Engine;
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(format!("PREF_LOGO_TOO_LARGE: the logo is {} KB; use an image under {} KB", bytes.len() / 1024, MAX_LOGO_BYTES / 1024));
    }
    let mime = logo_mime(bytes).ok_or_else(|| "PREF_LOGO_FORMAT: use a PNG, JPEG, GIF or WebP image".to_string())?;
    let url = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    sqlx::query(
        "INSERT INTO system_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(LOGO_KEY)
    .bind(&url)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(url)
}

pub async fn clear_logo(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("UPDATE system_preferences SET value = '', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE key = ?1")
        .bind(LOGO_KEY)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The logo data URL, or `None` when no logo is set.
pub async fn get_logo(pool: &SqlitePool) -> Result<Option<String>, String> {
    Ok(Some(get(pool, LOGO_KEY).await?).filter(|v| !v.is_empty()))
}

pub async fn fiscal_start_month(pool: &SqlitePool) -> Result<u32, String> {
    parse_fiscal_start(&get(pool, "fiscal_year_start").await?)
}

pub async fn currency_symbol(pool: &SqlitePool) -> Result<String, String> {
    get(pool, "currency_symbol").await
}

// ------------------------------------------------------------------------------------------------
// Fiscal periods
// ------------------------------------------------------------------------------------------------

/// First and last day of the fiscal year containing `date`.
pub fn fiscal_year_bounds(date: NaiveDate, start_month: u32) -> (NaiveDate, NaiveDate) {
    let year = if date.month() >= start_month { date.year() } else { date.year() - 1 };
    let start = NaiveDate::from_ymd_opt(year, start_month, 1).expect("valid fiscal start");
    let end = start + Months::new(12) - chrono::Duration::days(1);
    (start, end)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalPeriods {
    pub start_month: u32,
    pub this_year: DateRange,
    pub last_year: DateRange,
    pub this_quarter: DateRange,
    pub year_to_date: DateRange,
}

pub fn fiscal_periods(as_of: NaiveDate, start_month: u32) -> FiscalPeriods {
    let (start, end) = fiscal_year_bounds(as_of, start_month);
    let (last_start, last_end) = fiscal_year_bounds(start - chrono::Duration::days(1), start_month);
    let months_in = (as_of.year() - start.year()) as u32 * 12 + as_of.month() - start.month();
    let q_start = start + Months::new(months_in / 3 * 3);
    let q_end = q_start + Months::new(3) - chrono::Duration::days(1);
    FiscalPeriods {
        start_month,
        this_year: DateRange { start, end },
        last_year: DateRange { start: last_start, end: last_end },
        this_quarter: DateRange { start: q_start, end: q_end },
        year_to_date: DateRange { start, end: as_of },
    }
}

// ------------------------------------------------------------------------------------------------
// Federal tax calendar
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxDeadline {
    pub due_date: NaiveDate,
    /// The statutory date before weekend adjustment.
    pub statutory_date: NaiveDate,
    pub label: String,
    pub form: String,
    pub is_past: bool,
}

/// Weekend due dates move to the following Monday. (Federal holidays are not adjusted.)
fn next_business_day(date: NaiveDate) -> NaiveDate {
    match date.weekday() {
        Weekday::Sat => date + chrono::Duration::days(2),
        Weekday::Sun => date + chrono::Duration::days(1),
        _ => date,
    }
}

/// The 15th day of the `n`th month after (n > 0) or of (counting from 1) a period boundary.
fn fifteenth_of_month_after(month_start: NaiveDate, months: u32) -> NaiveDate {
    let m = month_start + Months::new(months);
    NaiveDate::from_ymd_opt(m.year(), m.month(), 15).expect("15th exists")
}

/// Federal return and estimated-payment due dates for the fiscal year containing `as_of` and the
/// return for the fiscal year before it, for the configured entity type.
pub fn tax_calendar(tax_form: &str, start_month: u32, as_of: NaiveDate) -> Vec<TaxDeadline> {
    let (start, end) = fiscal_year_bounds(as_of, start_month);
    let (prior_start, prior_end) = fiscal_year_bounds(start - chrono::Duration::days(1), start_month);
    let after_end = |fy_end: NaiveDate| NaiveDate::from_ymd_opt(fy_end.year(), fy_end.month(), 1).expect("first of month");
    let fy_label = |s: NaiveDate, e: NaiveDate| {
        if s.month() == 1 { format!("{}", s.year()) } else { format!("FY {}–{}", s.format("%b %Y"), e.format("%b %Y")) }
    };

    let mut items: Vec<(NaiveDate, String, &str)> = Vec::new();
    let return_months = |fy_start: NaiveDate, fy_end: NaiveDate| -> u32 {
        match tax_form {
            "1065" | "1120S" => 3,
            // C corporations with a June 30 year end kept the 3rd-month due date for tax years
            // beginning before 2026; every other C corporation files by the 4th month.
            "1120" if fy_end.month() == 6 && fy_start.year() < 2026 => 3,
            _ => 4,
        }
    };
    let form_name = match tax_form {
        "1065" => "Form 1065",
        "1120S" => "Form 1120-S",
        "1120" => "Form 1120",
        _ => "Form 1040 / Schedule C",
    };

    for (s, e) in [(prior_start, prior_end), (start, end)] {
        items.push((
            fifteenth_of_month_after(after_end(e), return_months(s, e)),
            format!("{form_name} return due for {}", fy_label(s, e)),
            form_name,
        ));
    }

    match tax_form {
        "1120" => {
            for (n, month) in [(1, 3), (2, 5), (3, 8), (4, 11)] {
                items.push((fifteenth_of_month_after(start, month), format!("Estimated tax installment {n} ({})", fy_label(start, end)), "Form 1120-W"));
            }
        }
        "SCHEDULE_C" => {
            for (n, month) in [(1, 3), (2, 5), (3, 8), (4, 12)] {
                items.push((fifteenth_of_month_after(start, month), format!("Estimated tax payment {n} ({})", fy_label(start, end)), "Form 1040-ES"));
            }
        }
        _ => {}
    }

    // Information returns for the calendar year in which the fiscal year starts.
    let info_due = NaiveDate::from_ymd_opt(start.year() + 1, 1, 31).expect("Jan 31");
    items.push((info_due, format!("Forms 1099-NEC and W-2 due for calendar {}", start.year()), "1099-NEC / W-2"));

    let mut deadlines: Vec<TaxDeadline> = items
        .into_iter()
        .map(|(statutory, label, form)| {
            let due = next_business_day(statutory);
            TaxDeadline { due_date: due, statutory_date: statutory, label, form: form.to_string(), is_past: due < as_of }
        })
        .collect();
    deadlines.sort_by_key(|d| d.due_date);
    deadlines
}

#[cfg(test)]
mod tests;
