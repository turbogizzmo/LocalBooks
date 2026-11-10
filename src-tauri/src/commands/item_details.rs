//! The rest of the item record and list: purchase description and cost, making several items
//! active or inactive, and Add/Edit Multiple Items from pasted rows (`item_details.rs`).

use super::prelude::*;
use crate::item_details::{PasteResult, PastedItem};

handlers![set_item_purchase_details, set_items_active, paste_items];

#[tauri::command]
pub async fn set_item_purchase_details(app: AppHandle, db: State<'_, Db>, product_id: i64, description: Option<String>, cost_cents: Option<i64>) -> Result<(), String> {
    crate::item_details::set_purchase_details(&db.pool().await, product_id, description.as_deref(), cost_cents).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_items_active(app: AppHandle, db: State<'_, Db>, ids: Vec<i64>, active: bool) -> Result<usize, String> {
    let changed = crate::item_details::set_items_active(&db.pool().await, &ids, active).await?;
    notify(&app);
    Ok(changed)
}

#[tauri::command]
pub async fn paste_items(app: AppHandle, db: State<'_, Db>, rows: Vec<PastedItem>) -> Result<PasteResult, String> {
    let result = crate::item_details::paste_items(&db.pool().await, &rows).await?;
    if result.problems.is_empty() {
        notify(&app);
    }
    Ok(result)
}
