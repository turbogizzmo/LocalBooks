//! Statement charges (Customers › Enter Statement Charges): a charge entered in a customer's own
//! register, without writing an invoice, and billed on the customer's next statement.
//!
//! Each charge is kept as an invoice marked in `statement_charge_invoices` (migration 0090). That
//! way it posts to Accounts Receivable for the customer through the invoice writer (and so through
//! `ledger.rs`), ages from its due date, shows in Receive Payments for a payment or credit to
//! apply to, and voids like any invoice. It is listed in the customer's register here rather than
//! among invoices, shows on the statement as "Statement Charge" with its description, and never
//! prints as an invoice of its own.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::invoices::{self, InvoiceInput, InvoiceSummary};
use crate::ledger::db_err;

/// One charge, as typed in the register.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementChargeInput {
    pub customer_name: String,
    pub charge_date: NaiveDate,
    /// When it is due; the charge date when left out.
    #[serde(default)]
    pub due_date: Option<NaiveDate>,
    #[serde(default)]
    pub product_service_id: Option<i64>,
    pub description: String,
    /// Quantity scaled by 10,000; 1 when left out.
    #[serde(default)]
    pub quantity_e4: Option<i64>,
    pub rate_cents: i64,
    pub income_account_id: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
}

/// The invoice a charge is kept as. Built from JSON so the invoice input's optional fields keep
/// their defaults as the invoice form grows.
fn as_invoice(input: &StatementChargeInput) -> Result<InvoiceInput, String> {
    if input.description.trim().is_empty() {
        return Err("SC_DESCRIPTION: describe the charge".into());
    }
    let due = input.due_date.unwrap_or(input.charge_date);
    if due < input.charge_date {
        return Err("SC_DUE_BEFORE: the due date is before the charge date".into());
    }
    serde_json::from_value(serde_json::json!({
        "customerName": input.customer_name,
        "issueDate": input.charge_date,
        "dueDate": due,
        "lines": [{
            "description": input.description.trim(),
            "quantityE4": input.quantity_e4.unwrap_or(10_000),
            "rateCents": input.rate_cents,
            "incomeAccountId": input.income_account_id,
            "productServiceId": input.product_service_id,
            "classId": input.class_id,
        }],
    }))
    .map_err(|e| format!("SC_INPUT: {e}"))
}

/// Records a charge: posts it to A/R for the customer and marks it as a statement charge, in one
/// write transaction.
pub async fn create(pool: &SqlitePool, input: StatementChargeInput) -> Result<InvoiceSummary, String> {
    let invoice = as_invoice(&input)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let written = async {
        let id = invoices::write_invoice(&mut tx, &invoice).await?;
        sqlx::query("INSERT INTO statement_charge_invoices (invoice_id) VALUES (?1)").bind(id).execute(&mut *tx).await.map_err(db_err)?;
        Ok::<i64, String>(id)
    }
    .await;
    let id = match written {
        Ok(id) => id,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    invoices::get_summary(pool, id).await
}

/// A charge in the customer's register: the invoice it is kept as, and what it was for.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementCharge {
    #[serde(flatten)]
    pub invoice: InvoiceSummary,
    pub description: String,
}

/// A customer's statement charges (with their jobs'), oldest first: the customer's register.
pub async fn list(pool: &SqlitePool, customer_id: i64) -> Result<Vec<StatementCharge>, String> {
    // The charges in one query, and each one's description (its first line) in another.
    let charges = invoices::statement_charge_summaries(pool, customer_id).await?;
    let descriptions: std::collections::HashMap<i64, String> = sqlx::query_as::<_, (i64, String)>(
        "SELECT it.invoice_id, it.description FROM invoice_items it JOIN statement_charge_invoices s ON s.invoice_id = it.invoice_id
         WHERE it.line_number = (SELECT MIN(line_number) FROM invoice_items WHERE invoice_id = it.invoice_id)",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    .into_iter()
    .collect();
    Ok(charges.into_iter().map(|invoice| StatementCharge { description: descriptions.get(&invoice.id).cloned().unwrap_or_default(), invoice }).collect())
}

#[cfg(test)]
mod tests;
