//! Contacts on a customer or vendor: any number, each with a role, in the order the record shows
//! them (gap T51; migration 0104).
//!
//! One of a customer's contacts can be the default for invoices: its name is printed in Bill To on
//! new sales forms, and its email (when it has one) receives emailed invoices, estimates, sales
//! receipts and credit memos. Another (or the same) can be the default for statements. A vendor's
//! default (`for_invoices`) is their main contact.
//!
//! `customers.contact_name` and `alt_contact_name` are kept as the invoice contact (or the first
//! one) and the next one, so what reads them carries on unchanged.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, rollback};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: i64,
    pub name: String,
    pub role: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub mobile: Option<String>,
    pub for_invoices: bool,
    pub for_statements: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    /// An existing contact of this customer or vendor; `None` for a new one.
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub mobile: Option<String>,
    #[serde(default)]
    pub for_invoices: bool,
    #[serde(default)]
    pub for_statements: bool,
}

/// The column that names the record a contact belongs to.
fn owner(kind: &str) -> Result<&'static str, String> {
    match kind {
        "customer" => Ok("customer_id"),
        "vendor" => Ok("vendor_id"),
        other => Err(format!("CONTACT_BAD_KIND: unknown list '{other}'")),
    }
}

fn clean(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn too_long(value: &Option<String>, max: usize, what: &str) -> Result<(), String> {
    if value.as_ref().is_some_and(|v| v.chars().count() > max) {
        return Err(format!("CONTACT_TOO_LONG: a contact's {what} is at most {max} characters"));
    }
    Ok(())
}

pub async fn list_contacts(pool: &SqlitePool, kind: &str, party_id: i64) -> Result<Vec<Contact>, String> {
    let column = owner(kind)?;
    sqlx::query_as::<_, Contact>(&format!(
        "SELECT id, name, role, email, phone, mobile, for_invoices, for_statements FROM party_contacts WHERE {column} = ?1 ORDER BY position, id"
    ))
    .bind(party_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Saves a customer's or vendor's whole contact list, in order: contacts with an `id` are changed,
/// those without are added, and contacts left out are removed. Only what changed is written, so the
/// audit trail records each real change once.
pub async fn save_contacts(pool: &SqlitePool, kind: &str, party_id: i64, contacts: &[ContactInput]) -> Result<Vec<Contact>, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if let Err(err) = write_contacts(&mut tx, kind, party_id, contacts).await {
        return Err(rollback(tx, err).await);
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    list_contacts(pool, kind, party_id).await
}

pub(crate) async fn write_contacts(tx: &mut Transaction<'_, Sqlite>, kind: &str, party_id: i64, contacts: &[ContactInput]) -> Result<(), String> {
    let column = owner(kind)?;
    let table = if kind == "customer" { "customers" } else { "vendors" };
    let exists: bool = sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"))
        .bind(party_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    if !exists {
        return Err(format!("PARTY_NOT_FOUND: that {kind} does not exist"));
    }
    if contacts.iter().filter(|c| c.for_invoices).count() > 1 {
        return Err(format!("CONTACT_TWO_DEFAULTS: choose one {} contact", if kind == "customer" { "invoice" } else { "main" }));
    }
    if contacts.iter().filter(|c| c.for_statements).count() > 1 {
        return Err("CONTACT_TWO_DEFAULTS: choose one statement contact".into());
    }
    if kind == "vendor" && contacts.iter().any(|c| c.for_statements) {
        return Err("CONTACT_VENDOR_STATEMENTS: vendors have no statements".into());
    }
    let current: Vec<(i64,)> = sqlx::query_as(&format!("SELECT id FROM party_contacts WHERE {column} = ?1"))
        .bind(party_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
    let current: Vec<i64> = current.into_iter().map(|(id,)| id).collect();
    for contact in contacts {
        if let Some(id) = contact.id {
            if !current.contains(&id) {
                return Err(format!("CONTACT_NOT_FOUND: contact {id} is not one of this {kind}'s"));
            }
        }
    }
    // Removed first, then defaults cleared, so the one-default-per-use indexes never see two.
    for id in current.iter().filter(|id| !contacts.iter().any(|c| c.id == Some(**id))) {
        sqlx::query("DELETE FROM party_contacts WHERE id = ?1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
    }
    // A default that moves to another contact is taken off the old one first; one that stays put
    // is left alone (no audit row for it).
    for flag in ["for_invoices", "for_statements"] {
        let keeps = contacts.iter().find(|c| if flag == "for_invoices" { c.for_invoices } else { c.for_statements }).and_then(|c| c.id);
        sqlx::query(&format!("UPDATE party_contacts SET {flag} = 0 WHERE {column} = ?1 AND {flag} = 1 AND id IS NOT ?2"))
            .bind(party_id)
            .bind(keeps)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    let insert = format!(
        "INSERT INTO party_contacts ({column}, position, name, role, email, phone, mobile, for_invoices, for_statements)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
    );
    for (index, contact) in contacts.iter().enumerate() {
        let position = i64::try_from(index + 1).map_err(|_| "CONTACT_TOO_MANY: too many contacts".to_string())?;
        let name = clean(Some(&contact.name)).ok_or_else(|| format!("CONTACT_NO_NAME: contact {position} needs a name"))?;
        let (role, email, phone, mobile) =
            (clean(contact.role.as_deref()), clean(contact.email.as_deref()), clean(contact.phone.as_deref()), clean(contact.mobile.as_deref()));
        too_long(&Some(name.clone()), 100, "name")?;
        too_long(&role, 50, "role")?;
        too_long(&email, 254, "email")?;
        too_long(&phone, 40, "phone")?;
        too_long(&mobile, 40, "mobile")?;
        if email.as_ref().is_some_and(|e| !e.contains('@')) {
            return Err(format!("CONTACT_BAD_EMAIL: {name}'s email doesn't look like an email address"));
        }
        let query = match contact.id {
            Some(id) => sqlx::query(
                "UPDATE party_contacts SET position = ?2, name = ?3, role = ?4, email = ?5, phone = ?6, mobile = ?7, for_invoices = ?8, for_statements = ?9
                 WHERE id = ?1",
            )
            .bind(id),
            None => sqlx::query(&insert).bind(party_id),
        };
        query
            .bind(position)
            .bind(&name)
            .bind(role)
            .bind(email)
            .bind(phone)
            .bind(mobile)
            .bind(contact.for_invoices)
            .bind(contact.for_statements)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    if kind == "customer" {
        sync_customer(tx, party_id).await?;
    }
    Ok(())
}

/// Keeps `customers.contact_name` and `alt_contact_name` as the invoice contact (or the first one)
/// and the next one. Writes only when they change, so the audit trail stays quiet otherwise.
async fn sync_customer(tx: &mut Transaction<'_, Sqlite>, customer_id: i64) -> Result<(), String> {
    sqlx::query(
        "WITH ordered AS (
             SELECT name, ROW_NUMBER() OVER (ORDER BY for_invoices DESC, position, id) AS n FROM party_contacts WHERE customer_id = ?1
         )
         UPDATE customers SET contact_name = (SELECT name FROM ordered WHERE n = 1), alt_contact_name = (SELECT name FROM ordered WHERE n = 2)
         WHERE id = ?1
           AND (contact_name IS NOT (SELECT name FROM ordered WHERE n = 1) OR alt_contact_name IS NOT (SELECT name FROM ordered WHERE n = 2))",
    )
    .bind(customer_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Copies a customer's or vendor's contacts to a copy of the record (Duplicate on the lists).
pub(crate) async fn copy_contacts(tx: &mut Transaction<'_, Sqlite>, kind: &str, from: i64, to: i64) -> Result<(), String> {
    let column = owner(kind)?;
    sqlx::query(&format!(
        "INSERT INTO party_contacts ({column}, position, name, role, email, phone, mobile, for_invoices, for_statements)
         SELECT ?2, position, name, role, email, phone, mobile, for_invoices, for_statements FROM party_contacts WHERE {column} = ?1 ORDER BY position"
    ))
    .bind(from)
    .bind(to)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// The email a customer's form goes to: the default contact's for that use when it has one, else
/// the customer's own. `use_` is `invoices` or `statements`. SQL, for the queries that list what
/// to send; `c` is the customers row.
pub(crate) fn recipient_sql(use_: &str) -> String {
    let flag = if use_ == "statements" { "for_statements" } else { "for_invoices" };
    format!("COALESCE((SELECT pc.email FROM party_contacts pc WHERE pc.customer_id = c.id AND pc.{flag} = 1 AND pc.email IS NOT NULL), c.email)")
}

/// The statement contact's name, printed on the statement under the customer's name.
pub(crate) async fn statement_contact(pool: &SqlitePool, customer_id: i64) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT name FROM party_contacts WHERE customer_id = ?1 AND for_statements = 1")
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests;
