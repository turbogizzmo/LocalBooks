//! Offsite backup: encrypted copies in a folder the user picks, and restoring from them.

use super::prelude::*;
use crate::offsite::{self, OffsiteCopy, OffsiteSettings, OffsiteStatus};

handlers![get_offsite_status, set_up_offsite_backup, turn_off_offsite_backup, offsite_backup_now, test_offsite_restore, restore_offsite_backup];

pub fn offsite_secrets(app: &AppHandle) -> Result<SecretStore, String> {
    Ok(offsite::secret_store(&crate::data_dirs(app).map_err(|e| format!("OFFSITE_CONFIG: {e}"))?.1))
}

#[tauri::command]
pub async fn get_offsite_status(app: AppHandle, db: State<'_, Db>) -> Result<OffsiteStatus, String> {
    offsite::status(&db.pool().await, &offsite_secrets(&app)?).await
}

#[tauri::command]
pub async fn set_up_offsite_backup(app: AppHandle, db: State<'_, Db>, folder: PathBuf, password: String, keep: i64) -> Result<OffsiteSettings, String> {
    let password = zeroize::Zeroizing::new(password);
    offsite::set_up(&db.pool().await, &offsite_secrets(&app)?, &folder, &password, keep).await
}

#[tauri::command]
pub async fn turn_off_offsite_backup(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    offsite::turn_off(&db.pool().await, &offsite_secrets(&app)?).await
}

#[tauri::command]
pub async fn offsite_backup_now(app: AppHandle, db: State<'_, Db>) -> Result<OffsiteCopy, String> {
    offsite::back_up_now(db.inner(), &offsite_secrets(&app)?).await
}

/// Decrypts and checks a copy without changing the books.
#[tauri::command]
pub async fn test_offsite_restore(db: State<'_, Db>, path: PathBuf, password: String) -> Result<CompanyFileInfo, String> {
    let password = zeroize::Zeroizing::new(password);
    offsite::test_restore(db.inner(), &path, &password).await
}

#[tauri::command]
pub async fn restore_offsite_backup(app: AppHandle, db: State<'_, Db>, path: PathBuf, password: String) -> Result<RestoreResult, String> {
    let password = zeroize::Zeroizing::new(password);
    let restored = offsite::restore(db.inner(), &path, &password).await?;
    super::company_changed(&app).await;
    notify(&app);
    Ok(restored)
}
