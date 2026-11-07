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
pub async fn create_estimate(app: AppHandle, db: State<'_, Db>, input: EstimateInput, confirmed: Option<Vec<String>>) -> Result<EstimateSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::dated(input.estimate_date), confirmed.as_deref()).await?;
    let estimate = estimates::create_estimate(&pool, input).await?;
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
pub async fn invoice_from_estimate(app: AppHandle, db: State<'_, Db>, estimate_id: i64, input: InvoiceInput, confirmed: Option<Vec<String>>) -> Result<InvoiceSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::sale(input.issue_date, Some(&input.customer_name), input.lines.iter().map(|l| (l.product_service_id, l.quantity_e4))), confirmed.as_deref()).await?;
    let invoice = estimates::invoice_from_estimate(&pool, estimate_id, input).await?;
    notify(&app);
    Ok(invoice)
}
