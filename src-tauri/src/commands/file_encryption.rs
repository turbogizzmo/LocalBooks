//! Whole-file encryption of the company file (gap R1): Settings › Users & Passwords, Admins only.

use super::prelude::*;
use super::company_files::COMPANY_FILE_CHANGED;
use crate::file_cipher::whole_file::{self, EncryptInput};

handlers![
    encrypt_company_file,
    decrypt_company_file,
    list_unencrypted_backups,
    delete_unencrypted_backups,
];

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedOn {
    pub user: UserView,
    /// The new recovery key, shown once; LocalBooks keeps no copy.
    pub recovery_key: String,
}

#[tauri::command]
pub async fn encrypt_company_file(db: State<'_, Db>, input: EncryptInput) -> Result<EncryptedOn, String> {
    let (_, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let admin = Session::require_admin(guard.as_ref())?;
    let (session, recovery) = whole_file::encrypt(&db, admin, input).await?;
    let user = session.user.clone();
    drop(guard);
    // The swap gave the file a fresh session slot: the Admin is signed in on the encrypted file.
    let (_, fresh) = db.pool_and_session().await?;
    *fresh.lock().await = Some(session);
    db.refresh_gate().await;
    // No reload here: the window shows the new recovery key first.
    Ok(EncryptedOn { user, recovery_key: recovery.as_str().to_string() })
}

#[tauri::command]
pub async fn decrypt_company_file(app: AppHandle, db: State<'_, Db>, password: String) -> Result<UserView, String> {
    let (_, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let admin = Session::require_admin(guard.as_ref())?;
    let session = whole_file::decrypt(&db, admin, &password).await?;
    drop(guard);
    let user = session.user.clone();
    let (_, slot) = db.pool_and_session().await?;
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    Ok(user)
}

#[tauri::command]
pub async fn list_unencrypted_backups(db: State<'_, Db>) -> Result<Vec<crate::backup::BackupFile>, String> {
    if !db.is_encrypted().await {
        return Ok(Vec::new());
    }
    whole_file::unencrypted_backups(db.backups_dir())
}

/// Deletes the backups made before encryption (they hold the books in plain). How many went.
#[tauri::command]
pub async fn delete_unencrypted_backups(db: State<'_, Db>) -> Result<usize, String> {
    if !db.is_encrypted().await {
        return Err("DECRYPT_NOT_ON: this company file isn't encrypted".into());
    }
    let mut deleted = 0;
    for backup in whole_file::unencrypted_backups(db.backups_dir())? {
        if std::fs::remove_file(&backup.path).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}
