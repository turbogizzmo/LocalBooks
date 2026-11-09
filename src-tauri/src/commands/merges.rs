//! Merging two customers, vendors, items or accounts.

use super::prelude::*;
use crate::merges::{self, MergeKind, MergeResult};

handlers![
    find_merge_target,
    merge_names,
];

/// The other entry named `name`, which renaming `id` to it would merge into: `[id, name]`.
#[tauri::command]
pub async fn find_merge_target(db: State<'_, Db>, kind: String, id: i64, name: String) -> Result<Option<(i64, String)>, String> {
    merges::merge_target(&db.pool().await, MergeKind::parse(&kind)?, id, &name).await
}

/// Merges `from_id` into `to_id` (`kind` is CUSTOMER, VENDOR, ITEM or ACCOUNT); it can't be undone.
#[tauri::command]
pub async fn merge_names(app: AppHandle, db: State<'_, Db>, kind: String, from_id: i64, to_id: i64) -> Result<MergeResult, String> {
    let result = merges::merge(&db.pool().await, MergeKind::parse(&kind)?, from_id, to_id, Local::now().date_naive()).await?;
    notify(&app);
    Ok(result)
}
