//! Employees and time.

use super::prelude::*;

handlers![
    list_employees,
    create_employee,
    create_time_activities,
    list_time_activities,
];

#[tauri::command]
pub async fn list_employees(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    timesheet::list_employees(&db.pool().await).await
}

#[tauri::command]
pub async fn create_employee(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let employee = timesheet::create_employee(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(employee)
}

#[tauri::command]
pub async fn create_time_activities(
    app: AppHandle,
    db: State<'_, Db>,
    inputs: Vec<TimeActivityInput>,
) -> Result<usize, String> {
    let count = timesheet::create_time_activities(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn list_time_activities(
    db: State<'_, Db>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<TimeActivityView>, String> {
    timesheet::list_time_activities(&db.pool().await, from, to).await
}
