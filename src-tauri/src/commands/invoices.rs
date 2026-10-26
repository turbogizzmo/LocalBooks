//! Invoices, customer payments and A/R aging.

use super::prelude::*;

handlers![
    list_invoices,
    create_invoice,
    record_invoice_payment,
    list_invoice_payments,
    get_ar_aging,
    record_invoice_payments,
    get_invoice_detail,
];

#[tauri::command]
pub async fn list_invoices(db: State<'_, Db>) -> Result<Vec<InvoiceSummary>, String> {
    invoices::list_invoices(&db.pool().await).await
}

#[tauri::command]
pub async fn create_invoice(
    app: AppHandle,
    db: State<'_, Db>,
    input: InvoiceInput,
) -> Result<InvoiceSummary, String> {
    let invoice = invoices::create_and_post_invoice(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoice)
}

#[tauri::command]
pub async fn record_invoice_payment(
    app: AppHandle,
    db: State<'_, Db>,
    input: PaymentInput,
) -> Result<InvoiceSummary, String> {
    let invoice = invoices::record_payment(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoice)
}

#[tauri::command]
pub async fn list_invoice_payments(db: State<'_, Db>, invoice_id: i64) -> Result<Vec<PaymentView>, String> {
    invoices::list_payments(&db.pool().await, invoice_id).await
}

#[tauri::command]
pub async fn get_ar_aging(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<ArAging, String> {
    invoices::ar_aging(&db.pool().await, as_of.unwrap_or_else(|| Local::now().date_naive())).await
}

#[tauri::command]
pub async fn record_invoice_payments(app: AppHandle, db: State<'_, Db>, inputs: Vec<PaymentInput>) -> Result<usize, String> {
    let count = invoices::record_payments(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn get_invoice_detail(db: State<'_, Db>, invoice_id: i64) -> Result<InvoiceDetail, String> {
    invoices::invoice_detail(&db.pool().await, invoice_id).await
}
