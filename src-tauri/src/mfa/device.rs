//! Two-step sign-in on this computer (gap R29; design in `docs/plans/passkeys.md`; migration 0175).
//!
//! Beside the authenticator app, a user may let **one computer** sign them in with Touch ID or
//! Windows Hello ([`crate::platform_auth`]) instead of typing a code:
//!
//! - **Adding the computer** (after the user's password and the system's check) makes a random
//!   32-byte secret and a random 16-byte id. The secret goes into this computer's credential store
//!   ([`crate::secrets`]: the macOS Keychain or Windows Credential Manager) under the id; the company
//!   file keeps the id and a salted SHA-256 hash of the secret, never the secret.
//! - **Signing in** with it, after the password: the system confirms the person, then LocalBooks
//!   reads the secret from the credential store and compares its hash with the file's. A copy of the
//!   file on another computer has no secret to read, so it asks for the app's code there.
//! - **The app stays.** A computer can only be added beside an authenticator app, and whatever
//!   removes the app removes the computers too (a trigger), so the backup codes, the Admin's reset
//!   and the recovery key work exactly as before.
//!
//! The secret is 256 random bits, so a plain hash is enough; the salt is the row's own id. Nothing
//! here writes the secret to the audit trail, a log or an error.

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use zeroize::Zeroizing;

use super::{db_err, has_factor, policy, Policy};
use crate::crypto;
use crate::platform_auth::Kind;
use crate::secrets::SecretStore;
use crate::users::Session;

const ID_BYTES: usize = 16;
const SECRET_BYTES: usize = 32;

pub const NOT_HERE: &str = "MFA_DEVICE_NOT_HERE: this computer isn't set up to sign you in; enter the code from your authenticator app";

/// The credential store this computer keeps its sign-in secrets in, one per computer added.
pub fn store(config_dir: &std::path::Path) -> SecretStore {
    SecretStore::for_this_computer(config_dir, "LocalBooks two-step sign-in", "two-step-computers.json", "this computer's two-step sign-in")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() / 2));
    for pair in text.as_bytes().chunks(2) {
        out.push(u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?);
    }
    Some(out)
}

/// The secret's name in the credential store.
fn store_key(credential_id: &[u8]) -> String {
    format!("two-step:{}", hex(credential_id))
}

/// What the file keeps: bound to the user and the id, so a row can't be moved to another user.
fn secret_hash(user_id: i64, credential_id: &[u8], secret: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"localbooks two-step computer v1\0");
    hasher.update(user_id.to_le_bytes());
    hasher.update(credential_id);
    hasher.update(secret);
    hasher.finalize().to_vec()
}

fn kind_text(kind: Kind) -> Result<&'static str, String> {
    match kind {
        Kind::TouchId => Ok("TOUCH_ID"),
        Kind::WindowsHello => Ok("WINDOWS_HELLO"),
        Kind::None => Err(crate::platform_auth::UNSUPPORTED.into()),
    }
}

/// One computer set up to sign a user in, as Settings › Users & Passwords lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Computer {
    pub id: i64,
    pub label: String,
    pub kind: String,
    pub enrolled_at: String,
    pub last_used_at: Option<String>,
    /// This is the computer LocalBooks is running on (its secret is in this credential store).
    pub here: bool,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    credential_id: Vec<u8>,
    label: String,
    kind: String,
    secret_hash: Vec<u8>,
    enrolled_at: String,
    last_used_at: Option<String>,
}

async fn rows(conn: &mut SqliteConnection, user_id: i64) -> Result<Vec<Row>, String> {
    sqlx::query_as("SELECT id, credential_id, label, kind, secret_hash, enrolled_at, last_used_at FROM user_device_factors WHERE user_id = ?1 ORDER BY id")
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await
        .map_err(db_err)
}

/// The row whose secret this computer holds and still matches, if any.
async fn row_here(conn: &mut SqliteConnection, store: &SecretStore, user_id: i64) -> Result<Option<Row>, String> {
    for row in rows(conn, user_id).await? {
        let Some(text) = store.get(&store_key(&row.credential_id)).await? else { continue };
        let Some(secret) = unhex(&text) else { continue };
        if super::same(&secret_hash(user_id, &row.credential_id, &secret), &row.secret_hash) {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

/// The computers set up to sign `user_id` in, marking this one.
pub async fn list(conn: &mut SqliteConnection, store: &SecretStore, user_id: i64) -> Result<Vec<Computer>, String> {
    let here = row_here(&mut *conn, store, user_id).await?.map(|r| r.id);
    Ok(rows(conn, user_id)
        .await?
        .into_iter()
        .map(|r| Computer { id: r.id, here: Some(r.id) == here, label: r.label, kind: r.kind, enrolled_at: r.enrolled_at, last_used_at: r.last_used_at })
        .collect())
}

/// Whether this computer can sign `user_id` in (its secret is here and matches the file).
pub async fn set_up_here(conn: &mut SqliteConnection, store: &SecretStore, user_id: i64) -> Result<bool, String> {
    Ok(row_here(conn, store, user_id).await?.is_some())
}

/// Adds this computer for the signed-in user. The caller has already checked their password and had
/// the system confirm them (`kind` says which check that was). Adding it again replaces the old one.
pub async fn add(pool: &SqlitePool, store: &SecretStore, session: &Session, kind: Kind, label: &str) -> Result<Computer, String> {
    let kind = kind_text(kind)?;
    let (user_id, username) = (session.user.id, session.user.username.as_str());
    let label: String = label.trim().chars().take(200).collect();
    let label = if label.is_empty() { "This computer".to_string() } else { label };
    {
        let mut conn = pool.acquire().await.map_err(db_err)?;
        if policy(&mut conn).await? == Policy::Off {
            return Err("MFA_OFF: an Admin has to turn on two-step sign-in for this company first".into());
        }
        if !has_factor(&mut conn, user_id).await? {
            return Err("MFA_DEVICE_NEEDS_APP: set up an authenticator app first; this computer is added beside it".into());
        }
    }
    let credential_id: [u8; ID_BYTES] = crypto::random()?;
    let secret = Zeroizing::new(crypto::random::<SECRET_BYTES>()?);
    // The secret is saved on the computer first: if the file can't be written, it is removed again.
    store.put(&store_key(&credential_id), &hex(&secret[..])).await?;
    let written = async {
        let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
        // Adding this computer again replaces its old row.
        let replaced = row_here(&mut tx, store, user_id).await?;
        if let Some(old) = &replaced {
            sqlx::query("DELETE FROM user_device_factors WHERE id = ?1").bind(old.id).execute(&mut *tx).await.map_err(db_err)?;
        }
        let id: i64 =
            sqlx::query_scalar("INSERT INTO user_device_factors (user_id, credential_id, label, kind, secret_hash) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id")
                .bind(user_id)
                .bind(&credential_id[..])
                .bind(&label)
                .bind(kind)
                .bind(secret_hash(user_id, &credential_id, &secret[..]))
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?;
        crate::users::record(
            &mut tx,
            user_id,
            "MFA_DEVICE_ADDED",
            serde_json::json!({ "username": username, "computer": label, "kind": kind, "replaced": replaced.is_some() }),
        )
        .await?;
        crate::ledger::commit_write(tx).await.map_err(db_err)?;
        Ok::<_, String>((id, replaced))
    }
    .await;
    match written {
        Ok((id, replaced)) => {
            if let Some(old) = replaced {
                let _ = store.delete(&store_key(&old.credential_id)).await;
            }
            let mut conn = pool.acquire().await.map_err(db_err)?;
            let computers = list(&mut conn, store, user_id).await?;
            computers.into_iter().find(|c| c.id == id).ok_or_else(|| "MFA_DEVICE: the computer was not saved".to_string())
        }
        Err(e) => {
            let _ = store.delete(&store_key(&credential_id)).await;
            Err(e)
        }
    }
}

/// At sign-in, after the password and the system's check: this computer's secret matches the
/// file's, so the code step is passed. [`NOT_HERE`] when this computer isn't set up for the user.
pub async fn check(pool: &SqlitePool, store: &SecretStore, user_id: i64) -> Result<(), String> {
    let row = {
        let mut conn = pool.acquire().await.map_err(db_err)?;
        row_here(&mut conn, store, user_id).await?.ok_or_else(|| NOT_HERE.to_string())?
    };
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    sqlx::query("UPDATE user_device_factors SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// The signed-in user removes one of their computers (this one or another). Their authenticator app
/// stays. This computer's secret is removed from its credential store when it is this one.
pub async fn remove(pool: &SqlitePool, store: &SecretStore, session: &Session, id: i64) -> Result<(), String> {
    let user_id = session.user.id;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let row: Option<(Vec<u8>, String)> = sqlx::query_as("SELECT credential_id, label FROM user_device_factors WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let (credential_id, label) = row.ok_or_else(|| "MFA_DEVICE_NONE: that computer isn't set up to sign you in".to_string())?;
    sqlx::query("DELETE FROM user_device_factors WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    crate::users::record(&mut tx, user_id, "MFA_DEVICE_REMOVED", serde_json::json!({ "username": session.user.username, "computer": label })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    store.delete(&store_key(&credential_id)).await
}

/// The credential-store names of `user_id`'s computers in the open file, read before something
/// removes the rows (turning the app off here), so [`forget_keys`] can clear them afterwards.
/// Secrets left behind by a removal made on another computer, or in a copy of the file, match
/// nothing and are harmless.
pub async fn here_keys(conn: &mut SqliteConnection, store: &SecretStore, user_id: i64) -> Result<Vec<String>, String> {
    let mut keys = Vec::new();
    for row in rows(conn, user_id).await? {
        let key = store_key(&row.credential_id);
        if store.get(&key).await?.is_some() {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// Removes those secrets from this computer's credential store (best effort: they no longer match
/// any row).
pub async fn forget_keys(store: &SecretStore, keys: &[String]) {
    for key in keys {
        if let Err(e) = store.delete(key).await {
            eprintln!("LocalBooks: could not remove a two-step sign-in secret from this computer: {e}");
        }
    }
}

#[cfg(test)]
mod tests;
