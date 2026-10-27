//! The To Do list and Reminders.

use super::prelude::*;
use crate::reminders::{self, Assignee, LinkOption, ReminderGroup, ReminderSettings, TodoInput, TodoView, Viewer};

handlers![
    get_reminders,
    get_reminder_settings,
    save_reminder_settings,
    list_todos,
    list_todo_assignees,
    list_todo_links,
    save_todo,
    set_todo_done,
    delete_todo,
];

/// The signed-in user (nobody in a file without users) and the open file's pool.
async fn viewer(db: &Db) -> Result<(sqlx::SqlitePool, Viewer), String> {
    let (pool, slot) = db.pool_and_session().await?;
    let viewer = slot.lock().await.as_ref().map(|s| Viewer { user_id: Some(s.user.id), role: Some(s.user.role) }).unwrap_or_default();
    Ok((pool, viewer))
}

/// The day of the newest snapshot in the backups folder, if any.
fn newest_backup(db: &Db) -> Option<NaiveDate> {
    let newest = backup::list_backups(db.backups_dir()).ok()?.into_iter().next()?;
    // `modified` is a local time, `YYYY-MM-DDTHH:MM:SS`; its first ten characters are the day.
    NaiveDate::parse_from_str(newest.modified.get(..10)?, "%Y-%m-%d").ok()
}

/// What needs attention today, by the signed-in user's reminder settings.
#[tauri::command]
pub async fn get_reminders(db: State<'_, Db>) -> Result<Vec<ReminderGroup>, String> {
    let (pool, viewer) = viewer(&db).await?;
    reminders::reminders(&pool, Local::now().date_naive(), viewer, newest_backup(&db)).await
}

#[tauri::command]
pub async fn get_reminder_settings(db: State<'_, Db>) -> Result<ReminderSettings, String> {
    let (pool, viewer) = viewer(&db).await?;
    reminders::settings(&pool, viewer).await
}

/// The signed-in user's own settings, so every role may save them (see `permissions.rs`).
#[tauri::command]
pub async fn save_reminder_settings(app: AppHandle, db: State<'_, Db>, settings: ReminderSettings) -> Result<ReminderSettings, String> {
    let (pool, viewer) = viewer(&db).await?;
    let saved = reminders::save_settings(&pool, viewer, &settings).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn list_todos(db: State<'_, Db>) -> Result<Vec<TodoView>, String> {
    reminders::list_todos(&db.pool().await).await
}

#[tauri::command]
pub async fn list_todo_assignees(db: State<'_, Db>) -> Result<Vec<Assignee>, String> {
    reminders::assignees(&db.pool().await).await
}

#[tauri::command]
pub async fn list_todo_links(db: State<'_, Db>, customer_id: Option<i64>, vendor_id: Option<i64>) -> Result<Vec<LinkOption>, String> {
    reminders::link_options(&db.pool().await, customer_id, vendor_id).await
}

#[tauri::command]
pub async fn save_todo(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: TodoInput) -> Result<TodoView, String> {
    let (pool, viewer) = viewer(&db).await?;
    let saved = reminders::save_todo(&pool, id, &input, viewer).await?;
    notify(&app);
    Ok(saved)
}

/// Open to every role; a View-only user is held to their own to-dos in [`reminders::set_done`].
#[tauri::command]
pub async fn set_todo_done(app: AppHandle, db: State<'_, Db>, id: i64, done: bool) -> Result<TodoView, String> {
    let (pool, viewer) = viewer(&db).await?;
    let saved = reminders::set_done(&pool, id, done, viewer).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn delete_todo(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    reminders::delete_todo(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
