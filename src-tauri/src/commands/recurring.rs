//! Recurring transactions.

use super::prelude::*;

handlers![
    list_recurring,
    get_recurring,
    save_recurring,
    set_recurring_active,
    delete_recurring,
    enter_recurring,
    edit_recurring_schedule,
];

/// A memorized transaction with the transaction it enters, for Edit transaction….
#[tauri::command]
pub async fn get_recurring(db: State<'_, Db>, id: i64) -> Result<workflows::RecurringTemplate, String> {
    workflows::get_recurring(&db.pool().await, id).await
}

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
pub async fn enter_recurring(app: AppHandle, db: State<'_, Db>, as_of: Option<NaiveDate>, only: Option<i64>) -> Result<workflows::EnteredDue, String> {
    let report = workflows::enter_due_report(&db.pool().await, as_of.unwrap_or_else(|| Local::now().date_naive()), only).await?;
    notify(&app);
    Ok(report)
}

/// Edit…: a memorized transaction's name and schedule.
#[tauri::command]
pub async fn edit_recurring_schedule(app: AppHandle, db: State<'_, Db>, id: i64, input: workflows::ScheduleInput) -> Result<(), String> {
    workflows::edit_recurring_schedule(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(())
}
