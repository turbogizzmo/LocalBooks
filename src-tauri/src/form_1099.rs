//! Form 1099-NEC: W-9 details on vendors, and the year's forms with their 1096 summary.
//!
//! - **W-9.** A vendor's Form W-9 gives their legal name, business name, federal tax
//!   classification and taxpayer identification number (TIN): a Social Security number (or ITIN)
//!   for a person, an Employer Identification Number for a business. The details are kept in
//!   `vendor_w9` (migration 0125); the TIN itself stays in `vendors.tax_id`, where
//!   [`crate::tax_ids`] encrypts it once users are on. [`check_tin`] rejects numbers the IRS never
//!   issues. No error message, log line or audit record contains a TIN: the audit trail only notes
//!   that the masked TIN changed.
//! - **1099-NEC.** Box 1 (nonemployee compensation) for each vendor over the year's threshold,
//!   from [`crate::workflows::vendors_1099`] (payments by bank, split by account mapping), with what
//!   is missing to file it: a TIN, a W-9, an address. Recipient copies show the TIN masked to its
//!   last four digits, which the IRS allows on recipient statements; the full TIN appears only on the
//!   filer's worksheet, and only for a user who may see sensitive data.
//! - **1096.** The summary a paper filing is sent with: the payer, the number of forms and their
//!   total.
//!
//! Electronic filing in the IRS's IRIS format is not built: its XML schema is issued only to
//! registered software developers (see `docs/gaps.md`).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::tax_ids::{self, Access};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TinType {
    /// A Social Security number or an Individual Taxpayer Identification Number.
    Ssn,
    /// An Employer Identification Number.
    Ein,
}

impl TinType {
    fn code(self) -> &'static str {
        match self {
            TinType::Ssn => "SSN",
            TinType::Ein => "EIN",
        }
    }
}

/// The two-digit prefixes the IRS assigns EINs from (its campuses and online application).
const EIN_PREFIXES: &[std::ops::RangeInclusive<u8>] = &[1..=6, 10..=16, 20..=27, 30..=48, 50..=68, 71..=77, 80..=88, 90..=95, 98..=99];

/// The TIN in its usual form (`123-45-6789` or `12-3456789`), or why it cannot be one. The message
/// never repeats the number.
pub fn check_tin(kind: TinType, raw: &str) -> Result<String, String> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 9 || !raw.chars().all(|c| c.is_ascii_digit() || c == '-' || c == ' ') {
        return Err(format!("TIN_FORMAT: an {} has 9 digits", if kind == TinType::Ssn { "SSN or ITIN" } else { "EIN" }));
    }
    let n = |r: std::ops::Range<usize>| digits[r].parse::<u32>().unwrap_or(0);
    match kind {
        TinType::Ssn => {
            let (area, group, serial) = (n(0..3), n(3..5), n(5..9));
            if area >= 900 {
                // An ITIN: 9XX-NN-XXXX with NN in the ranges the IRS uses.
                if !matches!(group, 50..=65 | 70..=88 | 90..=92 | 94..=99) {
                    return Err("TIN_INVALID: that is not a valid ITIN (the middle two digits are not ones the IRS uses)".into());
                }
            } else if area == 0 || area == 666 || group == 0 || serial == 0 {
                return Err("TIN_INVALID: that is not a valid SSN (no part may be all zeros, and none starts with 666)".into());
            }
            Ok(format!("{}-{}-{}", &digits[..3], &digits[3..5], &digits[5..]))
        }
        TinType::Ein => {
            let prefix = u8::try_from(n(0..2)).unwrap_or(0);
            if !EIN_PREFIXES.iter().any(|r| r.contains(&prefix)) {
                return Err("TIN_INVALID: that is not a valid EIN (the IRS does not issue that prefix)".into());
            }
            Ok(format!("{}-{}", &digits[..2], &digits[2..]))
        }
    }
}

/// Federal tax classifications on Form W-9, line 3.
pub const CLASSIFICATIONS: &[(&str, &str)] = &[
    ("INDIVIDUAL", "Individual or sole proprietor"),
    ("C_CORPORATION", "C corporation"),
    ("S_CORPORATION", "S corporation"),
    ("PARTNERSHIP", "Partnership"),
    ("TRUST_ESTATE", "Trust or estate"),
    ("LLC_C", "LLC taxed as a C corporation"),
    ("LLC_S", "LLC taxed as an S corporation"),
    ("LLC_P", "LLC taxed as a partnership"),
    ("OTHER", "Other"),
];

/// A vendor's W-9 details, as one user may see them.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct W9 {
    pub vendor_id: i64,
    pub tin_type: Option<String>,
    /// The full TIN, when this user may see it.
    pub tin: Option<String>,
    pub tin_masked: Option<String>,
    /// The TIN is protected and this user may not see or change it.
    pub tin_locked: bool,
    pub legal_name: Option<String>,
    pub business_name: Option<String>,
    pub tax_classification: Option<String>,
    pub exempt_payee_code: Option<String>,
    pub received_on: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct W9Input {
    pub tin_type: Option<TinType>,
    pub tin: Option<String>,
    /// Leave the stored TIN as it is (a user who may not see it, or a form that did not change it).
    #[serde(default)]
    pub keep_tin: bool,
    pub legal_name: Option<String>,
    pub business_name: Option<String>,
    pub tax_classification: Option<String>,
    pub exempt_payee_code: Option<String>,
    pub received_on: Option<NaiveDate>,
}

#[derive(sqlx::FromRow)]
struct W9Row {
    tax_id: Option<String>,
    tax_id_masked: Option<String>,
    tin_type: Option<String>,
    legal_name: Option<String>,
    business_name: Option<String>,
    tax_classification: Option<String>,
    exempt_payee_code: Option<String>,
    received_on: Option<NaiveDate>,
}

pub async fn get_w9(pool: &SqlitePool, access: Access<'_>, vendor_id: i64) -> Result<W9, String> {
    let row: W9Row = sqlx::query_as(
        "SELECT v.tax_id, v.tax_id_masked, w.tin_type, w.legal_name, w.business_name, w.tax_classification, w.exempt_payee_code, w.received_on
         FROM vendors v LEFT JOIN vendor_w9 w ON w.vendor_id = v.id WHERE v.id = ?1",
    )
    .bind(vendor_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("VENDOR_NOT_FOUND: no vendor {vendor_id}"))?;
    let view = tax_ids::show(access, vendor_id, row.tax_id.as_deref(), row.tax_id_masked.as_deref())?;
    Ok(W9 {
        vendor_id,
        tin_type: row.tin_type,
        tin: view.full,
        tin_masked: view.masked,
        tin_locked: !access.can_see(),
        legal_name: row.legal_name,
        business_name: row.business_name,
        tax_classification: row.tax_classification,
        exempt_payee_code: row.exempt_payee_code,
        received_on: row.received_on,
    })
}

fn clean(v: Option<&str>, max: usize, what: &str) -> Result<Option<String>, String> {
    let v = v.map(str::trim).filter(|s| !s.is_empty());
    match v {
        Some(s) if s.chars().count() > max => Err(format!("W9_TOO_LONG: the {what} is longer than {max} characters")),
        _ => Ok(v.map(str::to_string)),
    }
}

/// Saves a vendor's W-9 details, checking the TIN against its type.
pub async fn set_w9(pool: &SqlitePool, access: Access<'_>, vendor_id: i64, input: &W9Input) -> Result<W9, String> {
    let legal_name = clean(input.legal_name.as_deref(), 150, "legal name")?;
    let business_name = clean(input.business_name.as_deref(), 150, "business name")?;
    let exempt = clean(input.exempt_payee_code.as_deref(), 2, "exempt payee code")?;
    if let Some(c) = &input.tax_classification {
        if !CLASSIFICATIONS.iter().any(|(code, _)| code == c) {
            return Err("W9_CLASSIFICATION: choose a tax classification from the list".into());
        }
    }
    let tin = match (&input.tin, input.keep_tin) {
        (_, true) => None,
        (Some(t), false) if !t.trim().is_empty() => {
            let kind = input.tin_type.ok_or_else(|| "TIN_TYPE: say whether the TIN is an SSN or an EIN".to_string())?;
            Some(check_tin(kind, t)?)
        }
        _ => None,
    };
    if !input.keep_tin && !access.can_see() {
        return Err("TAX_ID_LOCKED: only users who can see sensitive data can change a tax ID".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<(), String> = async {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM vendors WHERE id = ?1)").bind(vendor_id).fetch_one(&mut *tx).await.map_err(db_err)?;
        if !exists {
            return Err(format!("VENDOR_NOT_FOUND: no vendor {vendor_id}"));
        }
        sqlx::query(
            "INSERT INTO vendor_w9 (vendor_id, tin_type, legal_name, business_name, tax_classification, exempt_payee_code, received_on) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (vendor_id) DO UPDATE SET tin_type = excluded.tin_type, legal_name = excluded.legal_name, business_name = excluded.business_name,
               tax_classification = excluded.tax_classification, exempt_payee_code = excluded.exempt_payee_code, received_on = excluded.received_on",
        )
        .bind(vendor_id)
        .bind(input.tin_type.map(TinType::code))
        .bind(&legal_name)
        .bind(&business_name)
        .bind(&input.tax_classification)
        .bind(&exempt)
        .bind(input.received_on)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if !input.keep_tin {
            let (stored, masked) = tax_ids::store(access, vendor_id, tin.as_deref())?;
            sqlx::query("UPDATE vendors SET tax_id = ?1, tax_id_masked = ?2 WHERE id = ?3").bind(stored).bind(masked).bind(vendor_id).execute(&mut *tx).await.map_err(db_err)?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Err(e) => return Err(crate::ledger::rollback(tx, e).await),
    }
    get_w9(pool, access, vendor_id).await
}

/// The payer: the company, from Settings › Company.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payer {
    pub name: String,
    pub ein: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

/// One Form 1099-NEC.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Nec1099 {
    pub vendor_id: i64,
    /// The W-9 legal name, or the vendor's name.
    pub recipient_name: String,
    pub business_name: Option<String>,
    pub tin_type: Option<String>,
    /// For the filer's worksheet, when this user may see it.
    pub tin: Option<String>,
    /// For recipient copies.
    pub tin_masked: Option<String>,
    pub address: Option<String>,
    pub box1_cents: i64,
    pub w9_received_on: Option<NaiveDate>,
    /// What is missing to file this form.
    pub missing: Vec<String>,
}

/// The year's 1099-NEC forms and their Form 1096 totals.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary1096 {
    pub year: i32,
    pub payer: Payer,
    pub forms: Vec<Nec1099>,
    pub form_count: usize,
    pub total_cents: i64,
    pub threshold_cents: i64,
    /// Vendors paid in box 1 but under the threshold: no form is due.
    pub under_threshold: Vec<(String, i64)>,
    /// What is missing for the payer.
    pub payer_missing: Vec<String>,
    /// The full TINs are on the worksheet (this user may see them).
    pub full_tins: bool,
}

async fn pref(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    let v: Option<String> = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(key).fetch_optional(pool).await.map_err(db_err)?;
    Ok(v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
}

#[derive(sqlx::FromRow)]
struct W9Brief {
    vendor_id: i64,
    tin_type: Option<String>,
    legal_name: Option<String>,
    business_name: Option<String>,
    received_on: Option<NaiveDate>,
}

/// The 1099-NEC forms for payments in `year`, with the 1096 summary.
pub async fn nec_summary(pool: &SqlitePool, year: i32, access: Access<'_>) -> Result<Summary1096, String> {
    let rows = crate::workflows::vendors_1099(pool, year, access).await?;
    let w9s: std::collections::HashMap<i64, W9Brief> =
        sqlx::query_as::<_, W9Brief>("SELECT vendor_id, tin_type, legal_name, business_name, received_on FROM vendor_w9")
            .fetch_all(pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|w| (w.vendor_id, w))
            .collect();
    let threshold = crate::workflows::threshold_1099_cents("NEC-1", year);
    let mut forms = Vec::new();
    let mut under = Vec::new();
    let mut total: i64 = 0;
    for r in rows {
        let Some(b) = r.boxes.iter().find(|b| b.code == "NEC-1") else { continue };
        if b.paid_cents <= 0 {
            continue;
        }
        if !b.over_threshold {
            under.push((r.vendor_name.clone(), b.paid_cents));
            continue;
        }
        let w9 = w9s.get(&r.vendor_id);
        let mut missing = Vec::new();
        if r.tax_id_masked.is_none() {
            missing.push("TIN".to_string());
        } else if w9.and_then(|w| w.tin_type.as_ref()).is_none() {
            missing.push("whether the TIN is an SSN or EIN".to_string());
        }
        if w9.and_then(|w| w.received_on).is_none() {
            missing.push("W-9 received date".to_string());
        }
        if r.address.as_deref().is_none_or(|a| a.trim().is_empty()) {
            missing.push("address".to_string());
        }
        total = total.checked_add(b.paid_cents).ok_or_else(|| "RPT_OVERFLOW: the 1099 amounts are out of range".to_string())?;
        forms.push(Nec1099 {
            vendor_id: r.vendor_id,
            recipient_name: w9.and_then(|w| w.legal_name.clone()).unwrap_or_else(|| r.vendor_name.clone()),
            business_name: w9.and_then(|w| w.business_name.clone()),
            tin_type: w9.and_then(|w| w.tin_type.clone()),
            tin: r.tax_id.clone(),
            tin_masked: r.tax_id_masked.clone(),
            address: r.address.clone(),
            box1_cents: b.paid_cents,
            w9_received_on: w9.and_then(|w| w.received_on),
            missing,
        });
    }
    let name = match pref(pool, "company_legal_name").await? {
        Some(n) => n,
        None => pref(pool, "company_name").await?.unwrap_or_default(),
    };
    let payer = Payer {
        name,
        ein: pref(pool, "company_ein").await?,
        address: pref(pool, "company_address").await?,
        phone: pref(pool, "company_phone").await?,
        email: pref(pool, "company_email").await?,
    };
    let mut payer_missing = Vec::new();
    if payer.name.is_empty() {
        payer_missing.push("company name".to_string());
    }
    if payer.ein.is_none() {
        payer_missing.push("company EIN".to_string());
    }
    if payer.address.is_none() {
        payer_missing.push("company address".to_string());
    }
    let form_count = forms.len();
    Ok(Summary1096 {
        year,
        payer,
        forms,
        form_count,
        total_cents: total,
        threshold_cents: threshold,
        under_threshold: under,
        payer_missing,
        full_tins: access.can_see(),
    })
}

#[cfg(test)]
mod tests;
