//! Backup safety (gap R8): a copy off this disk and a test restore.

use super::prelude::*;
use crate::backup::CompanyFileInfo;
use crate::backup_safety::{self, Safety};

handlers![get_backup_safety, test_restore_backup];

#[tauri::command]
pub async fn get_backup_safety(db: State<'_, Db>) -> Result<Safety, String> {
    backup_safety::status(&db.pool().await).await
}

/// Opens a backup read-only and checks it as a restore would, without replacing the books.
#[tauri::command]
pub async fn test_restore_backup(app: AppHandle, db: State<'_, Db>, path: String) -> Result<CompanyFileInfo, String> {
    let pool = if db.is_open().await { Some(db.pool().await) } else { None };
    let info = backup_safety::test_restore(pool.as_ref(), &PathBuf::from(path)).await?;
    notify(&app);
    Ok(info)
}
