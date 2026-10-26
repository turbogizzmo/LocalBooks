//! Sales reps and shipping methods.

use super::prelude::*;

handlers![
    list_sales_reps,
    create_sales_rep,
    update_sales_rep,
    set_sales_rep_active,
    delete_sales_rep,
    list_ship_methods,
    create_ship_method,
    rename_ship_method,
    set_ship_method_active,
    delete_ship_method,
];

#[tauri::command]
pub async fn list_sales_reps(db: State<'_, Db>) -> Result<Vec<SalesRepView>, String> {
    sales_lists::list_sales_reps(&db.pool().await).await
}

#[tauri::command]
pub async fn create_sales_rep(app: AppHandle, db: State<'_, Db>, input: SalesRepInput) -> Result<SalesRepView, String> {
    let rep = sales_lists::create_sales_rep(&db.pool().await, input).await?;
    notify(&app);
    Ok(rep)
}

#[tauri::command]
pub async fn update_sales_rep(app: AppHandle, db: State<'_, Db>, id: i64, input: SalesRepInput) -> Result<SalesRepView, String> {
    let rep = sales_lists::update_sales_rep(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(rep)
}

#[tauri::command]
pub async fn set_sales_rep_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<SalesRepView, String> {
    let rep = sales_lists::set_sales_rep_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(rep)
}

#[tauri::command]
pub async fn delete_sales_rep(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    sales_lists::delete_sales_rep(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_ship_methods(db: State<'_, Db>) -> Result<Vec<ShipMethodView>, String> {
    sales_lists::list_ship_methods(&db.pool().await).await
}

#[tauri::command]
pub async fn create_ship_method(app: AppHandle, db: State<'_, Db>, name: String) -> Result<ShipMethodView, String> {
    let method = sales_lists::create_ship_method(&db.pool().await, &name).await?;
    notify(&app);
    Ok(method)
}

#[tauri::command]
pub async fn rename_ship_method(app: AppHandle, db: State<'_, Db>, id: i64, name: String) -> Result<ShipMethodView, String> {
    let method = sales_lists::rename_ship_method(&db.pool().await, id, &name).await?;
    notify(&app);
    Ok(method)
}

#[tauri::command]
pub async fn set_ship_method_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ShipMethodView, String> {
    let method = sales_lists::set_ship_method_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(method)
}

#[tauri::command]
pub async fn delete_ship_method(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    sales_lists::delete_ship_method(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
