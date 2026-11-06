//! The Customer & Vendor Profile lists: Terms, Payment Method, Customer Type, Vendor Type, Job Type
//! and Customer Message.
//!
//! * A term names how long a customer or the company has to pay ("Net 30"), and may offer an
//!   early-payment discount ("2% 10 Net 30": 2 percent off when paid within 10 days). Customers and
//!   vendors have a default term; an invoice or bill keeps the term it was issued with, and its due
//!   date is the document date plus the term's days (the form fills it in; it can be changed).
//! * A payment method is a name ("Visa") for one of the six kinds of payment the ledger records
//!   (cash, check, credit card, ACH, wire, other). Payments, sales receipts and expenses keep both.
//! * Types group customers, vendors and jobs for reports and lists; a customer message is a line
//!   printed at the foot of an invoice, sales receipt or estimate.
//!
//! As with sales reps, an entry that is used can be renamed or made inactive but not deleted; an
//! inactive entry stays on whatever has it and cannot be chosen again.

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::ledger::{db_err, mul_div_round};

/// Parts per million in one whole (a discount rate of 1,000,000 ppm would be 100%).
pub const PPM: i64 = 1_000_000;

fn unique_err(err: sqlx::Error, message: String) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        message
    } else {
        msg
    }
}

fn entry_name(name: &str, max: usize, what: &str, code: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("{code}_NAME_REQUIRED: enter a {what}"));
    }
    if name.chars().count() > max {
        return Err(format!("{code}_NAME_TOO_LONG: a {what} is at most {max} characters"));
    }
    Ok(name.to_string())
}

// ------------------------------------------------------------------------------------------------
// Terms
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TermsView {
    pub id: i64,
    pub name: String,
    /// Days from the document date to its due date.
    pub net_days: i64,
    /// Early-payment discount in parts per million (20,000 = 2%); 0 when there is none.
    pub discount_ppm: i64,
    /// Days from the document date during which the discount may be taken.
    pub discount_days: Option<i64>,
    pub is_active: bool,
    /// On a customer, vendor, invoice or bill, so it can no longer be deleted.
    pub is_used: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermsInput {
    pub name: String,
    pub net_days: i64,
    #[serde(default)]
    pub discount_ppm: i64,
    #[serde(default)]
    pub discount_days: Option<i64>,
}

const TERMS_USED: &str = "EXISTS (SELECT 1 FROM customers WHERE terms_id = t.id) OR EXISTS (SELECT 1 FROM vendors WHERE terms_id = t.id)
    OR EXISTS (SELECT 1 FROM invoices WHERE terms_id = t.id) OR EXISTS (SELECT 1 FROM bills WHERE terms_id = t.id)";

/// The due date for a document dated `date` under a term of `net_days`.
pub fn due_date(date: NaiveDate, net_days: i64) -> Result<NaiveDate, String> {
    let days = u64::try_from(net_days).map_err(|_| "TERMS_BAD_DAYS: terms are 0 to 365 days".to_string())?;
    date.checked_add_days(Days::new(days)).ok_or_else(|| "TERMS_BAD_DAYS: that due date is out of range".to_string())
}

/// The early-payment discount on `total_cents` at `discount_ppm`, rounded to the cent.
pub fn early_discount_cents(total_cents: i64, discount_ppm: i64) -> Option<i64> {
    mul_div_round(total_cents, discount_ppm, PPM)
}

impl TermsView {
    /// The document's due date and, with a discount, the last day it may be taken.
    pub fn dates(&self, date: NaiveDate) -> Result<(NaiveDate, Option<NaiveDate>), String> {
        let until = self.discount_days.map(|d| due_date(date, d)).transpose()?;
        Ok((due_date(date, self.net_days)?, until))
    }
}

fn terms_fields(input: &TermsInput) -> Result<(String, i64, Option<i64>), String> {
    let name = entry_name(&input.name, 31, "term name", "TERMS")?;
    if !(0..=365).contains(&input.net_days) {
        return Err("TERMS_BAD_DAYS: terms are 0 to 365 days".into());
    }
    if !(0..PPM).contains(&input.discount_ppm) {
        return Err("TERMS_BAD_DISCOUNT: a discount is 0 to less than 100 percent".into());
    }
    let discount_days = if input.discount_ppm == 0 {
        None
    } else {
        let days = input.discount_days.ok_or("TERMS_DISCOUNT_DAYS_REQUIRED: enter how many days the discount is offered")?;
        if !(0..=input.net_days).contains(&days) {
            return Err("TERMS_BAD_DISCOUNT_DAYS: the discount days are 0 up to the net days".into());
        }
        Some(days)
    };
    Ok((name, input.discount_ppm, discount_days))
}

pub async fn list_terms(pool: &SqlitePool) -> Result<Vec<TermsView>, String> {
    sqlx::query_as::<_, TermsView>(&format!(
        "SELECT t.id, t.name, t.net_days, t.discount_ppm, t.discount_days, t.is_active, ({TERMS_USED}) AS is_used FROM terms t
         ORDER BY t.net_days, t.discount_ppm, t.name COLLATE NOCASE"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_terms(pool: &SqlitePool, id: i64) -> Result<TermsView, String> {
    list_terms(pool).await?.into_iter().find(|t| t.id == id).ok_or_else(|| format!("TERMS_NOT_FOUND: term {id} does not exist"))
}

pub async fn create_terms(pool: &SqlitePool, input: TermsInput) -> Result<TermsView, String> {
    let (name, ppm, discount_days) = terms_fields(&input)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO terms (name, net_days, discount_ppm, discount_days) VALUES (?1, ?2, ?3, ?4) RETURNING id")
        .bind(&name)
        .bind(input.net_days)
        .bind(ppm)
        .bind(discount_days)
        .fetch_one(pool)
        .await
        .map_err(|e| unique_err(e, format!("TERMS_DUPLICATE: there is already a term named '{name}'")))?;
    get_terms(pool, id).await
}

/// Changes a term. Invoices and bills already issued keep their due dates; customers and vendors
/// with the term get the new days on their next documents.
pub async fn update_terms(pool: &SqlitePool, id: i64, input: TermsInput) -> Result<TermsView, String> {
    let (name, ppm, discount_days) = terms_fields(&input)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let done = sqlx::query("UPDATE terms SET name = ?1, net_days = ?2, discount_ppm = ?3, discount_days = ?4 WHERE id = ?5")
            .bind(&name)
            .bind(input.net_days)
            .bind(ppm)
            .bind(discount_days)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| unique_err(e, format!("TERMS_DUPLICATE: there is already a term named '{name}'")))?;
        if done.rows_affected() == 0 {
            return Err(format!("TERMS_NOT_FOUND: term {id} does not exist"));
        }
        // Records with the term keep their days in step.
        for table in ["customers", "vendors"] {
            sqlx::query(&format!("UPDATE {table} SET terms_days = ?1 WHERE terms_id = ?2 AND terms_days IS NOT ?1"))
                .bind(input.net_days)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Err(err) => return Err(crate::ledger::rollback(tx, err).await),
    }
    get_terms(pool, id).await
}

pub async fn set_terms_active(pool: &SqlitePool, id: i64, active: bool) -> Result<TermsView, String> {
    sqlx::query("UPDATE terms SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_terms(pool, id).await
}

pub async fn delete_terms(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let terms = get_terms(pool, id).await?;
    if terms.is_used {
        return Err(format!("TERMS_IN_USE: '{}' is on customers, vendors, invoices or bills; make it inactive instead", terms.name));
    }
    sqlx::query("DELETE FROM terms WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// Checks a term chosen for a record or document: it must exist, and be active unless it is the
/// one already there (`current`). Returns its net days.
pub(crate) async fn check_terms(conn: &mut SqliteConnection, id: i64, current: Option<i64>, code: &str) -> Result<i64, String> {
    let row: Option<(String, i64, bool)> =
        sqlx::query_as("SELECT name, net_days, is_active FROM terms WHERE id = ?1").bind(id).fetch_optional(&mut *conn).await.map_err(db_err)?;
    match row {
        None => Err(format!("{code}_UNKNOWN_TERMS: that term no longer exists")),
        Some((name, _, false)) if current != Some(id) => Err(format!("{code}_INACTIVE_TERMS: the term '{name}' is inactive")),
        Some((_, days, _)) => Ok(days),
    }
}

/// The term on an invoice or bill as its views show it. Selected with [`document_terms_sql`].
#[derive(Debug, Clone, Default, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTerms {
    pub terms_id: Option<i64>,
    pub terms_name: Option<String>,
    /// Early-payment discount rate in parts per million; 0 when the term has none.
    pub discount_ppm: i64,
    /// The last day the early-payment discount may be taken.
    pub discount_until: Option<String>,
    /// The discount on the document's total, filled in by [`DocumentTerms::fill`].
    #[sqlx(skip)]
    pub early_discount_cents: i64,
}

impl DocumentTerms {
    pub fn fill(&mut self, total_cents: i64) {
        self.early_discount_cents = early_discount_cents(total_cents, self.discount_ppm).unwrap_or(0);
    }
}

// ------------------------------------------------------------------------------------------------
// Payment methods
// ------------------------------------------------------------------------------------------------

/// The kinds of payment the ledger records, as `payment_method` on payments, receipts and expenses.
pub const PAYMENT_KINDS: &[&str] = &["CASH", "CHECK", "CREDIT_CARD", "ACH", "WIRE", "OTHER"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentMethodView {
    pub id: i64,
    pub name: String,
    /// One of [`PAYMENT_KINDS`].
    pub kind: String,
    pub is_active: bool,
    pub is_used: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentMethodInput {
    pub name: String,
    pub kind: String,
}

const METHOD_USED: &str = "EXISTS (SELECT 1 FROM invoice_payments WHERE payment_method_id = m.id)
    OR EXISTS (SELECT 1 FROM sales_receipts WHERE payment_method_id = m.id) OR EXISTS (SELECT 1 FROM expenses WHERE payment_method_id = m.id)";

fn method_fields(input: &PaymentMethodInput) -> Result<(String, &'static str), String> {
    let name = entry_name(&input.name, 31, "payment method", "PAYMENT_METHOD")?;
    let kind = PAYMENT_KINDS.iter().find(|k| **k == input.kind).ok_or_else(|| format!("PAYMENT_METHOD_BAD_KIND: '{}' is not a kind of payment", input.kind))?;
    Ok((name, kind))
}

pub async fn list_payment_methods(pool: &SqlitePool) -> Result<Vec<PaymentMethodView>, String> {
    sqlx::query_as::<_, PaymentMethodView>(&format!(
        "SELECT m.id, m.name, m.kind, m.is_active, ({METHOD_USED}) AS is_used FROM payment_methods m ORDER BY m.name COLLATE NOCASE"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_payment_method(pool: &SqlitePool, id: i64) -> Result<PaymentMethodView, String> {
    list_payment_methods(pool).await?.into_iter().find(|m| m.id == id).ok_or_else(|| format!("PAYMENT_METHOD_NOT_FOUND: payment method {id} does not exist"))
}

pub async fn create_payment_method(pool: &SqlitePool, input: PaymentMethodInput) -> Result<PaymentMethodView, String> {
    let (name, kind) = method_fields(&input)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO payment_methods (name, kind) VALUES (?1, ?2) RETURNING id")
        .bind(&name)
        .bind(kind)
        .fetch_one(pool)
        .await
        .map_err(|e| unique_err(e, format!("PAYMENT_METHOD_DUPLICATE: there is already a payment method named '{name}'")))?;
    get_payment_method(pool, id).await
}

/// Renames a method. Its kind can change only while nothing uses it, since payments already
/// recorded keep the kind they were recorded with.
pub async fn update_payment_method(pool: &SqlitePool, id: i64, input: PaymentMethodInput) -> Result<PaymentMethodView, String> {
    let (name, kind) = method_fields(&input)?;
    let current = get_payment_method(pool, id).await?;
    if current.is_used && current.kind != kind {
        return Err(format!("PAYMENT_METHOD_IN_USE: '{}' is on payments already, so its kind cannot change", current.name));
    }
    sqlx::query("UPDATE payment_methods SET name = ?1, kind = ?2 WHERE id = ?3")
        .bind(&name)
        .bind(kind)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| unique_err(e, format!("PAYMENT_METHOD_DUPLICATE: there is already a payment method named '{name}'")))?;
    get_payment_method(pool, id).await
}

pub async fn set_payment_method_active(pool: &SqlitePool, id: i64, active: bool) -> Result<PaymentMethodView, String> {
    sqlx::query("UPDATE payment_methods SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_payment_method(pool, id).await
}

pub async fn delete_payment_method(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let method = get_payment_method(pool, id).await?;
    if method.is_used {
        return Err(format!("PAYMENT_METHOD_IN_USE: '{}' is on payments; make it inactive instead", method.name));
    }
    sqlx::query("DELETE FROM payment_methods WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// The method and kind to record for a payment. A chosen method must exist and be active, and
/// decides the kind. Without one, the kind given is recorded with the first active method of that
/// kind (imports and older callers name only the kind), or with no method when none is active.
pub(crate) async fn resolve_payment_method(conn: &mut SqliteConnection, id: Option<i64>, kind: Option<&str>, code: &str) -> Result<(Option<i64>, Option<String>), String> {
    if let Some(id) = id {
        let row: Option<(String, String, bool)> =
            sqlx::query_as("SELECT name, kind, is_active FROM payment_methods WHERE id = ?1").bind(id).fetch_optional(&mut *conn).await.map_err(db_err)?;
        return match row {
            None => Err(format!("{code}_UNKNOWN_METHOD: that payment method no longer exists")),
            Some((name, _, false)) => Err(format!("{code}_INACTIVE_METHOD: the payment method '{name}' is inactive")),
            Some((_, kind, true)) => Ok((Some(id), Some(kind))),
        };
    }
    let Some(kind) = kind else { return Ok((None, None)) };
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM payment_methods WHERE kind = ?1 AND is_active = 1 ORDER BY id LIMIT 1")
        .bind(kind)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?;
    Ok((id, Some(kind.to_string())))
}

// ------------------------------------------------------------------------------------------------
// Customer, vendor and job types; customer messages
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProfileList {
    CustomerType,
    VendorType,
    JobType,
    CustomerMessage,
}

impl ProfileList {
    pub fn key(self) -> &'static str {
        match self {
            ProfileList::CustomerType => "CUSTOMER_TYPE",
            ProfileList::VendorType => "VENDOR_TYPE",
            ProfileList::JobType => "JOB_TYPE",
            ProfileList::CustomerMessage => "CUSTOMER_MESSAGE",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            ProfileList::CustomerType => "customer type",
            ProfileList::VendorType => "vendor type",
            ProfileList::JobType => "job type",
            ProfileList::CustomerMessage => "customer message",
        }
    }

    fn max_len(self) -> usize {
        if self == ProfileList::CustomerMessage {
            101
        } else {
            31
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntryView {
    pub id: i64,
    /// `CUSTOMER_TYPE`, `VENDOR_TYPE`, `JOB_TYPE` or `CUSTOMER_MESSAGE`.
    pub list: String,
    pub name: String,
    pub is_active: bool,
    pub is_used: bool,
}

const ENTRY_USED: &str = "EXISTS (SELECT 1 FROM customers WHERE customer_type_id = p.id OR job_type_id = p.id)
    OR EXISTS (SELECT 1 FROM vendors WHERE vendor_type_id = p.id)
    OR EXISTS (SELECT 1 FROM invoices WHERE customer_message_id = p.id) OR EXISTS (SELECT 1 FROM sales_receipts WHERE customer_message_id = p.id)
    OR EXISTS (SELECT 1 FROM estimates WHERE customer_message_id = p.id)";

/// Every type and message, by list and name.
pub async fn list_profile_entries(pool: &SqlitePool) -> Result<Vec<ProfileEntryView>, String> {
    sqlx::query_as::<_, ProfileEntryView>(&format!(
        "SELECT p.id, p.list, p.name, p.is_active, ({ENTRY_USED}) AS is_used FROM profile_entries p ORDER BY p.list, p.name COLLATE NOCASE"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_entry(pool: &SqlitePool, id: i64) -> Result<ProfileEntryView, String> {
    list_profile_entries(pool).await?.into_iter().find(|p| p.id == id).ok_or_else(|| format!("PROFILE_NOT_FOUND: list entry {id} does not exist"))
}

fn list_of(entry: &ProfileEntryView) -> Result<ProfileList, String> {
    serde_json::from_value(serde_json::Value::String(entry.list.clone())).map_err(|e| e.to_string())
}

pub async fn create_profile_entry(pool: &SqlitePool, list: ProfileList, name: &str) -> Result<ProfileEntryView, String> {
    let name = entry_name(name, list.max_len(), list.noun(), "PROFILE")?;
    let id: i64 = sqlx::query_scalar("INSERT INTO profile_entries (list, name) VALUES (?1, ?2) RETURNING id")
        .bind(list.key())
        .bind(&name)
        .fetch_one(pool)
        .await
        .map_err(|e| unique_err(e, format!("PROFILE_DUPLICATE: there is already a {} '{name}'", list.noun())))?;
    get_entry(pool, id).await
}

pub async fn rename_profile_entry(pool: &SqlitePool, id: i64, name: &str) -> Result<ProfileEntryView, String> {
    let list = list_of(&get_entry(pool, id).await?)?;
    let name = entry_name(name, list.max_len(), list.noun(), "PROFILE")?;
    sqlx::query("UPDATE profile_entries SET name = ?1 WHERE id = ?2")
        .bind(&name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| unique_err(e, format!("PROFILE_DUPLICATE: there is already a {} '{name}'", list.noun())))?;
    get_entry(pool, id).await
}

pub async fn set_profile_entry_active(pool: &SqlitePool, id: i64, active: bool) -> Result<ProfileEntryView, String> {
    sqlx::query("UPDATE profile_entries SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_entry(pool, id).await
}

pub async fn delete_profile_entry(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let entry = get_entry(pool, id).await?;
    if entry.is_used {
        return Err(format!("PROFILE_IN_USE: '{}' is in use; make it inactive instead", entry.name));
    }
    sqlx::query("DELETE FROM profile_entries WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// Checks an entry chosen for a record or document: it must be on `list`, and active unless it is
/// the one already there (`current`).
pub(crate) async fn check_entry(conn: &mut SqliteConnection, list: ProfileList, id: i64, current: Option<i64>, code: &str) -> Result<(), String> {
    let row: Option<(String, String, bool)> =
        sqlx::query_as("SELECT list, name, is_active FROM profile_entries WHERE id = ?1").bind(id).fetch_optional(&mut *conn).await.map_err(db_err)?;
    let tag = list.key();
    match row {
        Some((l, name, active)) if l == tag => {
            if active || current == Some(id) {
                Ok(())
            } else {
                Err(format!("{code}_INACTIVE_{tag}: the {} '{name}' is inactive", list.noun()))
            }
        }
        _ => Err(format!("{code}_UNKNOWN_{tag}: that {} does not exist", list.noun())),
    }
}

#[cfg(test)]
mod tests;
