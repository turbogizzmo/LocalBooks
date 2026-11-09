//! Lists in a custom order (`list_order.rs`): the chart of accounts, items and customers.

use super::prelude::*;
use crate::list_order::{self, OrderedList};

handlers![
    get_list_order,
    set_list_order,
    reset_list_order,
];

/// The ids with a place in the list's custom order, first to last.
#[tauri::command]
pub async fn get_list_order(db: State<'_, Db>, list: OrderedList) -> Result<Vec<i64>, String> {
    list_order::get(&db.pool().await, list).await
}

/// Keeps `ids` as the list's order (after a drag); returns the order kept.
#[tauri::command]
pub async fn set_list_order(app: AppHandle, db: State<'_, Db>, list: OrderedList, ids: Vec<i64>) -> Result<Vec<i64>, String> {
    let kept = list_order::set(&db.pool().await, list, &ids).await?;
    notify(&app);
    Ok(kept)
}

/// Puts the list back in its usual order.
#[tauri::command]
pub async fn reset_list_order(app: AppHandle, db: State<'_, Db>, list: OrderedList) -> Result<(), String> {
    list_order::reset(&db.pool().await, list).await?;
    notify(&app);
    Ok(())
}
