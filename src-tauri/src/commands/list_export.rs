//! File › Utilities › Export (T26): lists to an IIF file, and addresses to a text file.

use super::prelude::*;

handlers![
    export_lists_iif,
    export_addresses_text,
];

/// Asks where to save and writes `text` there; `None` when the dialog is cancelled.
async fn save_as(app: &AppHandle, name: String, filter: (&'static str, &'static str), text: String) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&name).add_filter(filter.0, &[filter.1]).blocking_save_file())
        .await
        .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("EXPORT_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, text.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

/// Lists to IIF Files: the chart of accounts, customers and jobs, vendors, items and classes.
#[tauri::command]
pub async fn export_lists_iif(app: AppHandle, db: State<'_, Db>) -> Result<Option<String>, String> {
    let text = crate::list_export::lists_iif(&db.pool().await).await?;
    save_as(&app, "Lists.iif".into(), ("IIF file", "iif"), text).await
}

/// Addresses to Text File: every active customer's and vendor's name and address, tab-separated.
#[tauri::command]
pub async fn export_addresses_text(app: AppHandle, db: State<'_, Db>) -> Result<Option<String>, String> {
    let text = crate::list_export::addresses_text(&db.pool().await).await?;
    save_as(&app, "Addresses.txt".into(), ("Text file", "txt"), text).await
}
