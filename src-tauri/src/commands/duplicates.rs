//! Duplicate Transaction for the forms that show saved documents read-only (T28).

use super::prelude::*;
use crate::duplicates::{self, CopySource};

handlers![
    get_copy_source,
];

/// A saved sales receipt, estimate, credit memo, vendor credit, purchase order, deposit or transfer,
/// read as its form loads it for Duplicate.
#[tauri::command]
pub async fn get_copy_source(db: State<'_, Db>, kind: String, id: i64) -> Result<CopySource, String> {
    duplicates::copy_source(&db.pool().await, &kind, id).await
}
