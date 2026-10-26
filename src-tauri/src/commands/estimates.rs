//! Estimates and turning them into invoices.

use super::prelude::*;

handlers![
    list_estimates,
    create_estimate,
    set_estimate_status,
    convert_estimate_to_invoice,
    get_estimate_detail,
    invoice_from_estimate,
];

#[tauri::command]
pub async fn list_estimates(db: State<'_, Db>) -> Result<Vec<EstimateSummary>, String> {
    estimates::list_estimates(&db.pool().await).await
}

#[tauri::command]
pub async fn create_estimate(app: AppHandle, db: State<'_, Db>, input: EstimateInput) -> Result<EstimateSummary, String> {
    let estimate = estimates::create_estimate(&db.pool().await, input).await?;
    notify(&app);
    Ok(estimate)
}

#[tauri::command]
pub async fn set_estimate_status(
    app: AppHandle,
    db: State<'_, Db>,
    estimate_id: i64,
    status: String,
) -> Result<EstimateSummary, String> {
    let estimate = estimates::set_estimate_status(&db.pool().await, estimate_id, &status).await?;
    notify(&app);
    Ok(estimate)
}

#[tauri::command]
pub async fn convert_estimate_to_invoice(
    app: AppHandle,
    db: State<'_, Db>,
    estimate_id: i64,
    issue_date: NaiveDate,
    due_date: NaiveDate,
) -> Result<EstimateSummary, String> {
    let estimate = estimates::convert_to_invoice(&db.pool().await, estimate_id, issue_date, due_date).await?;
    notify(&app);
    Ok(estimate)
}

#[tauri::command]
pub async fn get_estimate_detail(db: State<'_, Db>, estimate_id: i64) -> Result<EstimateDetail, String> {
    estimates::estimate_detail(&db.pool().await, estimate_id).await
}

/// Posts an invoice started from an estimate (possibly edited) and marks the estimate converted.
#[tauri::command]
pub async fn invoice_from_estimate(app: AppHandle, db: State<'_, Db>, estimate_id: i64, input: InvoiceInput) -> Result<InvoiceSummary, String> {
    let invoice = estimates::invoice_from_estimate(&db.pool().await, estimate_id, input).await?;
    notify(&app);
    Ok(invoice)
}
