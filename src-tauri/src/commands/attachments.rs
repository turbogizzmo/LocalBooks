//! Attachments.

use super::prelude::*;

handlers![
    add_attachment,
    list_attachments,
    attachment_counts,
    save_attachment,
    delete_attachment,
];

/// Attaches a file chosen in the open dialog (read here, so the page never handles file bytes).
#[tauri::command]
pub async fn add_attachment(app: AppHandle, db: State<'_, Db>, journal_entry_id: i64, path: String) -> Result<AttachmentView, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    if meta.len() as usize > workflows::MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let content = std::fs::read(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    let saved = workflows::add_attachment(&db.pool().await, journal_entry_id, &path, &content).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn list_attachments(db: State<'_, Db>, journal_entry_id: i64) -> Result<Vec<AttachmentView>, String> {
    workflows::list_attachments(&db.pool().await, journal_entry_id).await
}

#[tauri::command]
pub async fn attachment_counts(db: State<'_, Db>) -> Result<std::collections::HashMap<i64, i64>, String> {
    workflows::attachment_counts(&db.pool().await).await
}

/// Saves a copy of an attachment where the user chooses.
#[tauri::command]
pub async fn save_attachment(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<Option<String>, String> {
    let (name, content) = workflows::attachment_content(&db.pool().await, id).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&name).blocking_save_file())
        .await
        .map_err(|e| format!("ATTACH_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("ATTACH_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &content)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn delete_attachment(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    workflows::delete_attachment(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
