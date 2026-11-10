//! Item categories: the Category List and moving items between categories (`item_categories.rs`).

use super::prelude::*;
use crate::item_categories::{Category, CategoryInput};

handlers![
    list_item_categories,
    create_item_category,
    update_item_category,
    set_item_category_active,
    delete_item_category,
    move_items_to_category,
];

#[tauri::command]
pub async fn list_item_categories(db: State<'_, Db>) -> Result<Vec<Category>, String> {
    crate::item_categories::list_categories(&db.pool().await).await
}

#[tauri::command]
pub async fn create_item_category(app: AppHandle, db: State<'_, Db>, input: CategoryInput) -> Result<Category, String> {
    let category = crate::item_categories::create_category(&db.pool().await, input).await?;
    notify(&app);
    Ok(category)
}

#[tauri::command]
pub async fn update_item_category(app: AppHandle, db: State<'_, Db>, id: i64, input: CategoryInput) -> Result<Category, String> {
    let category = crate::item_categories::update_category(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(category)
}

#[tauri::command]
pub async fn set_item_category_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<Category, String> {
    let category = crate::item_categories::set_category_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(category)
}

#[tauri::command]
pub async fn delete_item_category(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    crate::item_categories::delete_category(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn move_items_to_category(app: AppHandle, db: State<'_, Db>, item_ids: Vec<i64>, category_id: Option<i64>) -> Result<usize, String> {
    let moved = crate::item_categories::move_items(&db.pool().await, &item_ids, category_id).await?;
    notify(&app);
    Ok(moved)
}
