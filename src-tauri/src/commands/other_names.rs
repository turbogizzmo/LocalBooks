//! The Other Names list: people and businesses who are neither customers nor vendors (`other_names.rs`).

use super::prelude::*;
use crate::other_names::{OtherName, OtherNameInput};

handlers![list_other_names, get_other_name, create_other_name, update_other_name, set_other_name_active];

#[tauri::command]
pub async fn list_other_names(db: State<'_, Db>) -> Result<Vec<OtherName>, String> {
    crate::other_names::list(&db.pool().await).await
}

#[tauri::command]
pub async fn get_other_name(db: State<'_, Db>, id: i64) -> Result<OtherName, String> {
    crate::other_names::get(&db.pool().await, id).await
}

#[tauri::command]
pub async fn create_other_name(app: AppHandle, db: State<'_, Db>, input: OtherNameInput) -> Result<OtherName, String> {
    let made = crate::other_names::create(&db.pool().await, &input).await?;
    notify(&app);
    Ok(made)
}

#[tauri::command]
pub async fn update_other_name(app: AppHandle, db: State<'_, Db>, id: i64, input: OtherNameInput) -> Result<OtherName, String> {
    let saved = crate::other_names::update(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn set_other_name_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<OtherName, String> {
    let saved = crate::other_names::set_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(saved)
}
