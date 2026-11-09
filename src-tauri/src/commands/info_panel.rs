//! The information panel beside lists and on forms (T56).

use super::prelude::*;
use crate::info_panel::{self, ItemSummary, PartySummary};

handlers![get_party_summary, get_item_summary, list_customer_emails, set_party_notes, set_item_purchasing];

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

/// What was emailed to a customer, from every document (the information panel's Emails list).
#[tauri::command]
pub async fn list_customer_emails(db: State<'_, Db>, customer_id: i64) -> Result<Vec<crate::email_log::SentToCustomer>, String> {
    crate::email_log::sent_to_customer(&db.pool().await, customer_id).await
}

/// Saves a customer's or vendor's notes from the information panel.
#[tauri::command]
pub async fn set_party_notes(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, notes: String) -> Result<(), String> {
    info_panel::set_notes(&db.pool().await, &kind, id, &notes).await?;
    notify(&app);
    Ok(())
}

/// Saves an item's reorder point and preferred vendor (its record's Purchasing fields).
#[tauri::command]
pub async fn set_item_purchasing(app: AppHandle, db: State<'_, Db>, product_id: i64, reorder_point_e4: Option<i64>, preferred_vendor_id: Option<i64>) -> Result<(), String> {
    info_panel::set_item_purchasing(&db.pool().await, product_id, reorder_point_e4, preferred_vendor_id).await?;
    notify(&app);
    Ok(())
}
