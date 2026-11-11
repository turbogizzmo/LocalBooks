//! Purchase orders.

use super::prelude::*;

handlers![
    list_purchase_orders,
    get_purchase_order_lines,
    create_purchase_order,
    update_purchase_order,
    delete_purchase_order,
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
pub async fn create_purchase_order(app: AppHandle, db: State<'_, Db>, input: PurchaseOrderInput, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    crate::sales::require_listed(&pool, crate::sales::Party::Vendor, &input.vendor_name).await?;
    let total = workflows::validate_purchase_order(&input)?;
    crate::approvals::check_not_held(&pool, crate::approvals::Kind::PurchaseOrder, &input.vendor_name, total).await?;
    guards::check(&pool, guards::dated(input.po_date), confirmed.as_deref()).await?;
    let id = workflows::create_purchase_order(&pool, input).await?;
    notify(&app);
    Ok(id)
}

/// Saves changes to an open purchase order, with the same approval rules and date warnings as a new one.
#[tauri::command]
pub async fn update_purchase_order(app: AppHandle, db: State<'_, Db>, id: i64, input: PurchaseOrderInput, confirmed: Option<Vec<String>>) -> Result<(), String> {
    let pool = db.pool().await;
    let total = workflows::validate_purchase_order(&input)?;
    crate::approvals::check_not_held(&pool, crate::approvals::Kind::PurchaseOrder, &input.vendor_name, total).await?;
    guards::check(&pool, guards::dated(input.po_date), confirmed.as_deref()).await?;
    workflows::update_purchase_order(&pool, id, input).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_purchase_order(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    workflows::delete_purchase_order(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_purchase_order_closed(app: AppHandle, db: State<'_, Db>, id: i64, closed: bool) -> Result<(), String> {
    workflows::set_purchase_order_closed(&db.pool().await, id, closed).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn bill_purchase_order(
    app: AppHandle,
    db: State<'_, Db>,
    id: i64,
    bill_date: NaiveDate,
    due_date: NaiveDate,
    bill_number: Option<String>,
    confirmed: Option<Vec<String>>,
) -> Result<i64, String> {
    let pool = db.pool().await;
    // The same checks as Enter Bills: approval rules, then the duplicate bill number, date and
    // retained-earnings warnings.
    let input = workflows::purchase_order_bill(&pool, id, bill_date, due_date, bill_number).await?;
    crate::approvals::check_not_held(&pool, crate::approvals::Kind::Bill, &input.vendor_name, input.total_cents.get()).await?;
    guards::check(&pool, guards::bill(&input, None), confirmed.as_deref()).await?;
    let bill = workflows::post_purchase_order_bill(&pool, id, &input).await?;
    notify(&app);
    Ok(bill)
}
