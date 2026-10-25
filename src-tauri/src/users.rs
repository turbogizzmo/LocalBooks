//! Company-file users and signing in (see migration 0043).
//!
//! A company file starts without users and opens for whoever starts LocalBooks. Turning users on
//! creates the first Admin together with the file's data key ([`crate::crypto`]) and returns the
//! recovery key, once. From then on the file opens on a sign-in screen.
//!
//! Signing in checks the user's password verifier; for a user who may see sensitive data it also
//! unlocks the data key with the same password. The signed-in [`Session`] lives with the open company
//! file in [`crate::db::Db`], so closing, switching or restoring the file signs the user out.
//!
//! An Admin manages users. Granting someone sensitive data needs a password for them (the Admin sets
//! a temporary one), because the data key can only be stored for a password. A forgotten Admin
//! password is replaced with the recovery key; when neither is left, [`remove_all_users`] removes
//! every user and the sensitive values they protected, and keeps the books.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::crypto::{self, DataKey, RecoveryKey};
use crate::ledger::db_err;

/// Minutes without activity before a signed-in user is signed out, unless an Admin changed it.
pub const DEFAULT_AUTO_LOCK_MINUTES: u32 = 15;

/// The company's idle limit in minutes (`auto_lock_minutes`; 0 = never).
pub async fn auto_lock_minutes(pool: &SqlitePool) -> u32 {
    crate::preferences::get(pool, "auto_lock_minutes").await.ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_AUTO_LOCK_MINUTES)
}

/// What the owner types to confirm [`remove_all_users`].
pub const REMOVE_ALL_CONFIRMATION: &str = "REMOVE ALL USERS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    /// Everything, including managing users.
    Admin,
    /// Everything except managing users and company-wide settings.
    Standard,
    /// Reads the books; changes nothing.
    ViewOnly,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "ADMIN",
            Role::Standard => "STANDARD",
            Role::ViewOnly => "VIEW_ONLY",
        }
    }

    fn parse(s: &str) -> Result<Role, String> {
        match s {
            "ADMIN" => Ok(Role::Admin),
            "STANDARD" => Ok(Role::Standard),
            "VIEW_ONLY" => Ok(Role::ViewOnly),
            other => Err(format!("USER_BAD_ROLE: unknown role '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub can_see_sensitive: bool,
    pub is_active: bool,
    pub must_change_password: bool,
    pub last_signed_in_at: Option<String>,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    username: String,
    display_name: String,
    role: String,
    can_see_sensitive: i64,
    is_active: i64,
    must_change_password: i64,
    last_signed_in_at: Option<String>,
    created_at: String,
    password_verifier: String,
}

impl UserRow {
    fn view(&self) -> Result<UserView, String> {
        Ok(UserView {
            id: self.id,
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            role: Role::parse(&self.role)?,
            can_see_sensitive: self.can_see_sensitive == 1,
            is_active: self.is_active == 1,
            must_change_password: self.must_change_password == 1,
            last_signed_in_at: self.last_signed_in_at.clone(),
            created_at: self.created_at.clone(),
        })
    }
}

const USER_COLUMNS: &str =
    "id, username, display_name, role, can_see_sensitive, is_active, must_change_password, last_signed_in_at, created_at, password_verifier";

/// Who is signed in to the open company file, and the data key when they may see sensitive data.
#[derive(Debug)]
pub struct Session {
    pub user: UserView,
    pub key: Option<DataKey>,
}

impl Session {
    /// This session, if its user is an Admin.
    pub fn require_admin(session: Option<&Session>) -> Result<&Session, String> {
        let session = session.ok_or_else(|| SIGN_IN_REQUIRED.to_string())?;
        if session.user.role != Role::Admin {
            return Err("USER_ADMIN_ONLY: only an Admin can manage users".into());
        }
        Ok(session)
    }

    fn admin_key(&self) -> Result<&DataKey, String> {
        self.key.as_ref().ok_or_else(|| "KEY_LOCKED: sign in again to unlock sensitive data".to_string())
    }
}

pub const SIGN_IN_REQUIRED: &str = "SIGN_IN_REQUIRED: sign in to this company file first";
const SIGN_IN_FAILED: &str = "SIGN_IN_FAILED: the user name or password is not correct";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUserInput {
    pub username: String,
    pub display_name: Option<String>,
    pub role: Role,
    pub can_see_sensitive: bool,
    /// A temporary password: the user chooses their own at first sign-in.
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdate {
    pub display_name: String,
    pub role: Role,
    pub can_see_sensitive: bool,
    pub is_active: bool,
    /// A new temporary password (a reset); required to grant sensitive data to someone without it.
    pub new_password: Option<String>,
}

fn clean_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() || username.chars().count() > 50 {
        return Err("USER_BAD_NAME: a user name has 1 to 50 characters".into());
    }
    Ok(username.to_string())
}

fn display_or(display_name: Option<&str>, username: &str) -> Result<String, String> {
    let name = display_name.map(str::trim).filter(|d| !d.is_empty()).unwrap_or(username);
    if name.chars().count() > 100 {
        return Err("USER_BAD_NAME: a name has at most 100 characters".into());
    }
    Ok(name.to_string())
}

/// Records a change to users in the ledger audit trail, sealed into its hash chain when the write
/// commits (see `audit_chain.rs`): who did it is the actor, `user_id` the user it concerns (0 for
/// the file as a whole). Never carries a password or key.
async fn record(conn: &mut sqlx::SqliteConnection, user_id: i64, event: &str, mut details: serde_json::Value) -> Result<(), String> {
    details["event"] = event.into();
    sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('USER_CHANGE', ?1, ?2)")
        .bind(user_id)
        .bind(details.to_string())
        .execute(&mut *conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Whether this company file has users (and so asks who is signing in).
pub async fn users_enabled(pool: &SqlitePool) -> Result<bool, String> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await.map_err(db_err)?;
    Ok(n > 0)
}

async fn row_by_id(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<UserRow, String> {
    sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("USER_NOT_FOUND: no user {id}"))
}

async fn row_by_name(conn: &mut sqlx::SqliteConnection, username: &str) -> Result<Option<UserRow>, String> {
    sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1"))
        .bind(username.trim())
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)
}

async fn mark_signed_in(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<(), String> {
    sqlx::query("UPDATE users SET last_signed_in_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Turns users on: `username` becomes the first Admin, signed in. The recovery key is returned once.
pub async fn turn_on(pool: &SqlitePool, username: &str, display_name: Option<&str>, password: &str) -> Result<(Session, RecoveryKey), String> {
    let username = clean_username(username)?;
    crypto::check_password_rules(password)?;
    let verifier = crypto::password_verifier(password).await?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await.map_err(db_err)?;
    if existing > 0 {
        return Err("USERS_ALREADY_ON: this company file already has users".into());
    }
    let (key, recovery) = crypto::create(&mut tx).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, role, can_see_sensitive, password_verifier, last_signed_in_at)
         VALUES (?1, ?2, 'ADMIN', 1, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) RETURNING id",
    )
    .bind(&username)
    .bind(display_or(display_name, &username)?)
    .bind(verifier)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    crypto::set_password(&mut tx, &key, id, password).await?;
    // Tax IDs already in the file are encrypted now, in the same transaction.
    let protected = crate::tax_ids::protect_all(&mut tx, &key).await?;
    record(&mut tx, id, "USERS_TURNED_ON", serde_json::json!({ "admin": username, "taxIdsEncrypted": protected })).await?;
    let user = row_by_id(&mut tx, id).await?.view()?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok((Session { user, key: Some(key) }, recovery))
}

/// Signs `username` in with `password`.
pub async fn sign_in(pool: &SqlitePool, username: &str, password: &str) -> Result<Session, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let Some(row) = row_by_name(&mut conn, username).await? else {
        // Take as long as a real check, so the answer does not reveal which user names exist.
        let _ = crypto::password_verifier(password).await;
        return Err(SIGN_IN_FAILED.into());
    };
    if !crypto::verify_password(password, &row.password_verifier).await? {
        return Err(SIGN_IN_FAILED.into());
    }
    if row.is_active == 0 {
        return Err("SIGN_IN_TURNED_OFF: this user is turned off; ask an Admin to turn it back on".into());
    }
    let key = if row.can_see_sensitive == 1 { Some(crypto::unlock_with_password(&mut conn, row.id, password).await?) } else { None };
    if let Some(key) = &key {
        // Protects anything written in plain since (an import by someone without the key).
        crate::tax_ids::protect_all(&mut conn, key).await?;
    }
    mark_signed_in(&mut conn, row.id).await?;
    let user = row_by_id(&mut conn, row.id).await?.view()?;
    Ok(Session { user, key })
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<UserView>, String> {
    let rows: Vec<UserRow> = sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users ORDER BY is_active DESC, username"))
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    rows.iter().map(UserRow::view).collect()
}

/// An Admin adds a user with a temporary password.
pub async fn add(pool: &SqlitePool, admin: &Session, input: NewUserInput) -> Result<UserView, String> {
    let username = clean_username(&input.username)?;
    crypto::check_password_rules(&input.password)?;
    let sensitive = input.can_see_sensitive || input.role == Role::Admin;
    let key = if sensitive { Some(admin.admin_key()?) } else { None };
    let verifier = crypto::password_verifier(&input.password).await?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if row_by_name(&mut tx, &username).await?.is_some() {
        return Err(format!("USER_EXISTS: there is already a user named {username}"));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, role, can_see_sensitive, password_verifier, must_change_password)
         VALUES (?1, ?2, ?3, ?4, ?5, 1) RETURNING id",
    )
    .bind(&username)
    .bind(display_or(input.display_name.as_deref(), &username)?)
    .bind(input.role.as_str())
    .bind(sensitive as i64)
    .bind(verifier)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if let Some(key) = key {
        crypto::set_password(&mut tx, key, id, &input.password).await?;
    }
    let user = row_by_id(&mut tx, id).await?.view()?;
    record(&mut tx, id, "USER_ADDED", serde_json::json!({ "username": user.username, "role": user.role, "canSeeSensitive": user.can_see_sensitive })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(user)
}

/// An Admin changes another user: name, role, sensitive data, on or off, and optionally a password reset.
pub async fn update(pool: &SqlitePool, admin: &Session, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let sensitive = input.can_see_sensitive || input.role == Role::Admin;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let current = row_by_id(&mut tx, id).await?;
    if id == admin.user.id && (input.role != Role::Admin || !input.is_active) {
        return Err("USER_EDIT_SELF: another Admin has to change your role or turn you off".into());
    }
    let verifier = match &input.new_password {
        Some(password) => {
            crypto::check_password_rules(password)?;
            Some(crypto::password_verifier(password).await?)
        }
        None => None,
    };
    let display_name = display_or(Some(&input.display_name), &current.username)?;
    sqlx::query(
        "UPDATE users SET display_name = ?1, role = ?2, can_see_sensitive = ?3, is_active = ?4,
                password_verifier = coalesce(?5, password_verifier),
                must_change_password = CASE WHEN ?5 IS NULL THEN must_change_password ELSE 1 END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?6",
    )
    .bind(display_name)
    .bind(input.role.as_str())
    .bind(sensitive as i64)
    .bind(input.is_active as i64)
    .bind(verifier)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    // Losing sensitive data or being turned off removes the user's slot (a trigger). Keeping or
    // gaining it with a new password writes a slot for that password.
    if sensitive && input.is_active {
        match &input.new_password {
            Some(password) => crypto::set_password(&mut tx, admin.admin_key()?, id, password).await?,
            None => {
                let has_slot: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_slots WHERE user_id = ?1").bind(id).fetch_one(&mut *tx).await.map_err(db_err)?;
                if has_slot == 0 {
                    return Err("SENSITIVE_NEEDS_PASSWORD: set a temporary password to give this user access to sensitive data".into());
                }
            }
        }
    }
    let user = row_by_id(&mut tx, id).await?.view()?;
    record(
        &mut tx,
        id,
        "USER_CHANGED",
        serde_json::json!({
            "username": user.username,
            "role": user.role,
            "canSeeSensitive": user.can_see_sensitive,
            "isActive": user.is_active,
            "passwordReset": input.new_password.is_some(),
        }),
    )
    .await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(user)
}

/// The signed-in user changes their own password.
pub async fn change_own_password(pool: &SqlitePool, session: &mut Session, current: &str, next: &str) -> Result<UserView, String> {
    crypto::check_password_rules(next)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let row = row_by_id(&mut tx, session.user.id).await?;
    if !crypto::verify_password(current, &row.password_verifier).await? {
        return Err("PASSWORD_WRONG: your current password is not correct".into());
    }
    let verifier = crypto::password_verifier(next).await?;
    sqlx::query("UPDATE users SET password_verifier = ?1, must_change_password = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2")
        .bind(verifier)
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    if let Some(key) = &session.key {
        crypto::set_password(&mut tx, key, row.id, next).await?;
    }
    let user = row_by_id(&mut tx, row.id).await?.view()?;
    record(&mut tx, row.id, "PASSWORD_CHANGED", serde_json::json!({ "username": user.username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    session.user = user.clone();
    Ok(user)
}

/// Replaces a forgotten Admin password with the recovery key and signs that Admin in.
pub async fn recover(pool: &SqlitePool, recovery_key: &str, username: &str, new_password: &str) -> Result<Session, String> {
    crypto::check_password_rules(new_password)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let key = crypto::unlock_with_recovery_key(&mut tx, recovery_key).await?;
    let row = row_by_name(&mut tx, username)
        .await?
        .filter(|r| r.role == "ADMIN" && r.is_active == 1)
        .ok_or_else(|| "RECOVER_NOT_ADMIN: the recovery key resets the password of an active Admin; enter an Admin's user name".to_string())?;
    let verifier = crypto::password_verifier(new_password).await?;
    sqlx::query("UPDATE users SET password_verifier = ?1, must_change_password = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2")
        .bind(verifier)
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    crypto::set_password(&mut tx, &key, row.id, new_password).await?;
    mark_signed_in(&mut tx, row.id).await?;
    let user = row_by_id(&mut tx, row.id).await?.view()?;
    record(&mut tx, row.id, "RECOVERY_KEY_USED", serde_json::json!({ "username": user.username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(Session { user, key: Some(key) })
}

/// An Admin issues a new recovery key; the old one stops working.
pub async fn replace_recovery_key(pool: &SqlitePool, admin: &Session) -> Result<RecoveryKey, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let recovery = crypto::replace_recovery_key(&mut tx, admin.admin_key()?).await?;
    record(&mut tx, 0, "RECOVERY_KEY_REPLACED", serde_json::json!({ "by": admin.user.username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(recovery)
}

/// The last resort when no Admin password or recovery key is left: removes every user, the data key
/// and the sensitive values it protected. The books themselves are kept, and the file opens without
/// signing in again.
pub async fn remove_all_users(pool: &SqlitePool, confirmation: &str) -> Result<(), String> {
    if confirmation.trim() != REMOVE_ALL_CONFIRMATION {
        return Err(format!("USERS_REMOVE_CONFIRM: type {REMOVE_ALL_CONFIRMATION} to confirm"));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let removed: Vec<String> = sqlx::query_scalar("SELECT username FROM users ORDER BY id").fetch_all(&mut *tx).await.map_err(db_err)?;
    if removed.is_empty() {
        return Err("USERS_NONE: this company file has no users to remove".into());
    }
    let erased: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vendors WHERE tax_id LIKE 'lb1:%'").fetch_one(&mut *tx).await.map_err(db_err)?;
    // Anyone at the sign-in screen can do this, so it is recorded where it can't be undone quietly:
    // the hash-chained audit trail, anchored outside the file.
    record(&mut tx, 0, "ALL_USERS_REMOVED", serde_json::json!({ "users": removed, "taxIdsErased": erased })).await?;
    for statement in [
        "UPDATE user_reset_in_progress SET value = 1",
        "DELETE FROM key_slots",
        "DELETE FROM users",
        "DELETE FROM data_key",
        // Encrypted values cannot be read without the data key.
        "UPDATE vendors SET tax_id = NULL, tax_id_masked = NULL WHERE tax_id LIKE 'lb1:%'",
        "UPDATE user_reset_in_progress SET value = NULL",
    ] {
        sqlx::query(statement).execute(&mut *tx).await.map_err(db_err)?;
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

#[cfg(test)]
mod tests;
