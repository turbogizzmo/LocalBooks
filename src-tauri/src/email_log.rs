//! Emailed forms: the append-only sent history and the "Email later" queue.
//!
//! Every successful send (over SMTP, or a message opened in the user's mail app) adds a row to
//! `form_email_log` and takes the form off the queue in the same transaction. The history is a
//! record about the form; it never changes the posted transaction, and the database refuses to
//! edit or delete it.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::forms::FormKind;
use crate::ledger::db_err;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    MailApp,
    Smtp,
}

impl Method {
    fn key(self) -> &'static str {
        match self {
            Method::MailApp => "MAIL_APP",
            Method::Smtp => "SMTP",
        }
    }
}

/// One entry of a form's sent history.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SentRecord {
    pub sent_at: String,
    /// Comma-separated addresses.
    pub recipients: String,
    /// MAIL_APP or SMTP.
    pub method: String,
    pub subject: String,
}

/// Records a send and takes the form off the "Email later" queue.
pub async fn record(pool: &SqlitePool, kind: FormKind, document_id: i64, recipients: &[String], method: Method, subject: &str) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    sqlx::query("INSERT INTO form_email_log (form_type, document_id, recipients, method, subject) VALUES (?1, ?2, ?3, ?4, ?5)")
        .bind(kind.key())
        .bind(document_id)
        .bind(recipients.join(", "))
        .bind(method.key())
        .bind(subject)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("DELETE FROM form_email_queue WHERE form_type = ?1 AND document_id = ?2").bind(kind.key()).bind(document_id).execute(&mut *tx).await.map_err(db_err)?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// A form's sent history, newest first.
pub async fn history(pool: &SqlitePool, kind: FormKind, document_id: i64) -> Result<Vec<SentRecord>, String> {
    sqlx::query_as("SELECT sent_at, recipients, method, subject FROM form_email_log WHERE form_type = ?1 AND document_id = ?2 ORDER BY id DESC")
        .bind(kind.key())
        .bind(document_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Per form of one type: the last send and whether it is waiting in the queue.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EmailStatus {
    pub document_id: i64,
    pub last_sent_at: Option<String>,
    pub last_recipients: Option<String>,
    pub send_count: i64,
    pub queued: bool,
}

/// The forms of a type that have been emailed or are queued (others have no entry).
pub async fn statuses(pool: &SqlitePool, kind: FormKind) -> Result<Vec<EmailStatus>, String> {
    sqlx::query_as(
        "WITH ids AS (
             SELECT document_id FROM form_email_log WHERE form_type = ?1
             UNION SELECT document_id FROM form_email_queue WHERE form_type = ?1
         )
         SELECT ids.document_id,
                (SELECT sent_at FROM form_email_log l WHERE l.form_type = ?1 AND l.document_id = ids.document_id ORDER BY l.id DESC LIMIT 1) AS last_sent_at,
                (SELECT recipients FROM form_email_log l WHERE l.form_type = ?1 AND l.document_id = ids.document_id ORDER BY l.id DESC LIMIT 1) AS last_recipients,
                (SELECT COUNT(*) FROM form_email_log l WHERE l.form_type = ?1 AND l.document_id = ids.document_id) AS send_count,
                EXISTS (SELECT 1 FROM form_email_queue q WHERE q.form_type = ?1 AND q.document_id = ids.document_id) AS queued
         FROM ids ORDER BY ids.document_id",
    )
    .bind(kind.key())
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Marks a form "Email later", or takes it off the queue.
pub async fn set_queued(pool: &SqlitePool, kind: FormKind, document_id: i64, queued: bool) -> Result<(), String> {
    if !queued {
        sqlx::query("DELETE FROM form_email_queue WHERE form_type = ?1 AND document_id = ?2").bind(kind.key()).bind(document_id).execute(pool).await.map_err(db_err)?;
        return Ok(());
    }
    let state: Option<bool> = match kind {
        FormKind::Invoice => sqlx::query_scalar("SELECT is_voided FROM invoices WHERE id = ?1").bind(document_id).fetch_optional(pool).await,
        FormKind::Estimate => sqlx::query_scalar("SELECT 0 FROM estimates WHERE id = ?1").bind(document_id).fetch_optional(pool).await,
        FormKind::SalesReceipt => sqlx::query_scalar("SELECT is_voided FROM sales_receipts WHERE id = ?1").bind(document_id).fetch_optional(pool).await,
        // A statement covers a period chosen when it is sent; it is emailed from Customer Statements.
        FormKind::Statement => return Err("EMAIL_STATEMENT_QUEUE: email statements from Sales › Customer Statements".into()),
    }
    .map_err(db_err)?;
    match state {
        None => return Err("EMAIL_NOT_FOUND: that form no longer exists".into()),
        Some(true) => return Err("EMAIL_VOIDED: a voided form cannot be queued for email".into()),
        Some(false) => {}
    }
    sqlx::query("INSERT INTO form_email_queue (form_type, document_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING")
        .bind(kind.key())
        .bind(document_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// A form waiting in the "Email later" queue, for Send Forms.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedForm {
    pub kind: FormKind,
    pub document_id: i64,
    pub number: i64,
    pub customer_name: String,
    /// The customer's email address, if any.
    pub customer_email: Option<String>,
    pub date: String,
    pub total_cents: i64,
    pub currency_code: Option<String>,
    pub queued_at: String,
}

pub async fn queue(pool: &SqlitePool) -> Result<Vec<QueuedForm>, String> {
    type Row = (String, i64, i64, String, Option<String>, String, i64, Option<String>, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT q.form_type, q.document_id, i.invoice_number, COALESCE(c.name, i.customer_name), c.email, i.issue_date, i.total_cents, i.currency_code, q.queued_at
         FROM form_email_queue q JOIN invoices i ON q.form_type = 'INVOICE' AND i.id = q.document_id LEFT JOIN customers c ON c.id = i.customer_id
         UNION ALL
         SELECT q.form_type, q.document_id, e.estimate_number, c.name, c.email, e.estimate_date, e.total_cents, NULL, q.queued_at
         FROM form_email_queue q JOIN estimates e ON q.form_type = 'ESTIMATE' AND e.id = q.document_id JOIN customers c ON c.id = e.customer_id
         UNION ALL
         SELECT q.form_type, q.document_id, r.receipt_number, COALESCE(c.name, ''), c.email, r.receipt_date, r.total_cents, NULL, q.queued_at
         FROM form_email_queue q JOIN sales_receipts r ON q.form_type = 'SALES_RECEIPT' AND r.id = q.document_id LEFT JOIN customers c ON c.id = r.customer_id
         ORDER BY 9, 1, 3",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(kind, document_id, number, customer_name, customer_email, date, total_cents, currency_code, queued_at)| QueuedForm {
            kind: match kind.as_str() {
                "ESTIMATE" => FormKind::Estimate,
                "SALES_RECEIPT" => FormKind::SalesReceipt,
                _ => FormKind::Invoice,
            },
            document_id,
            number,
            customer_name,
            customer_email: customer_email.filter(|e| !e.trim().is_empty()),
            date,
            total_cents,
            currency_code,
            queued_at,
        })
        .collect())
}

#[cfg(test)]
mod tests;
