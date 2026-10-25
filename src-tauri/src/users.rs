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

fn display_or(display_name: Option<&str>, username: &str) -> String {
    display_name.map(str::trim).filter(|d| !d.is_empty()).unwrap_or(username).to_string()
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
    let mut tx = pool.begin().await.map_err(db_err)?;
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
    .bind(display_or(display_name, &username))
    .bind(verifier)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    crypto::set_password(&mut tx, &key, id, password).await?;
    // Tax IDs already in the file are encrypted now, in the same transaction.
    crate::tax_ids::protect_all(&mut tx, &key).await?;
    let user = row_by_id(&mut tx, id).await?.view()?;
    tx.commit().await.map_err(db_err)?;
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
    let mut tx = pool.begin().await.map_err(db_err)?;
    if row_by_name(&mut tx, &username).await?.is_some() {
        return Err(format!("USER_EXISTS: there is already a user named {username}"));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, role, can_see_sensitive, password_verifier, must_change_password)
         VALUES (?1, ?2, ?3, ?4, ?5, 1) RETURNING id",
    )
    .bind(&username)
    .bind(display_or(input.display_name.as_deref(), &username))
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
    tx.commit().await.map_err(db_err)?;
    Ok(user)
}

/// An Admin changes another user: name, role, sensitive data, on or off, and optionally a password reset.
pub async fn update(pool: &SqlitePool, admin: &Session, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let sensitive = input.can_see_sensitive || input.role == Role::Admin;
    let mut tx = pool.begin().await.map_err(db_err)?;
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
    let display_name = display_or(Some(&input.display_name), &current.username);
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
    tx.commit().await.map_err(db_err)?;
    Ok(user)
}

/// The signed-in user changes their own password.
pub async fn change_own_password(pool: &SqlitePool, session: &mut Session, current: &str, next: &str) -> Result<UserView, String> {
    crypto::check_password_rules(next)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
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
    tx.commit().await.map_err(db_err)?;
    session.user = user.clone();
    Ok(user)
}

/// Replaces a forgotten Admin password with the recovery key and signs that Admin in.
pub async fn recover(pool: &SqlitePool, recovery_key: &str, username: &str, new_password: &str) -> Result<Session, String> {
    crypto::check_password_rules(new_password)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
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
    tx.commit().await.map_err(db_err)?;
    Ok(Session { user, key: Some(key) })
}

/// An Admin issues a new recovery key; the old one stops working.
pub async fn replace_recovery_key(pool: &SqlitePool, admin: &Session) -> Result<RecoveryKey, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    crypto::replace_recovery_key(&mut conn, admin.admin_key()?).await
}

/// The last resort when no Admin password or recovery key is left: removes every user, the data key
/// and the sensitive values it protected. The books themselves are kept, and the file opens without
/// signing in again.
pub async fn remove_all_users(pool: &SqlitePool, confirmation: &str) -> Result<(), String> {
    if confirmation.trim() != REMOVE_ALL_CONFIRMATION {
        return Err(format!("USERS_REMOVE_CONFIRM: type {REMOVE_ALL_CONFIRMATION} to confirm"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
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
    tx.commit().await.map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn with_admin() -> (SqlitePool, Session, RecoveryKey) {
        let pool = test_pool().await;
        let (session, recovery) = turn_on(&pool, " Owner ", Some("Pat Owner"), "owner password").await.unwrap();
        (pool, session, recovery)
    }

    fn new_user(username: &str, role: Role, sensitive: bool) -> NewUserInput {
        NewUserInput { username: username.into(), display_name: None, role, can_see_sensitive: sensitive, password: "temporary pw".into() }
    }

    fn unchanged(user: &UserView) -> UserUpdate {
        UserUpdate { display_name: user.display_name.clone(), role: user.role, can_see_sensitive: user.can_see_sensitive, is_active: user.is_active, new_password: None }
    }

    #[tokio::test]
    async fn turning_users_on_makes_a_signed_in_admin() {
        let pool = test_pool().await;
        assert!(!users_enabled(&pool).await.unwrap());
        let (session, recovery) = turn_on(&pool, " Owner ", Some("Pat Owner"), "owner password").await.unwrap();
        assert!(users_enabled(&pool).await.unwrap());
        assert_eq!((session.user.username.as_str(), session.user.display_name.as_str(), session.user.role), ("Owner", "Pat Owner", Role::Admin));
        assert!(session.user.can_see_sensitive && session.key.is_some() && !session.user.must_change_password);
        assert_eq!(recovery.as_str().len(), 39);
        assert!(turn_on(&pool, "Other", None, "owner password").await.unwrap_err().starts_with("USERS_ALREADY_ON"));
    }

    #[tokio::test]
    async fn signing_in_checks_the_password_and_unlocks_the_key() {
        let (pool, admin, _) = with_admin().await;
        let session = sign_in(&pool, "owner", "owner password").await.unwrap();
        assert_eq!(session.user.id, admin.user.id, "user names are not case-sensitive");
        assert_eq!(session.key.unwrap().key_id(), admin.key.as_ref().unwrap().key_id());
        assert!(session.user.last_signed_in_at.is_some());

        assert_eq!(sign_in(&pool, "Owner", "wrong password").await.unwrap_err(), SIGN_IN_FAILED);
        assert_eq!(sign_in(&pool, "Nobody", "owner password").await.unwrap_err(), SIGN_IN_FAILED, "the same answer for unknown names");
    }

    #[tokio::test]
    async fn users_without_sensitive_access_get_no_key() {
        let (pool, admin, _) = with_admin().await;
        let clerk = add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
        assert!(clerk.must_change_password && !clerk.can_see_sensitive);
        let session = sign_in(&pool, "clerk", "temporary pw").await.unwrap();
        assert!(session.key.is_none());
        let slots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_slots WHERE user_id = ?1").bind(clerk.id).fetch_one(&pool).await.unwrap();
        assert_eq!(slots, 0);

        assert!(add(&pool, &admin, new_user("CLERK", Role::ViewOnly, false)).await.unwrap_err().starts_with("USER_EXISTS"));
        assert!(add(&pool, &session, new_user("x", Role::Standard, false)).await.is_ok(), "the function trusts its caller; commands check the role");
        assert!(Session::require_admin(Some(&session)).unwrap_err().starts_with("USER_ADMIN_ONLY"));
        assert!(Session::require_admin(None).unwrap_err().starts_with("SIGN_IN_REQUIRED"));
    }

    #[tokio::test]
    async fn granting_sensitive_access_needs_a_password() {
        let (pool, admin, _) = with_admin().await;
        let clerk = add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
        let grant = UserUpdate { can_see_sensitive: true, ..unchanged(&clerk) };
        assert!(update(&pool, &admin, clerk.id, grant.clone()).await.unwrap_err().starts_with("SENSITIVE_NEEDS_PASSWORD"));
        let still: i64 = sqlx::query_scalar("SELECT can_see_sensitive FROM users WHERE id = ?1").bind(clerk.id).fetch_one(&pool).await.unwrap();
        assert_eq!(still, 0, "the refused update was rolled back");

        update(&pool, &admin, clerk.id, UserUpdate { new_password: Some("new temp pw".into()), ..grant }).await.unwrap();
        let session = sign_in(&pool, "clerk", "new temp pw").await.unwrap();
        assert!(session.key.is_some() && session.user.must_change_password);

        let revoked = update(&pool, &admin, clerk.id, UserUpdate { can_see_sensitive: false, ..unchanged(&session.user) }).await.unwrap();
        assert!(!revoked.can_see_sensitive);
        assert!(sign_in(&pool, "clerk", "new temp pw").await.unwrap().key.is_none(), "the slot went with the access");
    }

    #[tokio::test]
    async fn turned_off_users_cannot_sign_in() {
        let (pool, admin, _) = with_admin().await;
        let bookkeeper = add(&pool, &admin, new_user("books", Role::Standard, true)).await.unwrap();
        update(&pool, &admin, bookkeeper.id, UserUpdate { is_active: false, ..unchanged(&bookkeeper) }).await.unwrap();
        assert!(sign_in(&pool, "books", "temporary pw").await.unwrap_err().starts_with("SIGN_IN_TURNED_OFF"));
        assert!(sign_in(&pool, "books", "wrong").await.unwrap_err().starts_with("SIGN_IN_FAILED"), "no hint without the password");
        let back = UserUpdate { is_active: true, ..unchanged(&bookkeeper) };
        assert!(update(&pool, &admin, bookkeeper.id, back.clone()).await.unwrap_err().starts_with("SENSITIVE_NEEDS_PASSWORD"));
        update(&pool, &admin, bookkeeper.id, UserUpdate { new_password: Some("back again pw".into()), ..back }).await.unwrap();
        assert!(sign_in(&pool, "books", "back again pw").await.unwrap().key.is_some());
    }

    #[tokio::test]
    async fn an_active_admin_always_remains() {
        let (pool, admin, _) = with_admin().await;
        let mut me = unchanged(&admin.user);
        me.role = Role::Standard;
        assert!(update(&pool, &admin, admin.user.id, me).await.unwrap_err().starts_with("USER_EDIT_SELF"));

        let second = add(&pool, &admin, new_user("second", Role::Admin, false)).await.unwrap();
        assert!(second.can_see_sensitive, "Admins always may see sensitive data");
        let second_session = sign_in(&pool, "second", "temporary pw").await.unwrap();
        update(&pool, &second_session, admin.user.id, UserUpdate { is_active: false, ..unchanged(&admin.user) }).await.unwrap();
        let err = sqlx::query("UPDATE users SET is_active = 0 WHERE id = ?1").bind(second.id).execute(&pool).await.unwrap_err();
        assert!(db_err(err).starts_with("USER_LAST_ADMIN"));
        let err = sqlx::query("DELETE FROM users WHERE id = ?1").bind(second.id).execute(&pool).await.unwrap_err();
        assert!(db_err(err).starts_with("USER_NO_DELETE"));
    }

    #[tokio::test]
    async fn changing_your_own_password_keeps_your_key() {
        let (pool, admin, _) = with_admin().await;
        add(&pool, &admin, new_user("books", Role::Standard, true)).await.unwrap();
        let mut session = sign_in(&pool, "books", "temporary pw").await.unwrap();
        assert!(change_own_password(&pool, &mut session, "wrong one", "my own password").await.unwrap_err().starts_with("PASSWORD_WRONG"));
        assert!(change_own_password(&pool, &mut session, "temporary pw", "short").await.unwrap_err().starts_with("PASSWORD_TOO_SHORT"));
        let user = change_own_password(&pool, &mut session, "temporary pw", "my own password").await.unwrap();
        assert!(!user.must_change_password && !session.user.must_change_password);
        assert!(sign_in(&pool, "books", "temporary pw").await.is_err());
        assert!(sign_in(&pool, "books", "my own password").await.unwrap().key.is_some());
    }

    #[tokio::test]
    async fn the_recovery_key_resets_an_admin_password() {
        let (pool, admin, recovery) = with_admin().await;
        add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
        assert!(recover(&pool, recovery.as_str(), "clerk", "fresh password").await.unwrap_err().starts_with("RECOVER_NOT_ADMIN"));
        assert!(recover(&pool, &"A".repeat(32), "Owner", "fresh password").await.unwrap_err().starts_with("KEY_WRONG_RECOVERY"));

        let session = recover(&pool, recovery.as_str(), "owner", "fresh password").await.unwrap();
        assert_eq!(session.user.id, admin.user.id);
        assert!(sign_in(&pool, "Owner", "owner password").await.is_err());
        assert!(sign_in(&pool, "Owner", "fresh password").await.unwrap().key.is_some());

        let fresh = replace_recovery_key(&pool, &session).await.unwrap();
        assert!(recover(&pool, recovery.as_str(), "Owner", "another password").await.is_err());
        assert!(recover(&pool, fresh.as_str(), "Owner", "another password").await.is_ok());
    }

    #[tokio::test]
    async fn removing_all_users_keeps_the_books_and_drops_protected_values() {
        let (pool, admin, _) = with_admin().await;
        add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
        let vendor: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Acme', 'plain-123') RETURNING id").fetch_one(&pool).await.unwrap();
        let other: i64 = sqlx::query_scalar("INSERT INTO vendors (name) VALUES ('Beta') RETURNING id").fetch_one(&pool).await.unwrap();
        let stored = admin.key.as_ref().unwrap().encrypt_field("vendors", "tax_id", other, "123-45-6789").unwrap();
        sqlx::query("UPDATE vendors SET tax_id = ?1 WHERE id = ?2").bind(stored).bind(other).execute(&pool).await.unwrap();

        assert!(remove_all_users(&pool, "remove").await.unwrap_err().starts_with("USERS_REMOVE_CONFIRM"));
        remove_all_users(&pool, REMOVE_ALL_CONFIRMATION).await.unwrap();
        assert!(!users_enabled(&pool).await.unwrap());
        let taxes: Vec<Option<String>> = sqlx::query_scalar("SELECT tax_id FROM vendors WHERE id IN (?1, ?2) ORDER BY id").bind(vendor).bind(other).fetch_all(&pool).await.unwrap();
        assert_eq!(taxes, vec![Some("plain-123".into()), None]);
        let err = sqlx::query("DELETE FROM users").execute(&pool).await;
        assert!(err.is_ok(), "nothing left to delete");

        // Users can be turned on again, with a new data key.
        let (again, _) = turn_on(&pool, "Owner", None, "owner password").await.unwrap();
        assert_ne!(again.key.unwrap().key_id(), admin.key.unwrap().key_id());
    }
}
