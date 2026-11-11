//! Company file backup and restore.

use super::prelude::*;
use super::company_files::company_changed;

handlers![
    export_company_file,
    inspect_company_file,
    import_company_file,
    list_backups,
    backup_now,
];

/// Writes a consistent snapshot of the live company file to `destination_path` while the app keeps running.
#[tauri::command]
pub async fn export_company_file(app_handle: AppHandle, destination_path: String) -> Result<BackupFile, String> {
    let db = app_handle.state::<Db>();
    let destination = PathBuf::from(destination_path);
    let (pool, live) = (db.pool().await, db.path().await?);
    backup::snapshot(&pool, &live, &destination).await?;
    // A copy on another drive, a network share or a synced folder survives this disk (gap R8).
    if crate::backup_safety::saved_copy(&pool, &destination, &live, crate::backup_safety::Source::BackUpTo).await? {
        notify(&app_handle);
    }
    backup::describe_export(&destination)
}

#[tauri::command]
pub async fn inspect_company_file(source_path: String) -> Result<CompanyFileInfo, String> {
    backup::inspect(&PathBuf::from(source_path)).await
}

/// Verifies `source_path` and replaces the live company file with it (keeping a safety snapshot).
#[tauri::command]
pub async fn import_company_file(app_handle: AppHandle, source_path: String) -> Result<RestoreResult, String> {
    let db = app_handle.state::<Db>();
    let source = PathBuf::from(source_path);
    // The window shows Opening <the backup>… meanwhile, and no other open runs (gap R28).
    let _turn = crate::opening::begin(&app_handle, &source, None).await?;
    let result = backup::restore(&db, &source).await?;
    company_changed(&app_handle).await;
    notify(&app_handle);
    Ok(result)
}

#[tauri::command]
pub async fn list_backups(db: State<'_, Db>) -> Result<Vec<BackupFile>, String> {
    backup::list_backups(db.backups_dir())
}

#[tauri::command]
pub async fn backup_now(db: State<'_, Db>) -> Result<BackupFile, String> {
    backup::backup_now(&db).await
}
