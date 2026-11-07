//! Sales reps and shipping methods, and the details a sales form carries with them: the rep who
//! made the sale, the ship date and how it shipped ("Ship Via"), and the customer custom fields
//! printed on the form (see `custom_fields`).
//!
//! Both lists are optional. A rep is a short set of initials (as QuickBooks shows it in the Rep
//! column) with an optional full name; a customer may carry a default rep. A rep or shipping
//! method that is used anywhere can be renamed or made inactive, but not deleted. Inactive entries
//! stay on what already uses them and cannot be chosen again.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::custom_fields::{self, FieldValue};
use crate::ledger::db_err;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesRepView {
    pub id: i64,
    pub initials: String,
    pub name: Option<String>,
    pub is_active: bool,
    /// On a customer or a sales document, so it can no longer be deleted.
    pub is_used: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesRepInput {
    pub initials: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ShipMethodView {
    pub id: i64,
    pub name: String,
    pub is_active: bool,
    pub is_used: bool,
}

const REP_USED: &str = "EXISTS (SELECT 1 FROM customers WHERE sales_rep_id = r.id) OR EXISTS (SELECT 1 FROM invoices WHERE sales_rep_id = r.id)
    OR EXISTS (SELECT 1 FROM sales_receipts WHERE sales_rep_id = r.id) OR EXISTS (SELECT 1 FROM estimates WHERE sales_rep_id = r.id)";
const SHIP_USED: &str = "EXISTS (SELECT 1 FROM invoices WHERE ship_method_id = s.id)
    OR EXISTS (SELECT 1 FROM sales_receipts WHERE ship_method_id = s.id) OR EXISTS (SELECT 1 FROM estimates WHERE ship_method_id = s.id)";

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

fn unique_err(err: sqlx::Error, message: String) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        message
    } else {
        msg
    }
}

// ------------------------------------------------------------------------------------------------
// Sales reps
// ------------------------------------------------------------------------------------------------

pub async fn list_sales_reps(pool: &SqlitePool) -> Result<Vec<SalesRepView>, String> {
    sqlx::query_as::<_, SalesRepView>(&format!(
        "SELECT r.id, r.initials, r.name, r.is_active, ({REP_USED}) AS is_used FROM sales_reps r ORDER BY r.initials COLLATE NOCASE"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_sales_rep(pool: &SqlitePool, id: i64) -> Result<SalesRepView, String> {
    list_sales_reps(pool).await?.into_iter().find(|r| r.id == id).ok_or_else(|| format!("REP_NOT_FOUND: sales rep {id} does not exist"))
}

fn rep_fields(input: &SalesRepInput) -> Result<(String, Option<String>), String> {
    let initials = input.initials.trim();
    if initials.is_empty() {
        return Err("REP_INITIALS_REQUIRED: enter the rep's initials".into());
    }
    if initials.chars().count() > 5 {
        return Err("REP_INITIALS_TOO_LONG: initials are at most 5 characters".into());
    }
    let name = clean(input.name.as_deref());
    if name.is_some_and(|n| n.chars().count() > 41) {
        return Err("REP_NAME_TOO_LONG: a rep's name is at most 41 characters".into());
    }
    Ok((initials.to_string(), name.map(str::to_string)))
}

pub async fn create_sales_rep(pool: &SqlitePool, input: SalesRepInput) -> Result<SalesRepView, String> {
    let (initials, name) = rep_fields(&input)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO sales_reps (initials, name) VALUES (?1, ?2) RETURNING id")
        .bind(&initials)
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| unique_err(e, format!("REP_DUPLICATE: there is already a rep with initials '{initials}'")))?;
    get_sales_rep(pool, id).await
}

pub async fn update_sales_rep(pool: &SqlitePool, id: i64, input: SalesRepInput) -> Result<SalesRepView, String> {
    let (initials, name) = rep_fields(&input)?;
    let done = sqlx::query("UPDATE sales_reps SET initials = ?1, name = ?2 WHERE id = ?3")
        .bind(&initials)
        .bind(name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| unique_err(e, format!("REP_DUPLICATE: there is already a rep with initials '{initials}'")))?;
    if done.rows_affected() == 0 {
        return Err(format!("REP_NOT_FOUND: sales rep {id} does not exist"));
    }
    get_sales_rep(pool, id).await
}

pub async fn set_sales_rep_active(pool: &SqlitePool, id: i64, active: bool) -> Result<SalesRepView, String> {
    sqlx::query("UPDATE sales_reps SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_sales_rep(pool, id).await
}

/// Deletes a rep that no customer or document uses.
pub async fn delete_sales_rep(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let rep = get_sales_rep(pool, id).await?;
    if rep.is_used {
        return Err(format!("REP_IN_USE: '{}' is on customers or sales forms; make it inactive instead", rep.initials));
    }
    sqlx::query("DELETE FROM sales_reps WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Shipping methods
// ------------------------------------------------------------------------------------------------

pub async fn list_ship_methods(pool: &SqlitePool) -> Result<Vec<ShipMethodView>, String> {
    sqlx::query_as::<_, ShipMethodView>(&format!("SELECT s.id, s.name, s.is_active, ({SHIP_USED}) AS is_used FROM ship_methods s ORDER BY s.name COLLATE NOCASE"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

async fn get_ship_method(pool: &SqlitePool, id: i64) -> Result<ShipMethodView, String> {
    list_ship_methods(pool).await?.into_iter().find(|s| s.id == id).ok_or_else(|| format!("SHIP_NOT_FOUND: shipping method {id} does not exist"))
}

fn ship_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("SHIP_NAME_REQUIRED: enter a shipping method".into());
    }
    if name.chars().count() > 31 {
        return Err("SHIP_NAME_TOO_LONG: a shipping method is at most 31 characters".into());
    }
    Ok(name.to_string())
}

pub async fn create_ship_method(pool: &SqlitePool, name: &str) -> Result<ShipMethodView, String> {
    let name = ship_name(name)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO ship_methods (name) VALUES (?1) RETURNING id")
        .bind(&name)
        .fetch_one(pool)
        .await
        .map_err(|e| unique_err(e, format!("SHIP_DUPLICATE: there is already a shipping method named '{name}'")))?;
    get_ship_method(pool, id).await
}

pub async fn rename_ship_method(pool: &SqlitePool, id: i64, name: &str) -> Result<ShipMethodView, String> {
    let name = ship_name(name)?;
    let done = sqlx::query("UPDATE ship_methods SET name = ?1 WHERE id = ?2")
        .bind(&name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| unique_err(e, format!("SHIP_DUPLICATE: there is already a shipping method named '{name}'")))?;
    if done.rows_affected() == 0 {
        return Err(format!("SHIP_NOT_FOUND: shipping method {id} does not exist"));
    }
    get_ship_method(pool, id).await
}

pub async fn set_ship_method_active(pool: &SqlitePool, id: i64, active: bool) -> Result<ShipMethodView, String> {
    sqlx::query("UPDATE ship_methods SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_ship_method(pool, id).await
}

pub async fn delete_ship_method(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let method = get_ship_method(pool, id).await?;
    if method.is_used {
        return Err(format!("SHIP_IN_USE: '{}' is on sales forms; make it inactive instead", method.name));
    }
    sqlx::query("DELETE FROM ship_methods WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Finding or creating by name (imports)
// ------------------------------------------------------------------------------------------------

/// The rep with these initials, created (with `name`) when there is none. A QuickBooks rep may be
/// written "initials" or "initials:name".
pub(crate) async fn find_or_create_rep(tx: &mut Transaction<'_, Sqlite>, rep: &str) -> Result<Option<i64>, String> {
    let (initials, name) = match rep.split_once(':') {
        Some((i, n)) => (i.trim(), clean(n.split(':').next())),
        None => (rep.trim(), None),
    };
    if initials.is_empty() {
        return Ok(None);
    }
    let initials: String = initials.chars().take(5).collect::<String>().trim().to_string();
    sqlx::query("INSERT OR IGNORE INTO sales_reps (initials, name) VALUES (?1, ?2)").bind(&initials).bind(name).execute(&mut **tx).await.map_err(db_err)?;
    sqlx::query_scalar("SELECT id FROM sales_reps WHERE initials = ?1 COLLATE NOCASE").bind(&initials).fetch_optional(&mut **tx).await.map_err(db_err)
}

pub(crate) async fn find_or_create_ship_method(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<Option<i64>, String> {
    let name: String = name.trim().chars().take(31).collect::<String>().trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }
    sqlx::query("INSERT OR IGNORE INTO ship_methods (name) VALUES (?1)").bind(&name).execute(&mut **tx).await.map_err(db_err)?;
    sqlx::query_scalar("SELECT id FROM ship_methods WHERE name = ?1 COLLATE NOCASE").bind(&name).fetch_optional(&mut **tx).await.map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// The details on a sales document
// ------------------------------------------------------------------------------------------------

/// A sales document kind that carries a rep, ship details and printed custom fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalesDoc {
    Invoice,
    SalesReceipt,
    Estimate,
}

impl SalesDoc {
    fn table(self) -> &'static str {
        match self {
            SalesDoc::Invoice => "invoices",
            SalesDoc::SalesReceipt => "sales_receipts",
            SalesDoc::Estimate => "estimates",
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            SalesDoc::Invoice => "INVOICE",
            SalesDoc::SalesReceipt => "SALES_RECEIPT",
            SalesDoc::Estimate => "ESTIMATE",
        }
    }

    fn code(self) -> &'static str {
        match self {
            SalesDoc::Invoice => "INV",
            SalesDoc::SalesReceipt => "SR",
            SalesDoc::Estimate => "EST",
        }
    }
}

/// The optional details entered on an invoice, sales receipt or estimate. Flattened into each
/// form's input, so a caller that leaves them out gets none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SalesDetails {
    pub sales_rep_id: Option<i64>,
    pub ship_date: Option<NaiveDate>,
    pub ship_method_id: Option<i64>,
    /// Customer fields shown on sales forms, as entered on this document. Fields left out or blank
    /// are not printed.
    pub custom_fields: Vec<FieldValue>,
}

impl SalesDetails {
    pub fn is_empty(&self) -> bool {
        self.sales_rep_id.is_none() && self.ship_date.is_none() && self.ship_method_id.is_none() && self.custom_fields.iter().all(|f| f.value.trim().is_empty())
    }
}

/// The details as a form shows and prints them.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesDetailsView {
    pub sales_rep_id: Option<i64>,
    pub sales_rep_initials: Option<String>,
    pub sales_rep_name: Option<String>,
    pub ship_date: Option<String>,
    pub ship_method_id: Option<i64>,
    pub ship_method_name: Option<String>,
    /// Customer fields printed on this document, in the list's order, with their labels.
    pub custom_fields: Vec<custom_fields::LabeledValue>,
}

async fn check_active(tx: &mut Transaction<'_, Sqlite>, table: &str, id: i64, what: &str, code: &str) -> Result<(), String> {
    let active: Option<bool> =
        sqlx::query_scalar(&format!("SELECT is_active FROM {table} WHERE id = ?1")).bind(id).fetch_optional(&mut **tx).await.map_err(db_err)?;
    match active {
        None => Err(format!("{code}_UNKNOWN_{}: that {what} does not exist", what.to_ascii_uppercase().replace(' ', "_"))),
        Some(false) => Err(format!("{code}_INACTIVE_{}: that {what} is inactive", what.to_ascii_uppercase().replace(' ', "_"))),
        Some(true) => Ok(()),
    }
}

/// Writes a new document's details, after its row is inserted and before it posts. A rep and
/// shipping method chosen for it must be active (a ship date may be later or earlier than the
/// document's date, as QuickBooks allows).
pub(crate) async fn write_details(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, details: &SalesDetails) -> Result<(), String> {
    write_details_carrying(tx, doc, id, details, &SalesDetails::default()).await
}

/// Like [`write_details`], for a document that takes over another one's details: an estimate
/// becoming an invoice, or an invoice replaced by its edited version. Whatever is the same as on
/// `carried` comes over as it was, even when its rep or shipping method has since been made
/// inactive or its custom field taken off sales forms, so a later list change cannot block it.
/// Anything newly chosen is checked as for a new form.
pub(crate) async fn write_details_carrying(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, details: &SalesDetails, carried: &SalesDetails) -> Result<(), String> {
    if details.is_empty() {
        return Ok(());
    }
    let code = doc.code();
    if let Some(rep) = details.sales_rep_id.filter(|r| carried.sales_rep_id != Some(*r)) {
        check_active(tx, "sales_reps", rep, "sales rep", code).await?;
    }
    if let Some(method) = details.ship_method_id.filter(|m| carried.ship_method_id != Some(*m)) {
        check_active(tx, "ship_methods", method, "shipping method", code).await?;
    }
    sqlx::query(&format!("UPDATE {} SET sales_rep_id = ?1, ship_date = ?2, ship_method_id = ?3 WHERE id = ?4", doc.table()))
        .bind(details.sales_rep_id)
        .bind(details.ship_date)
        .bind(details.ship_method_id)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    let carried_fields: Vec<i64> = carried.custom_fields.iter().map(|f| f.field_id).collect();
    custom_fields::write_document_fields(tx, doc, id, &details.custom_fields, &carried_fields).await
}

/// An imported document's rep and shipping details, without the checks a new form gets: history
/// keeps the rep it was sold by even when that rep is inactive now.
pub(crate) async fn write_imported(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, details: &SalesDetails) -> Result<(), String> {
    sqlx::query(&format!("UPDATE {} SET sales_rep_id = ?1, ship_date = ?2, ship_method_id = ?3 WHERE id = ?4", doc.table()))
        .bind(details.sales_rep_id)
        .bind(details.ship_date)
        .bind(details.ship_method_id)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// A document's details as entered, for loading it back into its form (an edit, or an estimate
/// becoming an invoice) and printing it.
pub async fn document_details(pool: &SqlitePool, doc: SalesDoc, id: i64) -> Result<SalesDetailsView, String> {
    let row: Option<(Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>)> = sqlx::query_as(&format!(
        "SELECT d.sales_rep_id, r.initials, r.name, d.ship_date, d.ship_method_id, s.name
         FROM {} d LEFT JOIN sales_reps r ON r.id = d.sales_rep_id LEFT JOIN ship_methods s ON s.id = d.ship_method_id
         WHERE d.id = ?1",
        doc.table()
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((sales_rep_id, sales_rep_initials, sales_rep_name, ship_date, ship_method_id, ship_method_name)) = row else {
        return Ok(SalesDetailsView::default());
    };
    Ok(SalesDetailsView {
        sales_rep_id,
        sales_rep_initials,
        sales_rep_name,
        ship_date,
        ship_method_id,
        ship_method_name,
        custom_fields: custom_fields::document_fields(pool, doc, id).await?,
    })
}

/// A document's details inside a write, to carry them onto another document.
pub(crate) async fn details_in_tx(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64) -> Result<SalesDetails, String> {
    let (sales_rep_id, ship_date, ship_method_id): (Option<i64>, Option<NaiveDate>, Option<i64>) =
        sqlx::query_as(&format!("SELECT sales_rep_id, ship_date, ship_method_id FROM {} WHERE id = ?1", doc.table()))
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
    let custom_fields: Vec<(i64, String)> = sqlx::query_as("SELECT field_id, value FROM sales_document_fields WHERE document_kind = ?1 AND document_id = ?2")
        .bind(doc.key())
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(SalesDetails {
        sales_rep_id,
        ship_date,
        ship_method_id,
        custom_fields: custom_fields.into_iter().map(|(field_id, value)| FieldValue { field_id, value }).collect(),
    })
}

impl SalesDetailsView {
    /// The same details as an input, to carry them onto another document.
    pub fn to_input(&self) -> SalesDetails {
        SalesDetails {
            sales_rep_id: self.sales_rep_id,
            ship_date: self.ship_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
            ship_method_id: self.ship_method_id,
            custom_fields: self.custom_fields.iter().map(|f| FieldValue { field_id: f.field_id, value: f.value.clone() }).collect(),
        }
    }
}

#[cfg(test)]
mod tests;
