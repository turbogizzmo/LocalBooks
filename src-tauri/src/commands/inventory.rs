//! Inventory.

use super::prelude::*;

handlers![
    list_inventory_items,
    track_inventory,
    adjust_inventory,
    list_stock_movements,
];

#[tauri::command]
pub async fn list_inventory_items(db: State<'_, Db>) -> Result<Vec<InventoryItemView>, String> {
    inventory::list_items(&db.pool().await).await
}

#[tauri::command]
pub async fn track_inventory(app: AppHandle, db: State<'_, Db>, input: TrackInput) -> Result<InventoryItemView, String> {
    let item = inventory::track_product(&db.pool().await, &input).await?;
    notify(&app);
    Ok(item)
}

#[tauri::command]
pub async fn adjust_inventory(app: AppHandle, db: State<'_, Db>, input: AdjustInput) -> Result<InventoryItemView, String> {
    let item = inventory::adjust(&db.pool().await, &input).await?;
    notify(&app);
    Ok(item)
}

#[tauri::command]
pub async fn list_stock_movements(db: State<'_, Db>, item_id: i64) -> Result<Vec<StockMovementView>, String> {
    inventory::movements(&db.pool().await, item_id).await
}
