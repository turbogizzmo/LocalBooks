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
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
    use chrono::NaiveDate;

    async fn invoice(pool: &SqlitePool, customer: &str) -> i64 {
        let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap();
        let d = |day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
        create_and_post_invoice(
            pool,
            InvoiceInput {
                customer_name: customer.into(),
                issue_date: d(1),
                due_date: d(31),
                lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: 5_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn queue_then_send_records_history_and_dequeues() {
        let pool = test_pool().await;
        let a = invoice(&pool, "Acme").await;
        let b = invoice(&pool, "Initech").await;
        sqlx::query("UPDATE customers SET email = 'ap@acme.test' WHERE name = 'Acme'").execute(&pool).await.unwrap();

        set_queued(&pool, FormKind::Invoice, a, true).await.unwrap();
        set_queued(&pool, FormKind::Invoice, a, true).await.unwrap(); // idempotent
        set_queued(&pool, FormKind::Invoice, b, true).await.unwrap();
        let queued = queue(&pool).await.unwrap();
        assert_eq!(queued.len(), 2);
        assert_eq!((queued[0].customer_name.as_str(), queued[0].customer_email.as_deref()), ("Acme", Some("ap@acme.test")));
        assert_eq!(queued[1].customer_email, None);
        assert!(set_queued(&pool, FormKind::Estimate, 999, true).await.is_err());

        record(&pool, FormKind::Invoice, a, &["ap@acme.test".into(), "cfo@acme.test".into()], Method::Smtp, "Invoice 1001").await.unwrap();
        record(&pool, FormKind::Invoice, a, &["ap@acme.test".into()], Method::MailApp, "Invoice 1001 again").await.unwrap();
        let sent = history(&pool, FormKind::Invoice, a).await.unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!((sent[0].method.as_str(), sent[0].subject.as_str()), ("MAIL_APP", "Invoice 1001 again"));
        assert_eq!(sent[1].recipients, "ap@acme.test, cfo@acme.test");

        let status = statuses(&pool, FormKind::Invoice).await.unwrap();
        let sa = status.iter().find(|s| s.document_id == a).unwrap();
        assert_eq!((sa.send_count, sa.queued, sa.last_recipients.as_deref()), (2, false, Some("ap@acme.test")));
        let sb = status.iter().find(|s| s.document_id == b).unwrap();
        assert_eq!((sb.send_count, sb.queued, sb.last_sent_at.as_deref()), (0, true, None));
        assert_eq!(queue(&pool).await.unwrap().len(), 1, "a sent form leaves the queue");

        set_queued(&pool, FormKind::Invoice, b, false).await.unwrap();
        assert!(queue(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_is_append_only_and_leaves_the_ledger_alone() {
        let pool = test_pool().await;
        let a = invoice(&pool, "Acme").await;
        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
        record(&pool, FormKind::Invoice, a, &["ap@acme.test".into()], Method::Smtp, "Invoice").await.unwrap();
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap(), entries);
        let edit = sqlx::query("UPDATE form_email_log SET recipients = 'x@y.test'").execute(&pool).await.unwrap_err();
        assert!(edit.to_string().contains("EMAIL_LOG_APPEND_ONLY"));
        assert!(sqlx::query("DELETE FROM form_email_log").execute(&pool).await.is_err());
    }

    #[tokio::test]
    async fn sales_receipts_queue_and_statements_do_not() {
        let pool = test_pool().await;
        let id = |n: &'static str| {
            let pool = pool.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
        };
        let (services, checking) = (id("4100").await, id("1010").await);
        let receipt = crate::sales::post_sales_receipt(
            &pool,
            serde_json::from_value(serde_json::json!({ "customerName": "Globex", "receiptDate": "2026-10-05", "depositAccountId": checking,
                "paymentMethod": "CASH", "lines": [{ "description": "Workshop", "quantityE4": 10_000, "rateCents": 9_900, "incomeAccountId": services }] }))
            .unwrap(),
        )
        .await
        .unwrap();
        set_queued(&pool, FormKind::SalesReceipt, receipt.id, true).await.unwrap();
        let queued = queue(&pool).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!((queued[0].kind, queued[0].number, queued[0].customer_name.as_str(), queued[0].total_cents), (FormKind::SalesReceipt, receipt.receipt_number, "Globex", 9_900));
        let customer: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Globex'").fetch_one(&pool).await.unwrap();
        assert!(set_queued(&pool, FormKind::Statement, customer, true).await.unwrap_err().starts_with("EMAIL_STATEMENT_QUEUE"));
        // Statements still keep a sent history, by customer.
        record(&pool, FormKind::Statement, customer, &["ap@globex.test".into()], Method::Smtp, "Statement").await.unwrap();
        assert_eq!(history(&pool, FormKind::Statement, customer).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn voided_invoices_cannot_be_queued() {
        let pool = test_pool().await;
        let a = invoice(&pool, "Acme").await;
        let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(a).fetch_one(&pool).await.unwrap();
        crate::ledger::void_transaction(&pool, entry).await.unwrap();
        assert!(set_queued(&pool, FormKind::Invoice, a, true).await.unwrap_err().starts_with("EMAIL_VOIDED"));
    }
}
