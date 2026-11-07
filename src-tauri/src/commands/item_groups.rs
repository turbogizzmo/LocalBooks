//! Item groups: a named set of items that a sales form expands into its lines.

use super::prelude::*;
use crate::item_types::{GroupInput, ItemGroup};

handlers![list_item_groups, save_item_group, set_item_group_active];

#[tauri::command]
pub async fn list_item_groups(db: State<'_, Db>) -> Result<Vec<ItemGroup>, String> {
    crate::item_types::list_groups(&db.pool().await).await
}

/// Adds a group (`id` left out) or changes one.
#[tauri::command]
pub async fn save_item_group(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: GroupInput) -> Result<ItemGroup, String> {
    let group = crate::item_types::save_group(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(group)
}

#[tauri::command]
pub async fn set_item_group_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ItemGroup, String> {
    let group = crate::item_types::set_group_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(group)
}
