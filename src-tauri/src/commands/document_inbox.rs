//! The document inbox: receipts and bills waiting to be entered.

use super::prelude::*;
use crate::document_inbox::{self, InboxDocument};

handlers![
    list_inbox_documents,
    add_inbox_documents,
    get_inbox_document_content,
    set_inbox_document_note,
    remove_inbox_document,
    attach_inbox_document,
];

#[tauri::command]
pub async fn list_inbox_documents(db: State<'_, Db>) -> Result<Vec<InboxDocument>, String> {
    document_inbox::list(&db.pool().await).await
}

/// Puts files (picked or dropped; read here, so the page never handles file bytes) in the inbox.
#[tauri::command]
pub async fn add_inbox_documents(app: AppHandle, db: State<'_, Db>, paths: Vec<String>) -> Result<Vec<InboxDocument>, String> {
    let pool = db.pool().await;
    // Every file is checked and read before any is added, so a bad one adds none.
    let mut files = Vec::with_capacity(paths.len());
    for path in &paths {
        let meta = std::fs::metadata(path).map_err(|e| format!("INBOX_READ: cannot read {path}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("INBOX_READ: {path} is not a file"));
        }
        if meta.len() as usize > workflows::MAX_ATTACHMENT_BYTES {
            return Err(format!("INBOX_SIZE: {path} is larger than 25 MB"));
        }
        files.push((path, std::fs::read(path).map_err(|e| format!("INBOX_READ: cannot read {path}: {e}"))?));
    }
    let mut added = Vec::with_capacity(files.len());
    for (path, content) in &files {
        match document_inbox::add(&pool, path, content).await {
            Ok(doc) => added.push(doc),
            Err(err) => {
                // The ones already added stay, so the screens refresh to show them.
                notify(&app);
                return Err(if added.is_empty() { err } else { format!("{err} ({} added before it)", added.len()) });
            }
        }
    }
    notify(&app);
    Ok(added)
}

/// The file itself, for the preview, as raw bytes (an ArrayBuffer on the page).
#[tauri::command]
pub async fn get_inbox_document_content(db: State<'_, Db>, id: i64) -> Result<tauri::ipc::Response, String> {
    let (_, _, bytes) = document_inbox::content(&db.pool().await, id).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn set_inbox_document_note(app: AppHandle, db: State<'_, Db>, id: i64, note: Option<String>) -> Result<InboxDocument, String> {
    let saved = document_inbox::set_note(&db.pool().await, id, note.as_deref()).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn remove_inbox_document(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    document_inbox::remove(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Attaches a waiting document to the transaction it was entered as, and takes it out of the inbox.
#[tauri::command]
pub async fn attach_inbox_document(app: AppHandle, db: State<'_, Db>, id: i64, journal_entry_id: i64) -> Result<AttachmentView, String> {
    let saved = document_inbox::attach(&db.pool().await, id, journal_entry_id).await?;
    notify(&app);
    Ok(saved)
}
