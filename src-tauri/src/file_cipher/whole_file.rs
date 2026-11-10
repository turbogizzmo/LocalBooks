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

use sqlx::{ConnectOptions, Connection, SqliteConnection};

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

/// Row counts of every table, to check an exported copy against the live books.
async fn row_counts(conn: &mut SqliteConnection) -> Result<Vec<(String, i64)>, String> {
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let mut counts = Vec::with_capacity(tables.len());
    for table in tables {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\""))).fetch_one(&mut *conn).await.map_err(|e| e.to_string())?;
        counts.push((table, n));
    }
    Ok(counts)
}

async fn check_copy(live: &sqlx::SqlitePool, copy: &mut SqliteConnection) -> Result<(), String> {
    let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&mut *copy).await.map_err(|e| e.to_string())?;
    if check != "ok" {
        return Err(format!("the copy failed its integrity check ({check})"));
    }
    let mut conn = live.acquire().await.map_err(|e| e.to_string())?;
    if row_counts(&mut conn).await? != row_counts(copy).await? {
        return Err("the copy doesn't have the same rows as the books".into());
    }
    Ok(())
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
    let recovery = {
        // A recovery key for the new file: replaced in the copy (below), shown once by the caller.
        let mut slots = vec![keys::password_slot(&key, &file_id, admin.user.id, &admin.user.username, &input.password).await?];
        for u in &others {
            let password = &input.temporary_passwords.iter().find(|t| t.user_id == u.id).expect("checked above").password;
            slots.push(keys::password_slot(&key, &file_id, u.id, &u.username, password).await?);
        }
        let header = Header { file_id: file_id.clone(), page_size: 4096, slots };
        let _ = sqlx::query("PRAGMA wal_checkpoint(PASSIVE)").execute(&pool).await;
        encrypt_into(&live_path, &staged, &key, &header).await?;

        let prepared = async {
            let mut copy = open_options(&staged, Some(&key))?.connect().await.map_err(|e| e.to_string())?;
            let checked = check_copy(&pool, &mut copy).await;
            let _ = copy.close().await;
            checked?;
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
    crate::backup::snapshot(&pool, &live_path, &fallback).await?;
    keyring::register(&file_id, key.clone());
    let swapped = db.swap_in(&staged, &fallback).await;
    if let Err(e) = swapped {
        keyring::forget(&file_id);
        remove_with_sidecars(&staged);
        let _ = std::fs::remove_file(&fallback);
        return Err(format!("ENCRYPT_FAILED: {e}"));
    }
    let _ = std::fs::remove_file(&fallback);
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
        let mut copy = sqlx::sqlite::SqliteConnectOptions::new().filename(&staged).connect().await.map_err(|e| e.to_string())?;
        let checked = check_copy(&pool, &mut copy).await;
        let _ = copy.close().await;
        checked
    }
    .await;
    if let Err(e) = exported {
        remove_with_sidecars(&staged);
        return Err(format!("DECRYPT_FAILED: {e}; nothing was changed"));
    }
    // The fallback is a copy of the encrypted books, so it stays encrypted; kept like any backup.
    let fallback = db.backups_dir().join(format!("pre-decryption-{}.db", stamp()));
    crate::backup::snapshot(&pool, &live_path, &fallback).await?;
    db.swap_in(&staged, &fallback).await.map_err(|e| format!("DECRYPT_FAILED: {e}"))?;
    let (pool, _) = db.pool_and_session().await?;
    users::sign_in(&pool, &admin.user.username, password).await
}

/// Backups in the backups folder that are not encrypted (made before encryption was turned on).
pub fn unencrypted_backups(db_backups: &Path) -> Result<Vec<crate::backup::BackupFile>, String> {
    Ok(crate::backup::list_backups(db_backups)?.into_iter().filter(|b| !super::header::is_encrypted(Path::new(&b.path))).collect())
}

#[cfg(test)]
mod tests;
