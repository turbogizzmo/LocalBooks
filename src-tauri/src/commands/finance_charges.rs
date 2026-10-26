//! Finance charges on overdue invoices.

use super::prelude::*;

handlers![
    preview_finance_charges,
    assess_finance_charges,
];

/// Assess Finance Charges: customers with overdue invoices as of a date, and the charge for each.
#[tauri::command]
pub async fn preview_finance_charges(db: State<'_, Db>, as_of: NaiveDate) -> Result<crate::finance_charges::Preview, String> {
    crate::finance_charges::preview(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn assess_finance_charges(app: AppHandle, db: State<'_, Db>, input: crate::finance_charges::AssessInput) -> Result<Vec<InvoiceSummary>, String> {
    let invoices = crate::finance_charges::assess(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoices)
}
