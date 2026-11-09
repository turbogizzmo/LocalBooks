//! The information panel beside lists and on forms (T56).

use super::prelude::*;
use crate::info_panel::{self, ItemSummary, PartySummary};

handlers![get_party_summary, get_item_summary];

/// Summary figures for a customer or vendor (`kind` = `customer` or `vendor`).
#[tauri::command]
pub async fn get_party_summary(db: State<'_, Db>, kind: String, id: i64) -> Result<PartySummary, String> {
    info_panel::party_summary(&db.pool().await, &kind, id).await
}

/// Stock figures for a product or service, as of today.
#[tauri::command]
pub async fn get_item_summary(db: State<'_, Db>, product_id: i64) -> Result<ItemSummary, String> {
    info_panel::item_summary(&db.pool().await, product_id, Local::now().date_naive()).await
}
