//! Edit › Delete on the customer, vendor and item lists (gap T33; migration 0106): a record that
//! nothing uses is deleted; one that something uses is refused, with a pointer to Make Inactive.
//! The foreign keys find nearly every use; invoices also keep the customer's name, checked here.

use sqlx::SqlitePool;

use crate::ledger::{db_err, rollback};

fn table(kind: &str) -> Result<(&'static str, &'static str), String> {
    match kind {
        "customer" => Ok(("customers", "CUSTOMER")),
        "vendor" => Ok(("vendors", "VENDOR")),
        "item" => Ok(("products_services", "ITEM")),
        other => Err(format!("LIST_BAD_KIND: unknown list '{other}'")),
    }
}

/// Deletes a customer, vendor or item that nothing uses. Returns its name.
pub async fn delete_record(pool: &SqlitePool, kind: &str, id: i64) -> Result<String, String> {
    let (t, list) = table(kind)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let name: Option<String> = sqlx::query_scalar(&format!("SELECT name FROM {t} WHERE id = ?1")).bind(id).fetch_optional(&mut *tx).await.map_err(db_err)?;
    let Some(name) = name else {
        return Err(rollback(tx, format!("LIST_NOT_FOUND: that {kind} does not exist")).await);
    };
    let in_use = |what: &str| format!("LIST_IN_USE: {name} {what}; make it inactive instead, which keeps its history and hides it from new transactions");
    if kind == "customer" {
        let named: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM invoices WHERE customer_name = ?1 COLLATE NOCASE)")
            .bind(&name)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if named {
            return Err(rollback(tx, in_use("has invoices")).await);
        }
    }
    // Values of custom fields and a place in the list's order go with the record.
    let cleared = sqlx::query("DELETE FROM custom_field_values WHERE record_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE list = ?2)")
        .bind(id)
        .bind(list)
        .execute(&mut *tx)
        .await;
    if let Err(err) = cleared {
        return Err(rollback(tx, db_err(err)).await);
    }
    match sqlx::query(&format!("DELETE FROM {t} WHERE id = ?1")).bind(id).execute(&mut *tx).await {
        Ok(_) => {}
        // A row still pointing at this one (SQLite reports FOREIGN KEY constraint failed).
        Err(err) if err.as_database_error().is_some_and(|e| e.is_foreign_key_violation() || e.message().contains("FOREIGN KEY")) => {
            let what = if kind == "item" { "is on transactions, groups, prices or stock records" } else { "has transactions, jobs or other records" };
            return Err(rollback(tx, in_use(what)).await);
        }
        Err(err) => return Err(rollback(tx, db_err(err)).await),
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(name)
}

#[cfg(test)]
mod tests;
