//! Batch invoices (Customers › Create Batch Invoices): the same lines invoiced to every customer in
//! a group, or to customers picked by hand, at once.
//!
//! Every invoice goes through the invoice writer (and so through `ledger.rs`) inside one write
//! transaction: either all of them post or none do. Each customer's invoice is due on their own
//! terms when they have them, and is queued the way the customer wants forms sent (Preferred
//! delivery: Email later, Print later, or neither).

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::invoices::{self, InvoiceSummary};
use crate::ledger::db_err;

/// One line, as typed once for the whole batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLine {
    #[serde(default)]
    pub product_service_id: Option<i64>,
    pub description: String,
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub income_account_id: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub is_taxable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInput {
    /// Customers picked by hand (added to the group's, without repeats).
    #[serde(default)]
    pub customer_ids: Vec<i64>,
    /// A customer group: its members right now.
    #[serde(default)]
    pub group_id: Option<i64>,
    pub issue_date: NaiveDate,
    /// For customers without terms of their own: due this many days after the invoice date.
    #[serde(default = "default_due_days")]
    pub due_days: i64,
    pub lines: Vec<BatchLine>,
    /// Inactive customers are skipped unless this is set.
    #[serde(default)]
    pub include_inactive: bool,
}

fn default_due_days() -> i64 {
    30
}

/// What a batch made: the invoices, and the customers left out with why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub invoices: Vec<InvoiceSummary>,
    pub emailed_later: usize,
    pub printed_later: usize,
    pub skipped: Vec<(String, String)>,
}

/// Who the batch goes to: (id, name, own terms in days, preferred delivery, active).
type Recipient = (i64, String, Option<i64>, Option<String>, bool);

async fn recipients(pool: &SqlitePool, input: &BatchInput) -> Result<Vec<Recipient>, String> {
    let mut ids = Vec::new();
    if let Some(group) = input.group_id {
        ids.extend(crate::customer_groups::get(pool, group).await?.member_ids);
    }
    // In order, without repeats.
    let mut seen: std::collections::HashSet<i64> = ids.iter().copied().collect();
    ids.extend(input.customer_ids.iter().copied().filter(|id| seen.insert(*id)));
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let row: Option<Recipient> = sqlx::query_as(
            "SELECT c.id, c.name, COALESCE(t.net_days, c.terms_days), c.preferred_delivery, c.is_active
             FROM customers c LEFT JOIN terms t ON t.id = c.terms_id WHERE c.id = ?1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
        out.extend(row);
    }
    Ok(out)
}

/// Creates the batch: one invoice per customer, all in one write transaction.
pub async fn create(pool: &SqlitePool, input: BatchInput) -> Result<BatchResult, String> {
    if input.lines.is_empty() {
        return Err("BATCH_NO_LINES: add at least one line to invoice".into());
    }
    if !(0..=365).contains(&input.due_days) {
        return Err("BATCH_BAD_DUE: invoices are due 0 to 365 days after their date".into());
    }
    let mut skipped = Vec::new();
    let mut chosen = Vec::new();
    for r in recipients(pool, &input).await? {
        if !r.4 && !input.include_inactive {
            skipped.push((r.1.clone(), "inactive".to_string()));
        } else {
            chosen.push(r);
        }
    }
    if chosen.is_empty() {
        return Err("BATCH_NO_CUSTOMERS: choose at least one active customer".into());
    }
    let lines = serde_json::to_value(&input.lines).map_err(|e| format!("BATCH_INPUT: {e}"))?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let mut ids = Vec::with_capacity(chosen.len());
    let (mut emailed_later, mut printed_later) = (0, 0);
    let written = async {
        for (_, name, terms_days, delivery, _) in &chosen {
            let due = input.issue_date + Duration::days(terms_days.unwrap_or(input.due_days));
            // Built from JSON so the invoice input's optional fields keep their defaults.
            let invoice: crate::invoices::InvoiceInput =
                serde_json::from_value(serde_json::json!({ "customerName": name, "issueDate": input.issue_date, "dueDate": due, "lines": lines }))
                    .map_err(|e| format!("BATCH_INPUT: {e}"))?;
            let invoice_id = invoices::write_invoice(&mut tx, &invoice).await.map_err(|e| format!("{e} (for {name})"))?;
            match delivery.as_deref() {
                Some("EMAIL") => {
                    sqlx::query("INSERT INTO form_email_queue (form_type, document_id) VALUES ('INVOICE', ?1) ON CONFLICT DO NOTHING").bind(invoice_id).execute(&mut *tx).await.map_err(db_err)?;
                    emailed_later += 1;
                }
                Some("PRINT") => {
                    sqlx::query("UPDATE invoices SET to_be_printed = 1 WHERE id = ?1").bind(invoice_id).execute(&mut *tx).await.map_err(db_err)?;
                    printed_later += 1;
                }
                _ => {}
            }
            ids.push(invoice_id);
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(err) = written {
        let _ = tx.rollback().await;
        return Err(err);
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(invoices::get_summary(pool, id).await?);
    }
    Ok(BatchResult { invoices: out, emailed_later, printed_later, skipped })
}

#[cfg(test)]
mod tests;
