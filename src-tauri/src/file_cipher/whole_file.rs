//! Turning whole-file encryption on and off (gap R1, phase 3b). An Admin's choice, off by default,
//! in Settings › Users & Passwords; the project owner decided it must be opt-in with a plain warning,
//! because with it on, losing every Admin password and the recovery key loses the books.
//!
//! **On** ([`encrypt`]):
//! 1. checks the Admin's password, and that every other active user has a temporary password;
//! 2. exports the live books into a new encrypted file beside them, under a new random page key,
//!    with a key header holding a slot for the Admin, one per other user and one for a **new
//!    recovery key**;
//! 3. checks the copy (`integrity_check`, and the same number of rows in every table as the live
//!    books), then gives the other users their temporary passwords and replaces the recovery key
//!    **in the copy**, so the live books are untouched until the swap;
//! 4. swaps the copy in (as a restore does, with a plain safety copy to fall back on), deletes that
//!    safety copy, and signs the Admin back in.
//!
//! **Off** ([`decrypt`]) is the reverse: export to a plain file, check, swap.
//!
//! Older backups made before encryption stay plain; [`unencrypted_backups`] lists them so the
//! Admin can delete them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::{ConnectOptions, Connection};

use super::header::Header;
use super::{encrypt_into, keyring, keys, open_options};
use crate::db::Db;
use crate::users::{self, Session, UserUpdate};

/// What turning encryption on needs besides the signed-in Admin.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptInput {
    /// The Admin's own password, again: it is what will open the file.
    pub password: String,
    /// A temporary password for each other active user (their key-header slot needs one).
    pub temporary_passwords: Vec<TemporaryPassword>,
    /// The Admin ticked that losing every Admin password and the recovery key loses the books.
    pub understood: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryPassword {
    pub user_id: i64,
    pub password: String,
}

/// The swap refused because the books changed while the copy was made (someone saved in another
/// window): said in terms of encryption rather than Condense's.
fn changed_while_copying(e: String) -> String {
    if e.starts_with("CONDENSE_BOOKS_CHANGED") {
        "BOOKS_CHANGED: the books changed while the copy was being made, so nothing was changed; try again when nobody is entering anything".into()
    } else {
        e
    }
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn beside(path: &Path, what: &str) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    dir.join(format!(".localbooks-{what}-{}.db", std::process::id()))
}

fn remove_with_sidecars(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
}

/// Checks an exported copy (opened with `options`): it passes `integrity_check` and holds exactly
/// the rows the books held when it was made (Condense's content digest, every row of every table).
async fn check_copy(expected_digest: &str, options: sqlx::sqlite::SqliteConnectOptions) -> Result<(), String> {
    let copy = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(options).await.map_err(|e| e.to_string())?;
    let checked = async {
        let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&copy).await.map_err(|e| e.to_string())?;
        if check != "ok" {
            return Err(format!("the copy failed its integrity check ({check})"));
        }
        if crate::condense::content_digest(&copy).await? != expected_digest {
            return Err("the copy doesn't hold the same rows as the books (if someone was entering something, try again)".into());
        }
        Ok(())
    }
    .await;
    crate::db::close_pool(&copy).await;
    checked
}

/// Turns whole-file encryption on for the open company file. Returns the Admin's new session (on
/// the encrypted file) and the new recovery key, which the caller shows once.
pub async fn encrypt(db: &Db, admin: &Session, input: EncryptInput) -> Result<(Session, crate::crypto::RecoveryKey), String> {
    Session::require_admin(Some(admin))?;
    if !input.understood {
        return Err("ENCRYPT_NOT_CONFIRMED: tick that you understand what losing every Admin password and the recovery key means".into());
    }
    if db.is_encrypted().await {
        return Err("ENCRYPT_ALREADY_ON: this company file is already encrypted".into());
    }
    if db.is_shared().await {
        // There LocalBooks holds the file exclusively, so it can't read it a second time to copy it.
        return Err("ENCRYPT_SHARED: move the company file to this computer first (Settings › File & Data Controls › Change Company File Location), encrypt it, then move it back".into());
    }
    let (pool, _) = db.pool_and_session().await?;
    // The Admin's password, checked like a sign-in: it is what will open the file.
    let fresh = users::sign_in(&pool, &admin.user.username, &input.password).await.map_err(|_| "ENCRYPT_WRONG_PASSWORD: that is not your password".to_string())?;
    let others: Vec<_> = users::list(&pool).await?.into_iter().filter(|u| u.is_active && u.id != admin.user.id).collect();
    let missing: Vec<_> = others.iter().filter(|u| !input.temporary_passwords.iter().any(|t| t.user_id == u.id)).map(|u| u.username.clone()).collect();
    if !missing.is_empty() {
        return Err(format!("ENCRYPT_NEEDS_PASSWORDS: give each user a temporary password first ({})", missing.join(", ")));
    }
    for t in &input.temporary_passwords {
        crate::crypto::check_password_rules(&t.password)?;
    }

    let live_path = db.path().await?;
    let staged = beside(&live_path, "encrypting");
    remove_with_sidecars(&staged);
    let key = Arc::new(keys::PageKey::generate()?);
    let file_id = keys::new_file_id()?;
    let digest;
    let recovery = {
        // A recovery key for the new file: replaced in the copy (below), shown once by the caller.
        let mut slots = vec![keys::password_slot(&key, &file_id, admin.user.id, &admin.user.username, &input.password).await?];
        for u in &others {
            let password = &input.temporary_passwords.iter().find(|t| t.user_id == u.id).expect("checked above").password;
            slots.push(keys::password_slot(&key, &file_id, u.id, &u.username, password).await?);
        }
        let header = Header { file_id: file_id.clone(), page_size: 4096, slots };
        let _ = sqlx::query("PRAGMA wal_checkpoint(PASSIVE)").execute(&pool).await;
        // What the books held when they were copied: the swap only happens if nothing changed since.
        digest = crate::condense::content_digest(&pool).await?;
        encrypt_into(&live_path, &staged, &key, &header).await?;

        let prepared = async {
            check_copy(&digest, open_options(&staged, Some(&key))?).await?;
            // The copy's own changes, through a pool so the user functions work as they always do.
            let copy_pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(open_options(&staged, Some(&key))?.foreign_keys(true)).await.map_err(|e| e.to_string())?;
            let changed = async {
                for u in &others {
                    let password = input.temporary_passwords.iter().find(|t| t.user_id == u.id).expect("checked above").password.clone();
                    let update = UserUpdate { display_name: u.display_name.clone(), role: u.role, can_see_sensitive: u.can_see_sensitive, is_active: true, new_password: Some(password) };
                    users::update(&copy_pool, &fresh, u.id, update).await?;
                }
                let recovery = users::replace_recovery_key(&copy_pool, &fresh).await?;
                let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&copy_pool).await.map_err(|e| e.to_string())?;
                if check != "ok" {
                    return Err(format!("the copy failed its integrity check ({check})"));
                }
                Ok(recovery)
            }
            .await;
            crate::db::close_pool(&copy_pool).await;
            let recovery = changed?;
            let slot = keys::recovery_slot(&key, &file_id, recovery.as_str()).await?;
            // The copy is closed now: its header can be written by path.
            let mut header = super::header::read(&staged)?;
            header.slots.push(slot);
            super::header::write(&staged, &header)?;
            Ok::<_, String>(recovery)
        }
        .await;
        match prepared {
            Ok(recovery) => recovery,
            Err(e) => {
                remove_with_sidecars(&staged);
                return Err(format!("ENCRYPT_FAILED: {e}; nothing was changed"));
            }
        }
    };

    // The swap needs a copy of the current books to fall back on; it is plain, so it goes afterwards.
    let fallback = db.backups_dir().join(format!("pre-encryption-{}.db", stamp()));
    if let Err(e) = crate::backup::snapshot(&pool, &live_path, &fallback).await {
        // The copy holds the new slots and recovery key: it doesn't outlive a failed attempt.
        remove_with_sidecars(&staged);
        return Err(format!("ENCRYPT_FAILED: {e}; nothing was changed"));
    }
    keyring::register(&file_id, key.clone());
    let swapped = db.swap_in_unchanged(&staged, &fallback, &digest).await.map_err(changed_while_copying);
    if let Err(e) = swapped {
        keyring::forget(&file_id);
        remove_with_sidecars(&staged);
        let _ = std::fs::remove_file(&fallback);
        return Err(format!("ENCRYPT_FAILED: {e}"));
    }
    if let Err(e) = std::fs::remove_file(&fallback) {
        // Still this company's, so Settings lists it with the other unencrypted backups to delete.
        eprintln!("LocalBooks: could not delete the unencrypted copy made before encryption ({}): {e}", fallback.display());
    }
    let session = super::user_slots::sign_in(db, &admin.user.username, &input.password).await?;
    Ok((session, recovery))
}

/// Turns whole-file encryption off: the books go back to a plain SQLite file. The Admin's password
/// is asked again. Returns the Admin's new session.
pub async fn decrypt(db: &Db, admin: &Session, password: &str) -> Result<Session, String> {
    Session::require_admin(Some(admin))?;
    if !db.is_encrypted().await {
        return Err("DECRYPT_NOT_ON: this company file isn't encrypted".into());
    }
    let (pool, _) = db.pool_and_session().await?;
    users::sign_in(&pool, &admin.user.username, password).await.map_err(|_| "DECRYPT_WRONG_PASSWORD: that is not your password".to_string())?;
    let live_path = db.path().await?;
    let staged = beside(&live_path, "decrypting");
    remove_with_sidecars(&staged);
    let digest = crate::condense::content_digest(&pool).await?;
    let exported = async {
        // On the live connection, which has the key: the attached file is plain (an empty key).
        let mut conn = pool.acquire().await.map_err(|e| e.to_string())?.detach();
        let result = async {
            sqlx::query("ATTACH DATABASE ?1 AS plain KEY ''").bind(staged.to_string_lossy().into_owned()).execute(&mut conn).await.map_err(|e| e.to_string())?;
            sqlx::query("SELECT sqlcipher_export('plain')").execute(&mut conn).await.map_err(|e| e.to_string())?;
            sqlx::query("DETACH DATABASE plain").execute(&mut conn).await.map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        }
        .await;
        let _ = conn.close().await;
        result?;
        check_copy(&digest, sqlx::sqlite::SqliteConnectOptions::new().filename(&staged)).await
    }
    .await;
    if let Err(e) = exported {
        remove_with_sidecars(&staged);
        return Err(format!("DECRYPT_FAILED: {e}; nothing was changed"));
    }
    // The fallback is a copy of the encrypted books, so it stays encrypted; kept like any backup.
    let fallback = db.backups_dir().join(format!("pre-decryption-{}.db", stamp()));
    if let Err(e) = crate::backup::snapshot(&pool, &live_path, &fallback).await {
        remove_with_sidecars(&staged);
        return Err(format!("DECRYPT_FAILED: {e}; nothing was changed"));
    }
    if let Err(e) = db.swap_in_unchanged(&staged, &fallback, &digest).await.map_err(changed_while_copying) {
        remove_with_sidecars(&staged);
        return Err(format!("DECRYPT_FAILED: {e}"));
    }
    let (pool, _) = db.pool_and_session().await?;
    users::sign_in(&pool, &admin.user.username, password).await
}

/// This company's backups in the backups folder that are not encrypted (made before encryption was
/// turned on). The folder is shared by every company file on this computer, so a plain backup counts
/// as this company's only if it has the same data key (the key users and tax IDs were set up with,
/// kept when the file is encrypted); other companies' backups, and backups made before users were
/// turned on, are left alone.
pub async fn unencrypted_backups(db: &Db) -> Result<Vec<crate::backup::BackupFile>, String> {
    let (pool, _) = db.pool_and_session().await?;
    let mine: Option<String> = sqlx::query_scalar("SELECT key_id FROM data_key WHERE id = 1").fetch_optional(&pool).await.map_err(|e| e.to_string())?;
    let Some(mine) = mine else { return Ok(Vec::new()) };
    let mut found = Vec::new();
    for backup in crate::backup::list_backups(db.backups_dir())? {
        let path = Path::new(&backup.path);
        if super::header::is_encrypted(path) {
            continue;
        }
        let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path).read_only(true);
        let Ok(mut conn) = options.connect().await else { continue };
        let theirs: Option<String> = sqlx::query_scalar("SELECT key_id FROM data_key WHERE id = 1").fetch_optional(&mut conn).await.ok().flatten();
        let _ = conn.close().await;
        if theirs.as_deref() == Some(mine.as_str()) {
            found.push(backup);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
