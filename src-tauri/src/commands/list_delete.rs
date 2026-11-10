//! Edit › Delete on the customer, vendor and item lists (`list_delete.rs`).

use super::prelude::*;

handlers![delete_list_record];

#[tauri::command]
pub async fn delete_list_record(app: AppHandle, db: State<'_, Db>, kind: String, id: i64) -> Result<String, String> {
    let name = crate::list_delete::delete_record(&db.pool().await, &kind, id).await?;
    notify(&app);
    Ok(name)
}
