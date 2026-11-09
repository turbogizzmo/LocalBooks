//! The attachments library and files on records other than transactions (T58).

use base64::Engine;
use serde::Serialize;

use super::prelude::*;
use crate::attachments_library::{self as library, FileRef, LibraryFile, RecordFile};

handlers![
    add_record_attachment,
    list_record_attachments,
    list_attachment_library,
    get_attachment_preview,
    save_library_attachment,
    set_attachment_details,
    delete_library_attachment,
    restore_attachment,
    purge_attachment,
];

/// Attaches a file chosen in the open dialog to a customer, vendor, item, account, estimate,
/// purchase order or reconciliation (read here, so the page never handles file bytes).
#[tauri::command]
pub async fn add_record_attachment(app: AppHandle, db: State<'_, Db>, record_kind: String, record_id: i64, path: String) -> Result<RecordFile, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    if meta.len() as usize > workflows::MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let content = std::fs::read(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    let saved = library::add_record_file(&db.pool().await, &record_kind, record_id, &path, &content).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn list_record_attachments(db: State<'_, Db>, record_kind: String, record_id: i64) -> Result<Vec<RecordFile>, String> {
    library::record_files(&db.pool().await, &record_kind, record_id).await
}

/// Every attachment, newest first, matching `query`; deleted ones too when asked.
#[tauri::command]
pub async fn list_attachment_library(db: State<'_, Db>, query: Option<String>, include_deleted: bool) -> Result<Vec<LibraryFile>, String> {
    library::library(&db.pool().await, query.as_deref(), include_deleted).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPreview {
    pub file_name: String,
    pub media_type: String,
    /// The bytes the preview pane shows, base64: a picture whole, a text file's first
    /// [`TEXT_PREVIEW_BYTES`]; `None` for every other kind (the pane offers Save a copy instead).
    pub data_base64: Option<String>,
}

/// How much of a text file the preview pane shows.
const TEXT_PREVIEW_BYTES: i64 = 200_000;

/// What the library's preview pane shows: a picture, or the start of a text file. Nothing else
/// is read from the company file or crosses to the page, so selecting a large PDF or spreadsheet
/// moves no bytes.
#[tauri::command]
pub async fn get_attachment_preview(db: State<'_, Db>, file: FileRef) -> Result<AttachmentPreview, String> {
    let (file_name, media_type, shown) = library::preview(&db.pool().await, file, TEXT_PREVIEW_BYTES).await?;
    let data_base64 = shown.map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes));
    Ok(AttachmentPreview { file_name, media_type, data_base64 })
}

/// Saves a copy of any attachment where the user chooses.
#[tauri::command]
pub async fn save_library_attachment(app: AppHandle, db: State<'_, Db>, file: FileRef) -> Result<Option<String>, String> {
    let (name, _, content) = library::content(&db.pool().await, file).await?;
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
pub async fn set_attachment_details(app: AppHandle, db: State<'_, Db>, file: FileRef, keywords: Option<String>, send_with_form: bool) -> Result<(), String> {
    library::set_details(&db.pool().await, file, keywords.as_deref(), send_with_form).await?;
    notify(&app);
    Ok(())
}

/// Deletes an attachment reversibly (the library can restore it).
#[tauri::command]
pub async fn delete_library_attachment(app: AppHandle, db: State<'_, Db>, file: FileRef) -> Result<(), String> {
    library::delete(&db.pool().await, file).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn restore_attachment(app: AppHandle, db: State<'_, Db>, file: FileRef) -> Result<(), String> {
    library::restore(&db.pool().await, file).await?;
    notify(&app);
    Ok(())
}

/// Removes a deleted attachment for good (the screen asks first).
#[tauri::command]
pub async fn purge_attachment(app: AppHandle, db: State<'_, Db>, file: FileRef) -> Result<(), String> {
    library::purge(&db.pool().await, file).await?;
    notify(&app);
    Ok(())
}
