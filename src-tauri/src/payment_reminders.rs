//! Payment reminders by customer group (Customer Center › Groups): every so many days, a group's
//! overdue invoices come up in Reminders to review. Sending queues each chosen invoice for Email
//! later (Sales › Send Forms emails it with the invoice's message) and starts the wait for the next
//! round. Nothing here touches the books.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::customer_groups::{self, CustomerGroup};
use crate::ledger::db_err;

/// An overdue invoice of a group's customer (or one of their jobs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OverdueInvoice {
    pub invoice_id: i64,
    pub invoice_number: i64,
    pub customer_name: String,
    pub customer_email: Option<String>,
    pub due_date: NaiveDate,
    pub days_overdue: i64,
    pub balance_cents: i64,
    /// Already waiting in Email later.
    pub queued: bool,
}

/// The group's invoices at least its `min_days_overdue` past due on `as_of`, most overdue first.
pub async fn overdue(pool: &SqlitePool, group: &CustomerGroup, as_of: NaiveDate) -> Result<Vec<OverdueInvoice>, String> {
    if group.member_ids.is_empty() {
        return Ok(Vec::new());
    }
    let members = serde_json::to_string(&group.member_ids).map_err(|e| format!("GROUP_SAVE: {e}"))?;
    sqlx::query_as(
        "SELECT i.id AS invoice_id, i.invoice_number, COALESCE(c.name, i.customer_name) AS customer_name, c.email AS customer_email, i.due_date,
                CAST(julianday(?2) - julianday(i.due_date) AS INTEGER) AS days_overdue, b.home_open_cents AS balance_cents,
                EXISTS (SELECT 1 FROM form_email_queue q WHERE q.form_type = 'INVOICE' AND q.document_id = i.id) AS queued
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 AND b.home_open_cents > 0
           AND (c.id IN (SELECT value FROM json_each(?1)) OR c.parent_id IN (SELECT value FROM json_each(?1)))
           AND julianday(?2) - julianday(i.due_date) >= ?3
         ORDER BY i.due_date, i.invoice_number",
    )
    .bind(&members)
    .bind(as_of)
    .bind(group.payments.min_days_overdue)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Sends a round: queues the chosen invoices (they must be the group's overdue ones) for Email
/// later and marks the group's reminder sent on `as_of`. Returns how many were queued.
pub async fn send(pool: &SqlitePool, group_id: i64, invoice_ids: &[i64], as_of: NaiveDate) -> Result<usize, String> {
    let group = customer_groups::get(pool, group_id).await?;
    let open: std::collections::HashSet<i64> = overdue(pool, &group, as_of).await?.into_iter().map(|o| o.invoice_id).collect();
    if let Some(stray) = invoice_ids.iter().find(|id| !open.contains(id)) {
        return Err(format!("REMINDER_NOT_OVERDUE: invoice {stray} is not one of this group's overdue invoices"));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    for id in invoice_ids {
        sqlx::query("INSERT INTO form_email_queue (form_type, document_id) VALUES ('INVOICE', ?1) ON CONFLICT DO NOTHING")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    sqlx::query("UPDATE customer_groups SET payment_last_sent = ?1 WHERE id = ?2").bind(as_of).bind(group_id).execute(&mut *tx).await.map_err(db_err)?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(invoice_ids.len())
}

/// Groups whose payment reminders are due on `as_of` and that have overdue invoices: (group, how
/// many invoices, their open total).
pub async fn due(pool: &SqlitePool, as_of: NaiveDate) -> Result<Vec<(CustomerGroup, usize, i64)>, String> {
    let mut out = Vec::new();
    for g in customer_groups::list(pool).await? {
        if !g.payments.next_due(as_of).is_some_and(|d| d <= as_of) {
            continue;
        }
        let invoices = overdue(pool, &g, as_of).await?;
        if !invoices.is_empty() {
            let total =
                invoices.iter().try_fold(0_i64, |sum, i| sum.checked_add(i.balance_cents)).ok_or("REMINDER_OVERFLOW: the overdue total is too large")?;
            out.push((g, invoices.len(), total));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
