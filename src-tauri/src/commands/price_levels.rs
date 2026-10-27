//! Price levels and the Item Price List.

use super::prelude::*;

handlers![
    list_price_levels,
    preview_price_level,
    save_price_level,
    set_price_level_active,
    delete_price_level,
    get_item_price_list,
    list_price_level_changes,
];

#[tauri::command]
pub async fn list_price_levels(db: State<'_, Db>) -> Result<Vec<crate::price_levels::PriceLevel>, String> {
    crate::price_levels::list(&db.pool().await).await
}

/// Each item's price under an unsaved level.
#[tauri::command]
pub async fn preview_price_level(db: State<'_, Db>, input: crate::price_levels::PriceLevelInput) -> Result<Vec<crate::price_levels::ItemPreview>, String> {
    crate::price_levels::preview(&db.pool().await, &input).await
}

/// Creates a price level (`id` left out) or changes one; recorded in the audit trail.
#[tauri::command]
pub async fn save_price_level(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: crate::price_levels::PriceLevelInput) -> Result<crate::price_levels::PriceLevel, String> {
    let level = crate::price_levels::save(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(level)
}

#[tauri::command]
pub async fn set_price_level_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<crate::price_levels::PriceLevel, String> {
    let level = crate::price_levels::set_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(level)
}

#[tauri::command]
pub async fn delete_price_level(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    crate::price_levels::delete(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Item Price List: each item's standard price and its price at every level, plus each customer's
/// default level (the sales forms fill in rates from it).
#[tauri::command]
pub async fn get_item_price_list(db: State<'_, Db>) -> Result<crate::price_levels::PriceList, String> {
    crate::price_levels::price_list(&db.pool().await).await
}

#[tauri::command]
pub async fn list_price_level_changes(db: State<'_, Db>) -> Result<Vec<crate::item_prices::PriceChangeRecord>, String> {
    crate::price_levels::history(&db.pool().await, 50).await
}
