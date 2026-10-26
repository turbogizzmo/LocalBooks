//! Recurring transactions.

use super::prelude::*;

handlers![
    list_recurring,
    save_recurring,
    set_recurring_active,
    delete_recurring,
    enter_recurring,
];

#[tauri::command]
pub async fn list_recurring(db: State<'_, Db>) -> Result<Vec<RecurringView>, String> {
    workflows::list_recurring(&db.pool().await).await
}

#[tauri::command]
pub async fn save_recurring(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: RecurringInput) -> Result<i64, String> {
    let id = workflows::save_recurring(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn set_recurring_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<(), String> {
    workflows::set_recurring_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_recurring(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    workflows::delete_recurring(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn enter_recurring(app: AppHandle, db: State<'_, Db>, as_of: Option<NaiveDate>, only: Option<i64>) -> Result<usize, String> {
    let entered = workflows::enter_due(&db.pool().await, as_of.unwrap_or_else(|| Local::now().date_naive()), only).await?;
    notify(&app);
    Ok(entered)
}
