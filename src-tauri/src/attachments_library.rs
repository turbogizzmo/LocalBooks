//! The attachments library (T58): every file kept in the company file, whatever it is attached to.
//!
//! Files on posted transactions live in `attachments` (one per journal entry, `workflows.rs`);
//! files on customers, vendors, items, accounts, estimates, purchase orders and reconciliations in
//! `record_attachments` (migration 0095). A file is named by a [`FileRef`]: its table and id.
//!
//! Deleting is reversible (R9): [`delete`] hides a file and keeps it, [`restore`] brings it back,
//! and only a deleted file can be removed for good with [`purge`] (the schema refuses otherwise;
//! the screens ask first).

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};

/// Which table a file is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Source {
    /// On a posted transaction (`attachments`).
    Entry,
    /// On another record (`record_attachments`).
    Record,
}

impl Source {
    fn table(self) -> &'static str {
        match self {
            Source::Entry => "attachments",
            Source::Record => "record_attachments",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub source: Source,
    pub id: i64,
}

/// The query that checks a record of `kind` exists (bound to its id).
fn record_exists_sql(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "CUSTOMER" => "SELECT EXISTS (SELECT 1 FROM customers WHERE id = ?1)",
        "VENDOR" => "SELECT EXISTS (SELECT 1 FROM vendors WHERE id = ?1)",
        "ITEM" => "SELECT EXISTS (SELECT 1 FROM products_services WHERE id = ?1)",
        "ACCOUNT" => "SELECT EXISTS (SELECT 1 FROM chart_of_accounts WHERE id = ?1)",
        "ESTIMATE" => "SELECT EXISTS (SELECT 1 FROM estimates WHERE id = ?1)",
        "PURCHASE_ORDER" => "SELECT EXISTS (SELECT 1 FROM purchase_orders WHERE id = ?1)",
        "RECONCILIATION" => "SELECT EXISTS (SELECT 1 FROM reconciliations WHERE id = ?1)",
        _ => return None,
    })
}

/// A file on a record (the record's own panel lists these).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecordFile {
    pub id: i64,
    pub record_kind: String,
    pub record_id: i64,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub created_at: String,
    pub keywords: Option<String>,
    pub send_with_form: bool,
}

/// Keeps a copy of a file with a customer, vendor, item, account, estimate, purchase order or
/// reconciliation. Up to 25 MB, as on transactions.
pub async fn add_record_file(pool: &SqlitePool, record_kind: &str, record_id: i64, file_name: &str, content: &[u8]) -> Result<RecordFile, String> {
    let exists_sql = record_exists_sql(record_kind).ok_or_else(|| format!("ATTACH_KIND: files can't be attached to a {record_kind}"))?;
    if content.is_empty() {
        return Err("ATTACH_EMPTY: that file is empty".into());
    }
    if content.len() > crate::workflows::MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let exists: bool = sqlx::query_scalar(exists_sql).bind(record_id).fetch_one(pool).await.map_err(db_err)?;
    if !exists {
        return Err(format!("ATTACH_NO_RECORD: that {} does not exist", record_kind.to_lowercase().replace('_', " ")));
    }
    let name = std::path::Path::new(file_name).file_name().and_then(|n| n.to_str()).unwrap_or("attachment");
    sqlx::query_as(
        "INSERT INTO record_attachments (record_kind, record_id, file_name, media_type, size_bytes, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         RETURNING id, record_kind, record_id, file_name, media_type, size_bytes, created_at, keywords, send_with_form",
    )
    .bind(record_kind)
    .bind(record_id)
    .bind(name)
    .bind(crate::workflows::media_type(name))
    .bind(content.len() as i64)
    .bind(content)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// The files on a record that aren't deleted, oldest first.
pub async fn record_files(pool: &SqlitePool, record_kind: &str, record_id: i64) -> Result<Vec<RecordFile>, String> {
    sqlx::query_as(
        "SELECT id, record_kind, record_id, file_name, media_type, size_bytes, created_at, keywords, send_with_form
         FROM record_attachments WHERE record_kind = ?1 AND record_id = ?2 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(record_kind)
    .bind(record_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// One file in the library: what it is, what it is attached to, and whether it was deleted.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFile {
    pub source: String,
    pub id: i64,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub created_at: String,
    pub keywords: Option<String>,
    pub send_with_form: bool,
    pub deleted_at: Option<String>,
    /// For a transaction its display source type (`INVOICE`, `CHECK`, …); otherwise the record kind.
    pub attached_kind: String,
    /// The journal entry (transactions) or the record's id.
    pub attached_id: i64,
    /// The document's number, where it has one.
    pub attached_number: Option<String>,
    /// The name it is for: the customer or vendor, item, account, or reconciled account.
    pub attached_name: Option<String>,
    /// The transaction's date, or the reconciliation's statement date.
    pub attached_date: Option<String>,
}

/// Every file, newest first. `query` (case ignored) matches the file name, keywords, and what it is
/// attached to; `include_deleted` adds the deleted ones (marked by `deletedAt`).
pub async fn library(pool: &SqlitePool, query: Option<&str>, include_deleted: bool) -> Result<Vec<LibraryFile>, String> {
    let mut files: Vec<LibraryFile> = sqlx::query_as(&format!(
        "SELECT * FROM (
            SELECT 'ENTRY' AS source, a.id, a.file_name, a.media_type, a.size_bytes, a.created_at, a.keywords, a.send_with_form, a.deleted_at,
                   {DISPLAY_SOURCE_TYPE} AS attached_kind, e.id AS attached_id,
                   COALESCE(x.check_number, inv.invoice_number, sr.receipt_number, cm.credit_number, bl.bill_number, vc.reference, bp.reference, ip.reference,
                            CASE WHEN e.source_type IN ('JOURNAL', 'OPENING_BALANCE') THEN CAST(e.entry_number AS TEXT) END) AS attached_number,
                   p.party_name AS attached_name, e.entry_date AS attached_date
            FROM attachments a
            JOIN journal_entries e ON e.id = a.journal_entry_id
            LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
            LEFT JOIN expenses x ON e.source_type IN ('EXPENSE', 'CHECK') AND x.id = e.source_id
            LEFT JOIN (SELECT id, CAST(invoice_number AS TEXT) AS invoice_number FROM invoices) inv ON e.source_type = 'INVOICE' AND inv.id = e.source_id
            LEFT JOIN (SELECT id, CAST(receipt_number AS TEXT) AS receipt_number FROM sales_receipts) sr ON e.source_type = 'SALES_RECEIPT' AND sr.id = e.source_id
            LEFT JOIN (SELECT id, CAST(credit_number AS TEXT) AS credit_number FROM credit_memos) cm ON e.source_type = 'CREDIT_MEMO' AND cm.id = e.source_id
            LEFT JOIN bills bl ON e.source_type = 'BILL' AND bl.id = e.source_id
            LEFT JOIN vendor_credits vc ON e.source_type = 'VENDOR_CREDIT' AND vc.id = e.source_id
            LEFT JOIN bill_payments bp ON e.source_type = 'BILL_PAYMENT' AND bp.id = e.source_id
            LEFT JOIN invoice_payments ip ON e.source_type = 'RECEIVE_PAYMENT' AND ip.id = e.source_id
            UNION ALL
            SELECT 'RECORD', r.id, r.file_name, r.media_type, r.size_bytes, r.created_at, r.keywords, r.send_with_form, r.deleted_at,
                   r.record_kind, r.record_id,
                   CASE r.record_kind
                       WHEN 'ESTIMATE' THEN (SELECT CAST(estimate_number AS TEXT) FROM estimates WHERE id = r.record_id)
                       WHEN 'PURCHASE_ORDER' THEN (SELECT CAST(po_number AS TEXT) FROM purchase_orders WHERE id = r.record_id)
                       WHEN 'ACCOUNT' THEN (SELECT account_number FROM chart_of_accounts WHERE id = r.record_id)
                   END,
                   CASE r.record_kind
                       WHEN 'CUSTOMER' THEN (SELECT name FROM customers WHERE id = r.record_id)
                       WHEN 'VENDOR' THEN (SELECT name FROM vendors WHERE id = r.record_id)
                       WHEN 'ITEM' THEN (SELECT name FROM products_services WHERE id = r.record_id)
                       WHEN 'ACCOUNT' THEN (SELECT full_name FROM v_chart_of_accounts WHERE id = r.record_id)
                       WHEN 'ESTIMATE' THEN (SELECT c.name FROM estimates s JOIN customers c ON c.id = s.customer_id WHERE s.id = r.record_id)
                       WHEN 'PURCHASE_ORDER' THEN (SELECT v.name FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id WHERE o.id = r.record_id)
                       WHEN 'RECONCILIATION' THEN (SELECT a.full_name FROM reconciliations c JOIN v_chart_of_accounts a ON a.id = c.account_id WHERE c.id = r.record_id)
                   END,
                   CASE r.record_kind
                       WHEN 'ESTIMATE' THEN (SELECT estimate_date FROM estimates WHERE id = r.record_id)
                       WHEN 'PURCHASE_ORDER' THEN (SELECT po_date FROM purchase_orders WHERE id = r.record_id)
                       WHEN 'RECONCILIATION' THEN (SELECT statement_end_date FROM reconciliations WHERE id = r.record_id)
                   END
            FROM record_attachments r
         ) WHERE ?1 OR deleted_at IS NULL
         ORDER BY created_at DESC, source, id DESC"
    ))
    .bind(include_deleted)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if let Some(needle) = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty()) {
        files.retain(|f| {
            [Some(&f.file_name), f.keywords.as_ref(), f.attached_name.as_ref(), f.attached_number.as_ref(), Some(&f.attached_kind)]
                .into_iter()
                .flatten()
                .any(|text| text.to_lowercase().replace('_', " ").contains(&needle))
        });
    }
    Ok(files)
}

/// A file's name, media type and bytes (to save a copy or preview it).
pub async fn content(pool: &SqlitePool, file: FileRef) -> Result<(String, String, Vec<u8>), String> {
    sqlx::query_as(&format!("SELECT file_name, media_type, content FROM {} WHERE id = ?1", file.source.table()))
        .bind(file.id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "ATTACH_NOT_FOUND: that attachment does not exist".into())
}

/// Sets a file's keywords (blank clears them) and whether it goes with the form when emailed.
pub async fn set_details(pool: &SqlitePool, file: FileRef, keywords: Option<&str>, send_with_form: bool) -> Result<(), String> {
    let keywords = keywords.map(str::trim).filter(|k| !k.is_empty());
    if keywords.is_some_and(|k| k.chars().count() > 500) {
        return Err("ATTACH_KEYWORDS_LONG: keep keywords to 500 characters".into());
    }
    one_row(
        sqlx::query(&format!("UPDATE {} SET keywords = ?1, send_with_form = ?2 WHERE id = ?3", file.source.table()))
            .bind(keywords)
            .bind(send_with_form)
            .bind(file.id)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected(),
    )
}

/// Deletes a file reversibly: hidden everywhere but the library's Deleted view, and kept.
pub async fn delete(pool: &SqlitePool, file: FileRef) -> Result<(), String> {
    one_row(
        sqlx::query(&format!("UPDATE {} SET deleted_at = COALESCE(deleted_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) WHERE id = ?1", file.source.table()))
            .bind(file.id)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected(),
    )
}

/// Brings a deleted file back to its transaction or record.
pub async fn restore(pool: &SqlitePool, file: FileRef) -> Result<(), String> {
    one_row(sqlx::query(&format!("UPDATE {} SET deleted_at = NULL WHERE id = ?1", file.source.table())).bind(file.id).execute(pool).await.map_err(db_err)?.rows_affected())
}

/// Removes a deleted file for good. A file not deleted first is refused (by the schema too).
pub async fn purge(pool: &SqlitePool, file: FileRef) -> Result<(), String> {
    let deleted: Option<Option<String>> = sqlx::query_scalar(&format!("SELECT deleted_at FROM {} WHERE id = ?1", file.source.table()))
        .bind(file.id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    match deleted {
        None => Err("ATTACH_NOT_FOUND: that attachment does not exist".into()),
        Some(None) => Err("ATTACH_NOT_DELETED: delete the file first; only deleted files can be removed for good".into()),
        Some(Some(_)) => {
            sqlx::query(&format!("DELETE FROM {} WHERE id = ?1", file.source.table())).bind(file.id).execute(pool).await.map(|_| ()).map_err(db_err)
        }
    }
}

/// The most the files sent with one form may add up to: mail servers commonly refuse messages much
/// over 25 MB, and the form's PDF and the encoding add to it.
pub const MAX_SENT_WITH_FORM_BYTES: i64 = 20 * 1024 * 1024;

/// A file to send with an emailed form: its name, media type and bytes.
pub type SentFile = (String, String, Vec<u8>);

/// The files marked Send with the form for the form `kind` / `document_id`, oldest first: those on
/// an invoice's or sales receipt's transaction, an estimate's own, and for a statement the
/// customer's. Deleted files are left out. Refused when they add up to more than
/// [`MAX_SENT_WITH_FORM_BYTES`].
pub async fn files_to_send(pool: &SqlitePool, kind: crate::forms::FormKind, document_id: i64) -> Result<Vec<SentFile>, String> {
    use crate::forms::FormKind;
    let sql = match kind {
        FormKind::Invoice => {
            "SELECT a.file_name, a.media_type, a.content FROM attachments a JOIN invoices i ON i.journal_entry_id = a.journal_entry_id
             WHERE i.id = ?1 AND a.send_with_form = 1 AND a.deleted_at IS NULL ORDER BY a.id"
        }
        FormKind::SalesReceipt => {
            "SELECT a.file_name, a.media_type, a.content FROM attachments a JOIN sales_receipts r ON r.journal_entry_id = a.journal_entry_id
             WHERE r.id = ?1 AND a.send_with_form = 1 AND a.deleted_at IS NULL ORDER BY a.id"
        }
        FormKind::Estimate => {
            "SELECT file_name, media_type, content FROM record_attachments
             WHERE record_kind = 'ESTIMATE' AND record_id = ?1 AND send_with_form = 1 AND deleted_at IS NULL ORDER BY id"
        }
        FormKind::Statement => {
            "SELECT file_name, media_type, content FROM record_attachments
             WHERE record_kind = 'CUSTOMER' AND record_id = ?1 AND send_with_form = 1 AND deleted_at IS NULL ORDER BY id"
        }
    };
    let files: Vec<SentFile> = sqlx::query_as(sql).bind(document_id).fetch_all(pool).await.map_err(db_err)?;
    let total: i64 = files.iter().map(|f| f.2.len() as i64).sum();
    if total > MAX_SENT_WITH_FORM_BYTES {
        return Err(format!(
            "MAIL_ATTACHMENTS_TOO_LARGE: the files marked to send with this form add up to {} MB; keep them under 20 MB (untick some in the Attachments Library)",
            total / (1024 * 1024)
        ));
    }
    Ok(files)
}

fn one_row(affected: u64) -> Result<(), String> {
    if affected == 1 { Ok(()) } else { Err("ATTACH_NOT_FOUND: that attachment does not exist".into()) }
}

#[cfg(test)]
mod tests;
