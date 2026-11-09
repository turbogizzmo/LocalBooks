//! Customers › Enter Statement Charges (`statement_charges.rs`).

use super::prelude::*;
use crate::invoices::InvoiceSummary;
use crate::statement_charges::{self, StatementCharge, StatementChargeInput};

handlers![
    list_statement_charges,
    create_statement_charge,
];

/// A customer's statement charges (their register), oldest first.
#[tauri::command]
pub async fn list_statement_charges(db: State<'_, Db>, customer_id: i64) -> Result<Vec<StatementCharge>, String> {
    statement_charges::list(&db.pool().await, customer_id).await
}

/// Records a statement charge: posted to A/R, billed on the customer's next statement.
#[tauri::command]
pub async fn create_statement_charge(app: AppHandle, db: State<'_, Db>, input: StatementChargeInput) -> Result<InvoiceSummary, String> {
    let charge = statement_charges::create(&db.pool().await, input).await?;
    notify(&app);
    Ok(charge)
}
