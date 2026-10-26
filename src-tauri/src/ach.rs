//! Paying bills by ACH file: vendors' bank details, the paying account's originator details, and
//! the NACHA credit files Pay Bills writes for the user to upload to their bank ([`crate::nacha`]).
//!
//! Vendor routing and account numbers are sensitive data, kept like tax IDs ([`crate::tax_ids`]):
//! plain in a file without users; once users are on, encrypted with the file's data key next to a
//! masked copy, shown and changed only by users who may see sensitive data.
//!
//! [`pay_bills_by_ach`] records one bill payment per bill (kind ACH, its reference the trace number)
//! and one ACH entry per vendor, in one transaction, then the file is written from those rows by
//! [`save_ach_file`]. Saving again gives the same file and records nothing new, and a second Pay with
//! the same request ID returns the first file, so a bill is never paid twice by LocalBooks. (Uploading
//! the same file twice is another matter; the bank's duplicate check and the guide's warning cover it.)
//! Every file written is an ACH_FILE event in the audit trail, with amounts and trace numbers but no
//! account numbers.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};

use crate::crypto::{self, DataKey};
use crate::expenses::BillPaymentInput;
use crate::ledger::{db_err, rollback};
use crate::nacha::{self, AccountType, SecCode};
use crate::tax_ids::{mask, Access};

const LOCKED: &str = "BANK_DETAILS_LOCKED: only users who can see sensitive data can see or change bank details or pay by ACH";
const MODIFIERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn seal(access: Access, table: &str, column: &str, row_id: i64, plain: &str) -> Result<String, String> {
    if !access.protected {
        return Ok(plain.to_string());
    }
    access.key.ok_or_else(|| LOCKED.to_string())?.encrypt_field(table, column, row_id, plain)
}

fn open(access: Access, table: &str, column: &str, row_id: i64, stored: &str) -> Result<Option<String>, String> {
    if !crypto::is_encrypted(stored) {
        return Ok(Some(stored.to_string()).filter(|_| access.can_see()));
    }
    match access.key {
        Some(key) => key.decrypt_field(table, column, row_id, stored).map(Some),
        None => Ok(None),
    }
}

// ------------------------------------------------------------------------------------------------
// Vendor bank details
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorBankView {
    pub vendor_id: i64,
    /// The full numbers, when this user may see them.
    pub routing_number: Option<String>,
    pub account_number: Option<String>,
    pub routing_masked: String,
    pub account_masked: String,
    /// `CHECKING` or `SAVINGS`.
    pub account_type: String,
    /// `CCD` (a business) or `PPD` (a person).
    pub sec_code: String,
    /// This user can neither see nor change the details.
    pub locked: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorBankInput {
    pub routing_number: String,
    pub account_number: String,
    pub account_type: String,
    #[serde(default = "default_sec")]
    pub sec_code: String,
}

fn default_sec() -> String {
    "CCD".into()
}

#[derive(sqlx::FromRow)]
struct BankRow {
    vendor_id: i64,
    routing_number: String,
    routing_masked: String,
    account_number: String,
    account_masked: String,
    account_type: String,
    sec_code: String,
}

const BANK: &str = "vendor_bank_accounts";

/// A vendor's bank details as `access` may see them, or `None` when they have none.
pub async fn get_vendor_bank(pool: &SqlitePool, vendor_id: i64, access: Access<'_>) -> Result<Option<VendorBankView>, String> {
    let row: Option<BankRow> = sqlx::query_as("SELECT vendor_id, routing_number, routing_masked, account_number, account_masked, account_type, sec_code FROM vendor_bank_accounts WHERE vendor_id = ?1")
        .bind(vendor_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(VendorBankView {
        vendor_id,
        routing_number: open(access, BANK, "routing_number", vendor_id, &row.routing_number)?,
        account_number: open(access, BANK, "account_number", vendor_id, &row.account_number)?,
        routing_masked: row.routing_masked,
        account_masked: row.account_masked,
        account_type: row.account_type,
        sec_code: row.sec_code,
        locked: !access.can_see(),
    }))
}

/// Checks and normalizes typed bank details: a routing number that passes its ABA check digit, an
/// account number of 4 to 17 letters, digits or dashes, checking or savings, CCD or PPD.
pub fn check_bank_input(input: &VendorBankInput) -> Result<(String, String, AccountType, SecCode), String> {
    let routing: String = input.routing_number.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if routing.len() != 9 || !routing.bytes().all(|b| b.is_ascii_digit()) {
        return Err("BANK_BAD_ROUTING: a routing number is nine digits".into());
    }
    if !nacha::routing_number_ok(&routing) {
        return Err("BANK_BAD_ROUTING: that routing number fails its check digit; check it against the vendor's form".into());
    }
    let account = nacha::clean_account(&input.account_number);
    if !nacha::account_ok(&account) {
        return Err("BANK_BAD_ACCOUNT: an account number is 4 to 17 digits (letters and dashes are allowed)".into());
    }
    let kind = AccountType::parse(&input.account_type).ok_or("BANK_BAD_TYPE: choose checking or savings")?;
    let sec = SecCode::parse(&input.sec_code).ok_or("BANK_BAD_SEC: choose business (CCD) or person (PPD)")?;
    Ok((routing, account, kind, sec))
}

/// Saves a vendor's bank details, or removes them with `None`. Needs sensitive access once users are on.
pub async fn set_vendor_bank(pool: &SqlitePool, vendor_id: i64, input: Option<VendorBankInput>, access: Access<'_>) -> Result<Option<VendorBankView>, String> {
    if !access.can_see() {
        return Err(LOCKED.into());
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM vendors WHERE id = ?1").bind(vendor_id).fetch_optional(pool).await.map_err(db_err)?;
    if exists.is_none() {
        return Err("PARTY_NOT_FOUND: that vendor does not exist".into());
    }
    let Some(input) = input else {
        sqlx::query("DELETE FROM vendor_bank_accounts WHERE vendor_id = ?1").bind(vendor_id).execute(pool).await.map_err(db_err)?;
        return Ok(None);
    };
    let (routing, account, kind, sec) = check_bank_input(&input)?;
    sqlx::query(
        "INSERT INTO vendor_bank_accounts (vendor_id, routing_number, routing_masked, account_number, account_masked, account_type, sec_code)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (vendor_id) DO UPDATE SET routing_number = excluded.routing_number, routing_masked = excluded.routing_masked,
             account_number = excluded.account_number, account_masked = excluded.account_masked, account_type = excluded.account_type,
             sec_code = excluded.sec_code, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(vendor_id)
    .bind(seal(access, BANK, "routing_number", vendor_id, &routing)?)
    .bind(mask(&routing))
    .bind(seal(access, BANK, "account_number", vendor_id, &account)?)
    .bind(mask(&account))
    .bind(if kind == AccountType::Checking { "CHECKING" } else { "SAVINGS" })
    .bind(sec.as_str())
    .execute(pool)
    .await
    .map_err(db_err)?;
    get_vendor_bank(pool, vendor_id, access).await
}

/// Encrypts every plain bank detail: vendors' and the copies kept with ACH files. Runs with
/// [`crate::tax_ids::protect_all`].
pub async fn protect_all(conn: &mut SqliteConnection, key: &DataKey) -> Result<usize, String> {
    let access = Access { protected: true, key: Some(key) };
    let mut protected = 0;
    for (table, id_column) in [(BANK, "vendor_id"), ("ach_entries", "id")] {
        for column in ["routing_number", "account_number"] {
            let rows: Vec<(i64, String)> =
                sqlx::query_as(&format!("SELECT {id_column}, {column} FROM {table} WHERE {column} NOT LIKE 'lb1:%' AND {column} <> ''"))
                    .fetch_all(&mut *conn)
                    .await
                    .map_err(db_err)?;
            for (id, plain) in rows {
                sqlx::query(&format!("UPDATE {table} SET {column} = ?1 WHERE {id_column} = ?2"))
                    .bind(seal(access, table, column, id, &plain)?)
                    .bind(id)
                    .execute(&mut *conn)
                    .await
                    .map_err(db_err)?;
                protected += 1;
            }
        }
    }
    Ok(protected)
}

/// When the data key is removed with every user: encrypted bank details can no longer be read, so
/// vendors lose theirs and ACH files keep only masked copies (they can no longer be saved again).
/// Returns how many vendors lost their details.
pub async fn erase_encrypted(conn: &mut SqliteConnection) -> Result<i64, String> {
    let erased = sqlx::query("DELETE FROM vendor_bank_accounts WHERE routing_number LIKE 'lb1:%' OR account_number LIKE 'lb1:%'")
        .execute(&mut *conn)
        .await
        .map_err(db_err)?
        .rows_affected();
    sqlx::query("UPDATE ach_entries SET routing_number = '', account_number = '' WHERE routing_number LIKE 'lb1:%' OR account_number LIKE 'lb1:%'")
        .execute(&mut *conn)
        .await
        .map_err(db_err)?;
    Ok(erased as i64)
}

// ------------------------------------------------------------------------------------------------
// Originator details
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OriginatorDetails {
    pub account_id: i64,
    pub immediate_destination: String,
    pub immediate_destination_name: String,
    pub immediate_origin: String,
    pub immediate_origin_name: String,
    pub company_name: String,
    pub company_id: String,
    pub entry_description: String,
}

impl OriginatorDetails {
    fn nacha(&self) -> nacha::Originator {
        nacha::Originator {
            immediate_destination: self.immediate_destination.clone(),
            immediate_destination_name: self.immediate_destination_name.clone(),
            immediate_origin: self.immediate_origin.clone(),
            immediate_origin_name: self.immediate_origin_name.clone(),
            company_name: self.company_name.clone(),
            company_id: self.company_id.clone(),
            entry_description: self.entry_description.clone(),
        }
    }
}

const ORIGINATOR_COLUMNS: &str =
    "account_id, immediate_destination, immediate_destination_name, immediate_origin, immediate_origin_name, company_name, company_id, entry_description";

pub async fn get_ach_originator(pool: &SqlitePool, account_id: i64) -> Result<Option<OriginatorDetails>, String> {
    sqlx::query_as(&format!("SELECT {ORIGINATOR_COLUMNS} FROM ach_originators WHERE account_id = ?1")).bind(account_id).fetch_optional(pool).await.map_err(db_err)
}

/// Saves a bank account's ACH originator details, as the bank gave them.
pub async fn set_ach_originator(pool: &SqlitePool, input: OriginatorDetails) -> Result<OriginatorDetails, String> {
    let kind: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1").bind(input.account_id).fetch_optional(pool).await.map_err(db_err)?;
    if kind.as_deref() != Some("BANK") {
        return Err("ACH_NOT_BANK: ACH files are paid from a Bank account".into());
    }
    let upper = |s: &str| s.trim().to_ascii_uppercase();
    let details = OriginatorDetails {
        account_id: input.account_id,
        immediate_destination: input.immediate_destination.chars().filter(char::is_ascii_digit).collect(),
        immediate_destination_name: upper(&input.immediate_destination_name),
        immediate_origin: upper(&input.immediate_origin).replace([' ', '-'], ""),
        immediate_origin_name: upper(&input.immediate_origin_name),
        company_name: upper(&input.company_name),
        company_id: upper(&input.company_id).replace([' ', '-'], ""),
        entry_description: upper(&input.entry_description),
    };
    details.nacha().check()?;
    for (text, max, what) in [
        (&details.immediate_destination_name, 23, "bank name"),
        (&details.immediate_origin_name, 23, "company name for the bank"),
        (&details.company_name, 16, "company name on statements"),
        (&details.entry_description, 10, "entry description"),
    ] {
        if text.chars().count() > max {
            return Err(format!("ACH_TOO_LONG: the {what} has at most {max} characters"));
        }
    }
    sqlx::query(&format!(
        "INSERT INTO ach_originators ({ORIGINATOR_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (account_id) DO UPDATE SET immediate_destination = excluded.immediate_destination,
             immediate_destination_name = excluded.immediate_destination_name, immediate_origin = excluded.immediate_origin,
             immediate_origin_name = excluded.immediate_origin_name, company_name = excluded.company_name,
             company_id = excluded.company_id, entry_description = excluded.entry_description,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
    ))
    .bind(details.account_id)
    .bind(&details.immediate_destination)
    .bind(&details.immediate_destination_name)
    .bind(&details.immediate_origin)
    .bind(&details.immediate_origin_name)
    .bind(&details.company_name)
    .bind(&details.company_id)
    .bind(&details.entry_description)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(details)
}

// ------------------------------------------------------------------------------------------------
// Paying by ACH file
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchBillInput {
    pub bill_id: i64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchPaymentInput {
    /// Chosen once per Pay click; paying again with it returns the same file.
    pub request_id: String,
    pub payment_account_id: i64,
    /// The date the payments are recorded on.
    pub payment_date: NaiveDate,
    /// The date the vendors should be paid (the bank's settlement date), usually a business day ahead.
    pub effective_date: NaiveDate,
    pub bills: Vec<AchBillInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AchEntryView {
    pub vendor_id: i64,
    pub vendor_name: String,
    pub sec_code: String,
    pub account_masked: String,
    pub amount_cents: i64,
    pub trace_number: String,
    pub bill_count: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AchFileView {
    pub id: i64,
    pub payment_account_id: i64,
    pub payment_account_name: String,
    pub payment_date: String,
    pub effective_date: String,
    pub created_on: String,
    pub file_id_modifier: String,
    pub total_cents: i64,
    /// Payments in the file since voided: it can no longer be saved again.
    pub voided_payments: i64,
    /// How many times the file has been written to disk.
    pub times_saved: i64,
    /// A name to suggest when saving it.
    #[sqlx(default)]
    pub file_name: String,
    #[sqlx(skip)]
    pub entries: Vec<AchEntryView>,
}

async fn file_view(conn: &mut SqliteConnection, id: i64) -> Result<AchFileView, String> {
    let mut view: AchFileView = sqlx::query_as(
        "SELECT f.id, f.payment_account_id, a.name AS payment_account_name, f.payment_date, f.effective_date, f.created_on,
                f.file_id_modifier, f.total_cents,
                (SELECT COUNT(*) FROM ach_entries e JOIN ach_entry_payments p ON p.ach_entry_id = e.id
                   JOIN bill_payments bp ON bp.id = p.bill_payment_id WHERE e.ach_file_id = f.id AND bp.is_voided = 1) AS voided_payments,
                (SELECT COUNT(*) FROM ledger_audit_log l WHERE l.action_type = 'ACH_FILE' AND l.target_entity_id = f.id) AS times_saved
         FROM ach_files f JOIN chart_of_accounts a ON a.id = f.payment_account_id WHERE f.id = ?1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("ACH_FILE_NOT_FOUND: there is no ACH file {id}"))?;
    view.file_name = format!("ACH-{}-{}.ach", view.created_on.replace('-', ""), view.file_id_modifier);
    view.entries = sqlx::query_as(
        "SELECT e.vendor_id, v.name AS vendor_name, e.sec_code, e.account_masked, e.amount_cents, e.trace_number,
                (SELECT COUNT(*) FROM ach_entry_payments p WHERE p.ach_entry_id = e.id) AS bill_count
         FROM ach_entries e JOIN vendors v ON v.id = e.vendor_id WHERE e.ach_file_id = ?1 ORDER BY e.trace_number",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(view)
}

pub async fn list_ach_files(pool: &SqlitePool) -> Result<Vec<AchFileView>, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM ach_files ORDER BY id DESC").fetch_all(&mut *conn).await.map_err(db_err)?;
    let mut files = Vec::with_capacity(ids.len());
    for id in ids {
        files.push(file_view(&mut conn, id).await?);
    }
    Ok(files)
}

struct Pending {
    vendor_id: i64,
    vendor_name: String,
    bank: BankRow,
    bills: Vec<(AchBillInput, Option<String>)>,
}

/// Pays the chosen bills from one bank account by ACH: records each bill payment (kind ACH) and the
/// file's entries, one per vendor, in one transaction. `now` is the file's creation time (local).
/// Write the file with [`save_ach_file`].
pub async fn pay_bills_by_ach(pool: &SqlitePool, input: AchPaymentInput, access: Access<'_>, now: NaiveDateTime) -> Result<AchFileView, String> {
    let request_id = input.request_id.trim();
    if !(8..=64).contains(&request_id.len()) {
        return Err("ACH_BAD_REQUEST: a request ID is 8 to 64 characters".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM ach_files WHERE request_id = ?1").bind(request_id).fetch_optional(&mut *tx).await.map_err(db_err)?;
    if let Some(id) = existing {
        let view = file_view(&mut *tx, id).await;
        let _ = tx.rollback().await;
        return view;
    }
    match write_ach_payments(&mut tx, &input, request_id, access, now).await {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            file_view(&mut *pool.acquire().await.map_err(db_err)?, id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_ach_payments(tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>, input: &AchPaymentInput, request_id: &str, access: Access<'_>, now: NaiveDateTime) -> Result<i64, String> {
    if input.bills.is_empty() {
        return Err("BILL_NOTHING_TO_PAY: select at least one bill".into());
    }
    if !access.can_see() {
        return Err(LOCKED.into());
    }
    if input.effective_date < input.payment_date {
        return Err("ACH_BAD_EFFECTIVE_DATE: the date vendors are paid cannot be before the payment date".into());
    }
    let originator: OriginatorDetails = sqlx::query_as(&format!("SELECT {ORIGINATOR_COLUMNS} FROM ach_originators WHERE account_id = ?1"))
        .bind(input.payment_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("ACH_NO_ORIGINATOR: set up this bank account's ACH details first")?;

    // Group the bills by vendor; every vendor needs bank details.
    let mut vendors: BTreeMap<String, Pending> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for bill in &input.bills {
        if !seen.insert(bill.bill_id) {
            return Err("ACH_DUPLICATE_BILL: a bill can be paid only once in a file".into());
        }
        let row: Option<(i64, String, Option<String>)> =
            sqlx::query_as("SELECT v.id, v.name, b.bill_number FROM bills b JOIN vendors v ON v.id = b.vendor_id WHERE b.id = ?1")
                .bind(bill.bill_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        let (vendor_id, vendor_name, bill_number) = row.ok_or_else(|| format!("BILL_NOT_FOUND: bill {} does not exist", bill.bill_id))?;
        if !vendors.contains_key(&vendor_name) {
            let bank: Option<BankRow> = sqlx::query_as(
                "SELECT vendor_id, routing_number, routing_masked, account_number, account_masked, account_type, sec_code FROM vendor_bank_accounts WHERE vendor_id = ?1",
            )
            .bind(vendor_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let bank = bank.ok_or_else(|| format!("ACH_NO_BANK_DETAILS: {vendor_name} has no bank details; add them on the vendor, or pay this bill another way"))?;
            vendors.insert(vendor_name.clone(), Pending { vendor_id, vendor_name: vendor_name.clone(), bank, bills: vec![] });
        }
        vendors.get_mut(&vendor_name).expect("inserted").bills.push((bill.clone(), bill_number));
    }

    // Business (CCD) entries first, then people (PPD), each by vendor name: the order traces ascend in.
    let mut ordered: Vec<Pending> = vendors.into_values().collect();
    ordered.sort_by(|a, b| (a.bank.sec_code.as_str(), a.vendor_name.as_str()).cmp(&(b.bank.sec_code.as_str(), b.vendor_name.as_str())));

    let odfi = originator.nacha().odfi().to_string();
    let last: Option<i64> = sqlx::query_scalar("SELECT MAX(CAST(substr(trace_number, 9) AS INTEGER)) FROM ach_entries WHERE substr(trace_number, 1, 8) = ?1")
        .bind(&odfi)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    let mut sequence = last.unwrap_or(0);
    if sequence + ordered.len() as i64 > 9_999_999 {
        return Err("ACH_TRACE_EXHAUSTED: this bank's trace numbers have run out".into());
    }
    let created_on = now.date().format("%Y-%m-%d").to_string();
    let same_day: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ach_files WHERE immediate_origin = ?1 AND created_on = ?2")
        .bind(&originator.immediate_origin)
        .bind(&created_on)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    let modifier = MODIFIERS.chars().nth(same_day as usize).ok_or("ACH_TOO_MANY_FILES: banks accept at most 36 files a day from one company; try again tomorrow")?;

    let total: i64 = input.bills.iter().map(|b| b.amount_cents).sum();
    let file_id: i64 = sqlx::query_scalar(
        "INSERT INTO ach_files (request_id, payment_account_id, payment_date, effective_date, created_on, created_time, file_id_modifier,
                                immediate_destination, immediate_destination_name, immediate_origin, immediate_origin_name,
                                company_name, company_id, entry_description, total_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) RETURNING id",
    )
    .bind(request_id)
    .bind(input.payment_account_id)
    .bind(input.payment_date)
    .bind(input.effective_date)
    .bind(&created_on)
    .bind(now.format("%H%M").to_string())
    .bind(modifier.to_string())
    .bind(&originator.immediate_destination)
    .bind(&originator.immediate_destination_name)
    .bind(&originator.immediate_origin)
    .bind(&originator.immediate_origin_name)
    .bind(&originator.company_name)
    .bind(&originator.company_id)
    .bind(&originator.entry_description)
    .bind(total)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        let msg = db_err(e);
        if msg.contains("total_cents") { "BILL_BAD_AMOUNT: payment amount must be greater than zero".into() } else { msg }
    })?;

    for vendor in ordered {
        sequence += 1;
        let trace = format!("{odfi}{sequence:07}");
        let amount: i64 = vendor.bills.iter().map(|(b, _)| b.amount_cents).sum();
        if amount > nacha::MAX_ENTRY_CENTS {
            return Err(format!("ACH_BAD_AMOUNT: one ACH payment to {} cannot exceed $99,999,999.99", vendor.vendor_name));
        }
        let bank = &vendor.bank;
        let routing = open(access, BANK, "routing_number", bank.vendor_id, &bank.routing_number)?.ok_or(LOCKED)?;
        let account = open(access, BANK, "account_number", bank.vendor_id, &bank.account_number)?.ok_or(LOCKED)?;
        let numbers: Vec<&str> = vendor.bills.iter().filter_map(|(_, n)| n.as_deref().map(str::trim).filter(|n| !n.is_empty())).collect();
        let addenda = (!numbers.is_empty()).then(|| format!("{} {}", if numbers.len() == 1 { "BILL" } else { "BILLS" }, numbers.join(", ")));
        let entry_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) + 1 FROM ach_entries").fetch_one(&mut **tx).await.map_err(db_err)?;
        sqlx::query(
            "INSERT INTO ach_entries (id, ach_file_id, vendor_id, sec_code, account_type, routing_number, account_number, account_masked,
                                      receiver_name, amount_cents, trace_number, addenda)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(entry_id)
        .bind(file_id)
        .bind(vendor.vendor_id)
        .bind(&bank.sec_code)
        .bind(&bank.account_type)
        .bind(seal(access, "ach_entries", "routing_number", entry_id, &routing)?)
        .bind(seal(access, "ach_entries", "account_number", entry_id, &account)?)
        .bind(&bank.account_masked)
        .bind(&vendor.vendor_name)
        .bind(amount)
        .bind(&trace)
        .bind(addenda)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        for (bill, _) in &vendor.bills {
            let payment = BillPaymentInput {
                bill_id: bill.bill_id,
                payment_account_id: input.payment_account_id,
                payment_date: input.payment_date,
                amount_cents: bill.amount_cents,
                reference: Some(format!("ACH {trace}")),
            };
            let payment_id = crate::expenses::write_bill_payment(tx, &payment, "ACH").await?;
            sqlx::query("INSERT INTO ach_entry_payments (bill_payment_id, ach_entry_id) VALUES (?1, ?2)")
                .bind(payment_id)
                .bind(entry_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
    }
    Ok(file_id)
}

#[derive(sqlx::FromRow)]
struct FileRow {
    effective_date: String,
    created_on: String,
    created_time: String,
    file_id_modifier: String,
    immediate_destination: String,
    immediate_destination_name: String,
    immediate_origin: String,
    immediate_origin_name: String,
    company_name: String,
    company_id: String,
    entry_description: String,
    total_cents: i64,
}

#[derive(sqlx::FromRow)]
struct EntryRow {
    id: i64,
    vendor_id: i64,
    sec_code: String,
    account_type: String,
    routing_number: String,
    account_number: String,
    receiver_name: String,
    amount_cents: i64,
    trace_number: String,
    addenda: Option<String>,
}

/// The NACHA text of ACH file `id`, rebuilt from what was recorded, and checked by reading it back:
/// it must validate, and its totals and entries must be the recorded ones.
pub async fn render_ach_file(conn: &mut SqliteConnection, id: i64, access: Access<'_>) -> Result<(String, nacha::Summary), String> {
    if !access.can_see() {
        return Err(LOCKED.into());
    }
    let view = file_view(conn, id).await?;
    if view.voided_payments > 0 {
        return Err("ACH_FILE_HAS_VOIDS: a payment in this file was voided, so the file would pay it anyway; pay what is left in a new file".into());
    }
    let f: FileRow = sqlx::query_as(
        "SELECT effective_date, created_on, created_time, file_id_modifier, immediate_destination, immediate_destination_name, immediate_origin,
                immediate_origin_name, company_name, company_id, entry_description, total_cents FROM ach_files WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(db_err)?;
    let rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT id, vendor_id, sec_code, account_type, routing_number, account_number, receiver_name, amount_cents, trace_number, addenda
         FROM ach_entries WHERE ach_file_id = ?1 ORDER BY trace_number",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let mut batches = vec![nacha::Batch { sec_code: SecCode::Ccd, entries: vec![] }, nacha::Batch { sec_code: SecCode::Ppd, entries: vec![] }];
    for r in &rows {
        if r.routing_number.is_empty() || r.account_number.is_empty() {
            return Err("ACH_FILE_ERASED: this file's bank details were erased when all users were removed; it cannot be saved again".into());
        }
        let entry = nacha::Entry {
            routing: open(access, "ach_entries", "routing_number", r.id, &r.routing_number)?.ok_or(LOCKED)?,
            account: open(access, "ach_entries", "account_number", r.id, &r.account_number)?.ok_or(LOCKED)?,
            account_type: AccountType::parse(&r.account_type).ok_or("ACH_BAD_TYPE: unknown account type")?,
            amount_cents: r.amount_cents,
            individual_id: format!("V{}", r.vendor_id),
            name: r.receiver_name.clone(),
            trace_number: r.trace_number.clone(),
            addenda: r.addenda.clone(),
        };
        batches[if r.sec_code == "PPD" { 1 } else { 0 }].entries.push(entry);
    }
    let date = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("ACH_BAD_DATE: {e}"));
    let time = chrono::NaiveTime::parse_from_str(&f.created_time, "%H%M").map_err(|e| format!("ACH_BAD_DATE: {e}"))?;
    let file = nacha::File {
        originator: nacha::Originator {
            immediate_destination: f.immediate_destination,
            immediate_destination_name: f.immediate_destination_name,
            immediate_origin: f.immediate_origin,
            immediate_origin_name: f.immediate_origin_name,
            company_name: f.company_name,
            company_id: f.company_id,
            entry_description: f.entry_description,
        },
        created: date(&f.created_on)?.and_time(time),
        file_id_modifier: f.file_id_modifier.chars().next().unwrap_or('A'),
        effective_date: date(&f.effective_date)?,
        batches,
    };
    let text = nacha::write(&file)?;
    let summary = nacha::validate(&text).map_err(|e| format!("ACH_FILE_INVALID: the file did not pass its own check, so it was not saved:\n{e}"))?;
    let recorded: Vec<(&str, i64)> = rows.iter().map(|r| (r.trace_number.as_str(), r.amount_cents)).collect();
    let read: Vec<(&str, i64)> = summary.entries.iter().map(|e| (e.trace_number.as_str(), e.amount_cents)).collect();
    if summary.total_credit_cents != f.total_cents || summary.total_debit_cents != 0 || read != recorded {
        return Err("ACH_FILE_INVALID: the file's entries do not match the recorded payments, so it was not saved".into());
    }
    Ok((text, summary))
}

/// Writes ACH file `id` to `path` (readable by its owner only) and records it in the audit trail.
pub async fn save_ach_file(pool: &SqlitePool, id: i64, path: &Path, access: Access<'_>) -> Result<AchFileView, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let (text, summary) = render_ach_file(&mut conn, id, access).await?;
    drop(conn);
    crate::instance::write_private(path, text.as_bytes()).map_err(|e| format!("ACH_FILE_WRITE: cannot write {}: {e}", path.display()))?;

    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let view = file_view(&mut *tx, id).await?;
        let entries: Vec<serde_json::Value> = view
            .entries
            .iter()
            .map(|e| serde_json::json!({ "vendorId": e.vendor_id, "amountCents": e.amount_cents, "traceNumber": e.trace_number, "secCode": e.sec_code }))
            .collect();
        let payload = serde_json::json!({
            "event": if view.times_saved == 0 { "generated" } else { "regenerated" },
            "achFileId": id,
            "paymentAccountId": view.payment_account_id,
            "effectiveDate": view.effective_date,
            "fileIdModifier": view.file_id_modifier,
            "batchCount": summary.batch_count,
            "entryAddendaCount": summary.entry_addenda_count,
            "entryHash": summary.entry_hash,
            "totalCreditCents": summary.total_credit_cents,
            "entries": entries,
        });
        sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('ACH_FILE', ?1, ?2)")
            .bind(id)
            .bind(payload.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok::<_, String>(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Err(err) => return Err(rollback(tx, err).await),
    }
    file_view(&mut *pool.acquire().await.map_err(db_err)?, id).await
}

#[cfg(test)]
mod tests;
