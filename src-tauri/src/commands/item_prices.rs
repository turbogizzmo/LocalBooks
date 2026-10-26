//! Change Item Prices.

use super::prelude::*;

handlers![
    list_price_items,
    preview_item_prices,
    apply_item_prices,
    list_price_changes,
];

/// Change Item Prices: products and services with their price and unit cost.
#[tauri::command]
pub async fn list_price_items(db: State<'_, Db>) -> Result<Vec<crate::item_prices::PriceItem>, String> {
    crate::item_prices::list_items(&db.pool().await).await
}

#[tauri::command]
pub async fn preview_item_prices(db: State<'_, Db>, input: crate::item_prices::PriceChangeInput) -> Result<Vec<crate::item_prices::PriceChange>, String> {
    crate::item_prices::preview(&db.pool().await, &input).await
}

#[tauri::command]
pub async fn apply_item_prices(app: AppHandle, db: State<'_, Db>, input: crate::item_prices::PriceChangeInput) -> Result<crate::item_prices::AppliedPrices, String> {
    let applied = crate::item_prices::apply(&db.pool().await, &input).await?;
    notify(&app);
    Ok(applied)
}

#[tauri::command]
pub async fn list_price_changes(db: State<'_, Db>) -> Result<Vec<crate::item_prices::PriceChangeRecord>, String> {
    crate::item_prices::history(&db.pool().await, 50).await
}
