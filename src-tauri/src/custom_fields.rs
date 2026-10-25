//! Custom fields: up to 15 user-defined text fields on each of the customer, vendor and item lists,
//! like QuickBooks' "Define Fields". Each record keeps its own value for each field.
//!
//! A customer or item field can be marked for sales forms. A marked customer field appears on new
//! invoices, sales receipts and estimates, filled in from the customer and editable there; the
//! document keeps what it was issued with (`sales_document_fields`). A marked item field prints
//! under each line that sells the item, with the item's value at the time it prints.
//!
//! A field can be renamed, marked or unmarked at any time. Deleting one removes its values from
//! every record; a field that has been printed on a sales document cannot be deleted (unmark it
//! instead), so issued forms keep printing what they were issued with.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::db_err;
use crate::sales_lists::SalesDoc;

/// The most custom fields one list can have.
pub const MAX_FIELDS: i64 = 15;
/// The longest label and value, in characters.
pub const MAX_LABEL: usize = 31;
pub const MAX_VALUE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FieldList {
    Customer,
    Vendor,
    Item,
}

impl FieldList {
    pub fn key(self) -> &'static str {
        match self {
            FieldList::Customer => "CUSTOMER",
            FieldList::Vendor => "VENDOR",
            FieldList::Item => "ITEM",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            FieldList::Customer => "customer",
            FieldList::Vendor => "vendor",
            FieldList::Item => "item",
        }
    }

    fn records(self) -> &'static str {
        match self {
            FieldList::Customer => "customers",
            FieldList::Vendor => "vendors",
            FieldList::Item => "products_services",
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CustomFieldView {
    pub id: i64,
    /// `CUSTOMER`, `VENDOR` or `ITEM`.
    pub list: String,
    pub label: String,
    pub position: i64,
    pub on_sales_forms: bool,
    /// Records with a value in this field.
    pub value_count: i64,
    /// Printed on at least one sales document, so it can no longer be deleted.
    pub is_printed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFieldInput {
    pub label: String,
    #[serde(default)]
    pub on_sales_forms: bool,
}

/// One field's value on a record or document, as entered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldValue {
    pub field_id: i64,
    pub value: String,
}

/// A field's value with its label, for showing and printing.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LabeledValue {
    pub field_id: i64,
    pub label: String,
    pub value: String,
}

/// One field on a record: every field of the list, with the record's value or an empty one.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecordField {
    pub field_id: i64,
    pub label: String,
    pub on_sales_forms: bool,
    pub value: String,
}

pub async fn list_custom_fields(pool: &SqlitePool) -> Result<Vec<CustomFieldView>, String> {
    sqlx::query_as::<_, CustomFieldView>(
        "SELECT f.id, f.list, f.label, f.position, f.on_sales_forms,
                (SELECT COUNT(*) FROM custom_field_values v WHERE v.field_id = f.id) AS value_count,
                EXISTS (SELECT 1 FROM sales_document_fields d WHERE d.field_id = f.id) AS is_printed
         FROM custom_fields f ORDER BY CASE f.list WHEN 'CUSTOMER' THEN 0 WHEN 'VENDOR' THEN 1 ELSE 2 END, f.position",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_field(pool: &SqlitePool, id: i64) -> Result<CustomFieldView, String> {
    list_custom_fields(pool).await?.into_iter().find(|f| f.id == id).ok_or_else(|| format!("CF_NOT_FOUND: custom field {id} does not exist"))
}

fn label_of(input: &CustomFieldInput) -> Result<&str, String> {
    let label = input.label.trim();
    if label.is_empty() {
        return Err("CF_LABEL_REQUIRED: enter a label for the field".into());
    }
    if label.chars().count() > MAX_LABEL {
        return Err(format!("CF_LABEL_TOO_LONG: a field label is at most {MAX_LABEL} characters"));
    }
    Ok(label)
}

fn label_err(err: sqlx::Error, list: FieldList, label: &str) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        format!("CF_DUPLICATE: the {} list already has a field named '{label}'", list.noun())
    } else {
        msg
    }
}

async fn insert_field(tx: &mut Transaction<'_, Sqlite>, list: FieldList, label: &str, on_sales_forms: bool) -> Result<i64, String> {
    if on_sales_forms && list == FieldList::Vendor {
        return Err("CF_NOT_ON_FORMS: vendor fields are not printed on sales forms".into());
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM custom_fields WHERE list = ?1").bind(list.key()).fetch_one(&mut **tx).await.map_err(db_err)?;
    if count >= MAX_FIELDS {
        return Err(format!("CF_LIST_FULL: the {} list already has {MAX_FIELDS} custom fields", list.noun()));
    }
    sqlx::query_scalar(
        "INSERT INTO custom_fields (list, label, position, on_sales_forms)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM custom_fields WHERE list = ?1), ?3) RETURNING id",
    )
    .bind(list.key())
    .bind(label)
    .bind(on_sales_forms)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| label_err(e, list, label))
}

pub async fn create_custom_field(pool: &SqlitePool, list: FieldList, input: CustomFieldInput) -> Result<CustomFieldView, String> {
    let label = label_of(&input)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let id = match insert_field(&mut tx, list, label, input.on_sales_forms).await {
        Ok(id) => id,
        Err(err) => return Err(crate::ledger::rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    get_field(pool, id).await
}

/// Renames a field or changes whether it shows on sales forms.
pub async fn update_custom_field(pool: &SqlitePool, id: i64, input: CustomFieldInput) -> Result<CustomFieldView, String> {
    let label = label_of(&input)?;
    let field = get_field(pool, id).await?;
    let list = parse_list(&field.list)?;
    if input.on_sales_forms && list == FieldList::Vendor {
        return Err("CF_NOT_ON_FORMS: vendor fields are not printed on sales forms".into());
    }
    sqlx::query("UPDATE custom_fields SET label = ?1, on_sales_forms = ?2 WHERE id = ?3")
        .bind(label)
        .bind(input.on_sales_forms)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| label_err(e, list, label))?;
    get_field(pool, id).await
}

/// Deletes a field and every record's value in it. A field printed on a sales document stays.
pub async fn delete_custom_field(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let field = get_field(pool, id).await?;
    if field.is_printed {
        return Err(format!("CF_PRINTED: '{}' is printed on sales forms already issued; take it off sales forms instead", field.label));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        sqlx::query("DELETE FROM custom_fields WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
        // Close the gap so the list stays numbered 1, 2, 3...
        sqlx::query("UPDATE custom_fields SET position = position - 1 WHERE list = ?1 AND position > ?2")
            .bind(&field.list)
            .bind(field.position)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok::<(), String>(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err),
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

pub fn parse_list(key: &str) -> Result<FieldList, String> {
    match key.to_ascii_uppercase().as_str() {
        "CUSTOMER" => Ok(FieldList::Customer),
        "VENDOR" => Ok(FieldList::Vendor),
        "ITEM" => Ok(FieldList::Item),
        other => Err(format!("CF_BAD_LIST: '{other}' is not the customer, vendor or item list")),
    }
}

// ------------------------------------------------------------------------------------------------
// Values on records
// ------------------------------------------------------------------------------------------------

/// Every field of `list` with this record's value (empty when it has none), in the list's order.
pub async fn record_fields(pool: &SqlitePool, list: FieldList, record_id: i64) -> Result<Vec<RecordField>, String> {
    sqlx::query_as::<_, RecordField>(
        "SELECT f.id AS field_id, f.label, f.on_sales_forms, COALESCE(v.value, '') AS value
         FROM custom_fields f LEFT JOIN custom_field_values v ON v.field_id = f.id AND v.record_id = ?2
         WHERE f.list = ?1 ORDER BY f.position",
    )
    .bind(list.key())
    .bind(record_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

fn value_of(value: &str, label: &str) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.chars().count() > MAX_VALUE {
        return Err(format!("CF_VALUE_TOO_LONG: '{label}' is at most {MAX_VALUE} characters"));
    }
    Ok((!value.is_empty()).then(|| value.to_string()))
}

/// Sets a record's values: each field given is set, or cleared when blank; fields left out keep
/// their value.
pub(crate) async fn set_record_values(tx: &mut Transaction<'_, Sqlite>, list: FieldList, record_id: i64, values: &[FieldValue]) -> Result<(), String> {
    for v in values {
        let label: Option<String> = sqlx::query_scalar("SELECT label FROM custom_fields WHERE id = ?1 AND list = ?2")
            .bind(v.field_id)
            .bind(list.key())
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let label = label.ok_or_else(|| format!("CF_NOT_FOUND: that is not a {} field", list.noun()))?;
        match value_of(&v.value, &label)? {
            Some(value) => sqlx::query(
                "INSERT INTO custom_field_values (field_id, record_id, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT (field_id, record_id) DO UPDATE SET value = excluded.value",
            )
            .bind(v.field_id)
            .bind(record_id)
            .bind(value),
            None => sqlx::query("DELETE FROM custom_field_values WHERE field_id = ?1 AND record_id = ?2").bind(v.field_id).bind(record_id),
        }
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// Sets one record's values from the fields' editor.
pub async fn save_record_values(pool: &SqlitePool, list: FieldList, record_id: i64, values: &[FieldValue]) -> Result<Vec<RecordField>, String> {
    let exists: bool = sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {} WHERE id = ?1)", list.records()))
        .bind(record_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !exists {
        return Err(format!("CF_NO_RECORD: that {} does not exist", list.noun()));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if let Err(err) = set_record_values(&mut tx, list, record_id, values).await {
        return Err(crate::ledger::rollback(tx, err).await);
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    record_fields(pool, list, record_id).await
}

/// Sets values by label, adding a field for a label the list does not have yet (while there is
/// room). Used by the importers; returns how many values had no room.
pub(crate) async fn set_values_by_label(tx: &mut Transaction<'_, Sqlite>, list: FieldList, record_id: i64, values: &[(String, String)]) -> Result<usize, String> {
    let mut dropped = 0;
    let mut resolved = Vec::new();
    for (label, value) in values {
        let label: String = label.trim().chars().take(MAX_LABEL).collect::<String>().trim().to_string();
        let value: String = value.trim().chars().take(MAX_VALUE).collect::<String>().trim().to_string();
        if label.is_empty() || value.is_empty() {
            continue;
        }
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM custom_fields WHERE list = ?1 AND label = ?2 COLLATE NOCASE")
            .bind(list.key())
            .bind(&label)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let field_id = match existing {
            Some(id) => id,
            None => match insert_field(tx, list, &label, false).await {
                Ok(id) => id,
                Err(e) if e.starts_with("CF_LIST_FULL") => {
                    dropped += 1;
                    continue;
                }
                Err(e) => return Err(e),
            },
        };
        resolved.push(FieldValue { field_id, value });
    }
    set_record_values(tx, list, record_id, &resolved).await?;
    Ok(dropped)
}

// ------------------------------------------------------------------------------------------------
// On sales documents
// ------------------------------------------------------------------------------------------------

/// Stores the customer fields entered on a new sales document. Only customer fields marked for
/// sales forms may be given; blanks are skipped.
pub(crate) async fn write_document_fields(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, values: &[FieldValue]) -> Result<(), String> {
    for v in values {
        let label: Option<String> = sqlx::query_scalar("SELECT label FROM custom_fields WHERE id = ?1 AND list = 'CUSTOMER' AND on_sales_forms = 1")
            .bind(v.field_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let label = label.ok_or("CF_NOT_ON_FORMS: that custom field is not shown on sales forms")?;
        let Some(value) = value_of(&v.value, &label)? else { continue };
        sqlx::query(
            "INSERT INTO sales_document_fields (document_kind, document_id, field_id, value) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (document_kind, document_id, field_id) DO UPDATE SET value = excluded.value",
        )
        .bind(doc.key())
        .bind(id)
        .bind(v.field_id)
        .bind(value)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

pub async fn document_fields(pool: &SqlitePool, doc: SalesDoc, id: i64) -> Result<Vec<LabeledValue>, String> {
    sqlx::query_as::<_, LabeledValue>(
        "SELECT f.id AS field_id, f.label, d.value
         FROM sales_document_fields d JOIN custom_fields f ON f.id = d.field_id
         WHERE d.document_kind = ?1 AND d.document_id = ?2 ORDER BY f.position",
    )
    .bind(doc.key())
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Each item's values in the item fields marked for sales forms.
pub async fn item_form_fields(pool: &SqlitePool) -> Result<HashMap<i64, Vec<LabeledValue>>, String> {
    let rows: Vec<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT v.record_id, f.id, f.label, v.value FROM custom_field_values v JOIN custom_fields f ON f.id = v.field_id
         WHERE f.list = 'ITEM' AND f.on_sales_forms = 1 ORDER BY v.record_id, f.position",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut by_item: HashMap<i64, Vec<LabeledValue>> = HashMap::new();
    for (item, field_id, label, value) in rows {
        by_item.entry(item).or_default().push(LabeledValue { field_id, label, value });
    }
    Ok(by_item)
}

#[cfg(test)]
mod tests;
