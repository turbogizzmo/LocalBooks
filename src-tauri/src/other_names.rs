//! The Other Names list (gap T56; migration 0109): people and businesses who are neither customers
//! nor vendors (an owner taking a draw, a one-off payee). A check or expense can be paid to one,
//! and the list has the information panel the customer and vendor lists have.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, rollback};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OtherName {
    pub id: i64,
    pub name: String,
    pub company_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub is_active: bool,
    /// Live checks and expenses paid to them, and their home-currency total.
    pub payment_count: i64,
    pub paid_cents: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherNameInput {
    pub name: String,
    #[serde(default)]
    pub company_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

const SELECT: &str = "SELECT o.id, o.name, o.company_name, o.email, o.phone, o.address, o.notes, o.is_active,
        (SELECT COUNT(*) FROM expenses x JOIN journal_entries e ON e.id = x.journal_entry_id WHERE x.other_name_id = o.id AND e.is_voided = 0) AS payment_count,
        COALESCE((SELECT SUM(CASE WHEN x.is_credit = 1 THEN -1 ELSE 1 END * COALESCE(x.home_total_cents, x.total_cents))
                  FROM expenses x JOIN journal_entries e ON e.id = x.journal_entry_id WHERE x.other_name_id = o.id AND e.is_voided = 0), 0) AS paid_cents
 FROM other_names o";

fn clean(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<OtherName>, String> {
    sqlx::query_as::<_, OtherName>(sqlx::AssertSqlSafe(format!("{SELECT} ORDER BY o.name COLLATE NOCASE"))).fetch_all(pool).await.map_err(db_err)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<OtherName, String> {
    sqlx::query_as::<_, OtherName>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE o.id = ?1")))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("OTHER_NOT_FOUND: other name {id} does not exist"))
}

/// A name a customer or vendor already has can't also be an other name: a payee typed on a check
/// must find one record.
async fn check_name(pool: &SqlitePool, name: &str, id: Option<i64>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("OTHER_NO_NAME: enter a name".into());
    }
    if name.contains(':') {
        return Err("OTHER_BAD_NAME: an other name can't contain a colon".into());
    }
    if name.chars().count() > 100 {
        return Err("OTHER_TOO_LONG: a name is at most 100 characters".into());
    }
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT 'a customer' FROM customers WHERE name = ?1 COLLATE NOCASE
         UNION ALL SELECT 'a vendor' FROM vendors WHERE name = ?1 COLLATE NOCASE
         UNION ALL SELECT 'another other name' FROM other_names WHERE name = ?1 COLLATE NOCASE AND id IS NOT ?2
         LIMIT 1",
    )
    .bind(name)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match taken {
        Some(who) => Err(format!("OTHER_DUPLICATE: {who} is already named '{name}'")),
        None => Ok(name.to_string()),
    }
}

async fn write(pool: &SqlitePool, id: Option<i64>, input: &OtherNameInput) -> Result<i64, String> {
    let name = check_name(pool, &input.name, id).await?;
    let email = clean(input.email.as_deref());
    if email.as_ref().is_some_and(|e| !e.contains('@')) {
        return Err("OTHER_BAD_EMAIL: that doesn't look like an email address".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let fields = (clean(input.company_name.as_deref()), email, clean(input.phone.as_deref()), clean(input.address.as_deref()), clean(input.notes.as_deref()));
    let result = match id {
        None => sqlx::query_scalar::<_, i64>(
            "INSERT INTO other_names (name, company_name, email, phone, address, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
        )
        .bind(&name)
        .bind(&fields.0)
        .bind(&fields.1)
        .bind(&fields.2)
        .bind(&fields.3)
        .bind(&fields.4)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err),
        Some(id) => sqlx::query("UPDATE other_names SET name = ?2, company_name = ?3, email = ?4, phone = ?5, address = ?6, notes = ?7 WHERE id = ?1")
            .bind(id)
            .bind(&name)
            .bind(&fields.0)
            .bind(&fields.1)
            .bind(&fields.2)
            .bind(&fields.3)
            .bind(&fields.4)
            .execute(&mut *tx)
            .await
            .map_err(db_err)
            .and_then(|d| if d.rows_affected() == 1 { Ok(id) } else { Err(format!("OTHER_NOT_FOUND: other name {id} does not exist")) }),
    };
    match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(id)
        }
        Err(e) => Err(rollback(tx, e).await),
    }
}

pub async fn create(pool: &SqlitePool, input: &OtherNameInput) -> Result<OtherName, String> {
    let id = write(pool, None, input).await?;
    get(pool, id).await
}

pub async fn update(pool: &SqlitePool, id: i64, input: &OtherNameInput) -> Result<OtherName, String> {
    write(pool, Some(id), input).await?;
    get(pool, id).await
}

pub async fn set_active(pool: &SqlitePool, id: i64, active: bool) -> Result<OtherName, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match sqlx::query("UPDATE other_names SET is_active = ?2 WHERE id = ?1").bind(id).bind(active).execute(&mut *tx).await {
        Ok(d) if d.rows_affected() == 1 => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Ok(_) => return Err(rollback(tx, format!("OTHER_NOT_FOUND: other name {id} does not exist")).await),
        Err(e) => return Err(rollback(tx, db_err(e)).await),
    }
    get(pool, id).await
}

/// The other name a check's payee names, when no vendor has that name: checks and expenses pay a
/// vendor first, then an other name, and otherwise add the payee as a new vendor.
pub(crate) async fn payee(conn: &mut sqlx::SqliteConnection, name: &str) -> Result<Option<i64>, String> {
    let vendor: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM vendors WHERE name = ?1 COLLATE NOCASE)")
        .bind(name.trim())
        .fetch_one(&mut *conn)
        .await
        .map_err(db_err)?;
    if vendor {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT id FROM other_names WHERE name = ?1 COLLATE NOCASE").bind(name.trim()).fetch_optional(&mut *conn).await.map_err(db_err)
}

#[cfg(test)]
mod tests;
