//! Accountant's Copy: making the copy, the accountant's changes file, and bringing it back.

use super::prelude::*;
use crate::accountants_copy::{self, ChangesFile, CopyRecord, ImportPreview};

handlers![
    get_accountants_copy_state,
    create_accountants_copy,
    cancel_accountants_copy,
    export_accountant_changes,
    preview_accountant_changes,
    import_accountant_changes,
];

/// Who is acting: the signed-in user's name, or "this computer" in a file without users.
async fn who(db: &Db) -> Result<String, String> {
    let (_, session) = signed_in(db).await?;
    Ok(session.as_ref().map_or_else(|| "this computer".to_string(), |s| s.user.display_name.clone()))
}

/// What the screen shows: the copies made from these books, and, when this file is itself an
/// Accountant's Copy, its dividing date.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyState {
    pub copies: Vec<CopyRecord>,
    pub this_copy_dividing_date: Option<NaiveDate>,
}

#[tauri::command]
pub async fn get_accountants_copy_state(db: State<'_, Db>) -> Result<CopyState, String> {
    let pool = db.pool().await;
    Ok(CopyState { copies: accountants_copy::copies(&pool).await?, this_copy_dividing_date: accountants_copy::copy_dividing_date(&pool).await? })
}

#[tauri::command]
pub async fn create_accountants_copy(app: AppHandle, db: State<'_, Db>, dividing_date: NaiveDate, destination: PathBuf) -> Result<CopyRecord, String> {
    let by = who(&db).await?;
    let record = accountants_copy::create(&db.pool().await, &db.path().await?, &destination, dividing_date, &by).await?;
    notify(&app);
    Ok(record)
}

#[tauri::command]
pub async fn cancel_accountants_copy(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let by = who(&db).await?;
    accountants_copy::cancel(&db.pool().await, &by).await?;
    notify(&app);
    Ok(())
}

/// In the copy: writes the accountant's changes for the client.
#[tauri::command]
pub async fn export_accountant_changes(db: State<'_, Db>, destination: PathBuf) -> Result<ChangesFile, String> {
    accountants_copy::export(&db.pool().await, &destination).await
}

/// Reads a changes file off the async runtime's threads.
async fn read_changes(path: PathBuf) -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("ACCOUNTANT_CHANGES_UNREADABLE: {e}"))
}

#[tauri::command]
pub async fn preview_accountant_changes(db: State<'_, Db>, path: PathBuf) -> Result<ImportPreview, String> {
    let bytes = read_changes(path).await?;
    accountants_copy::preview(&db.pool().await, &bytes).await
}

#[tauri::command]
pub async fn import_accountant_changes(app: AppHandle, db: State<'_, Db>, path: PathBuf) -> Result<CopyRecord, String> {
    let by = who(&db).await?;
    let bytes = read_changes(path).await?;
    let record = accountants_copy::import(&db.pool().await, &bytes, &by).await?;
    notify(&app);
    Ok(record)
}
