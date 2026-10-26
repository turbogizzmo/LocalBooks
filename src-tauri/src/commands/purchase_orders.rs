//! Purchase orders.

use super::prelude::*;

handlers![
    list_purchase_orders,
    get_purchase_order_lines,
    create_purchase_order,
    set_purchase_order_closed,
    bill_purchase_order,
];

#[tauri::command]
pub async fn list_purchase_orders(db: State<'_, Db>) -> Result<Vec<PurchaseOrderView>, String> {
    workflows::list_purchase_orders(&db.pool().await).await
}

#[tauri::command]
pub async fn get_purchase_order_lines(db: State<'_, Db>, id: i64) -> Result<Vec<PurchaseOrderLineView>, String> {
    workflows::purchase_order_lines(&db.pool().await, id).await
}

#[tauri::command]
pub async fn create_purchase_order(app: AppHandle, db: State<'_, Db>, input: PurchaseOrderInput) -> Result<i64, String> {
    let id = workflows::create_purchase_order(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn set_purchase_order_closed(app: AppHandle, db: State<'_, Db>, id: i64, closed: bool) -> Result<(), String> {
    workflows::set_purchase_order_closed(&db.pool().await, id, closed).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn bill_purchase_order(app: AppHandle, db: State<'_, Db>, id: i64, bill_date: NaiveDate, due_date: NaiveDate, bill_number: Option<String>) -> Result<i64, String> {
    let bill = workflows::bill_purchase_order(&db.pool().await, id, bill_date, due_date, bill_number).await?;
    notify(&app);
    Ok(bill)
}
