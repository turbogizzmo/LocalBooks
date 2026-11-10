//! Sales orders, invoicing them in parts, and the open-orders reports (`sales_orders.rs`).

use super::prelude::*;
use crate::sales_orders::{self, OrderFromEstimate, SalesOrderDetail, SalesOrderInput, SalesOrderSummary, Take};

handlers![
    list_sales_orders,
    get_sales_order,
    create_sales_order,
    update_sales_order,
    close_sales_order,
    reopen_sales_order,
    delete_sales_order,
    invoice_from_sales_order,
    create_sales_order_from_estimate,
];

#[tauri::command]
pub async fn list_sales_orders(db: State<'_, Db>, open_only: Option<bool>) -> Result<Vec<SalesOrderSummary>, String> {
    sales_orders::list(&db.pool().await, open_only.unwrap_or(false)).await
}

#[tauri::command]
pub async fn get_sales_order(db: State<'_, Db>, id: i64) -> Result<SalesOrderDetail, String> {
    sales_orders::detail(&db.pool().await, id).await
}

#[tauri::command]
pub async fn create_sales_order(app: AppHandle, db: State<'_, Db>, input: SalesOrderInput) -> Result<SalesOrderSummary, String> {
    let order = sales_orders::create(&db.pool().await, &input).await?;
    notify(&app);
    Ok(order)
}

#[tauri::command]
pub async fn update_sales_order(app: AppHandle, db: State<'_, Db>, id: i64, input: SalesOrderInput) -> Result<SalesOrderSummary, String> {
    let order = sales_orders::update(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(order)
}

#[tauri::command]
pub async fn close_sales_order(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<SalesOrderSummary, String> {
    let order = sales_orders::close(&db.pool().await, id).await?;
    notify(&app);
    Ok(order)
}

#[tauri::command]
pub async fn reopen_sales_order(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<SalesOrderSummary, String> {
    let order = sales_orders::reopen(&db.pool().await, id).await?;
    notify(&app);
    Ok(order)
}

#[tauri::command]
pub async fn delete_sales_order(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    sales_orders::delete(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Posts an invoice made from a sales order, taking the chosen quantities off it.
#[tauri::command]
pub async fn invoice_from_sales_order(app: AppHandle, db: State<'_, Db>, id: i64, input: InvoiceInput, takes: Vec<Take>, confirmed: Option<Vec<String>>) -> Result<InvoiceSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::invoice(&input, None), confirmed.as_deref()).await?;
    let invoice = sales_orders::invoice_from_order(&pool, id, &input, &takes).await?;
    notify(&app);
    Ok(invoice)
}

/// Makes a sales order from an estimate, dated `order_date`.
#[tauri::command]
pub async fn create_sales_order_from_estimate(app: AppHandle, db: State<'_, Db>, estimate_id: i64, order_date: NaiveDate) -> Result<OrderFromEstimate, String> {
    let order = sales_orders::create_from_estimate(&db.pool().await, estimate_id, order_date).await?;
    notify(&app);
    Ok(order)
}
