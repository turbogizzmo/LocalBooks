//! Sign-in and user changes on an encrypted company file (gap R1, phase 3a): each wraps the plain
//! operation in [`crate::users`] and keeps the key header's page-key slots in step with it. For a
//! plain company file every function here is exactly the `users` one.
//!
//! - **Signing in** unlocks the file with the password first ([`Db::unlock_with_password`]), then
//!   signs in on the opened books as usual. If the sign-in itself then fails (a turned-off user),
//!   the file is locked again, so nobody is left with an open file and no session.
//! - **A new password** (a reset, your own change, the recovery key) gets its slot **beside** the
//!   old one before the users table changes, and the old slot goes once the change is saved. A crash
//!   in between leaves both passwords able to unlock the file, and the users table decides which one
//!   signs in, so nobody is locked out by a half-saved change.
//! - **Turning a user off** removes their slot: they can no longer decrypt the file, even outside
//!   LocalBooks.
//! - **A new recovery key** replaces the recovery slot.

use crate::db::Db;
use crate::file_cipher::header::Header;
use crate::file_cipher::keys;
use crate::users::{self, NewUserInput, Session, UserUpdate, UserView};

/// `header` keeping only the newest of `user_id`'s slots (after a password change is saved).
fn keeping_newest(header: Header, user_id: i64) -> Header {
    let Some(newest) = header.slots.iter().rposition(|s| s.user_id == Some(user_id)) else { return header };
    let mut next = header;
    let keep = next.slots.remove(newest);
    next.slots.retain(|s| s.user_id != Some(user_id));
    next.slots.push(keep);
    next
}

/// `header` without the newest of `user_id`'s slots (a password change that didn't save).
fn dropping_newest(header: Header, user_id: i64) -> Header {
    let mut next = header;
    if let Some(newest) = next.slots.iter().rposition(|s| s.user_id == Some(user_id)) {
        next.slots.remove(newest);
    }
    next
}

/// Runs `change` with a slot for `password` added beside `user_id`'s current ones; keeps only the
/// new slot if `change` succeeds, and drops it if not.
async fn with_new_password<T, Fut>(db: &Db, user_id: i64, username: &str, password: &str, change: Fut) -> Result<T, String>
where
    Fut: std::future::Future<Output = Result<T, String>>,
{
    if !db.is_encrypted().await {
        return change.await;
    }
    if let Some(slot) = db.new_password_slot(user_id, username, password).await? {
        db.update_key_header(|header| keys::adding_user_slot(&header, slot)).await?;
    }
    let result = change.await;
    let tidy = match &result {
        Ok(_) => db.update_key_header(|header| keeping_newest(header, user_id)).await,
        Err(_) => db.update_key_header(|header| dropping_newest(header, user_id)).await,
    };
    if let Err(e) = tidy {
        // Both slots still open the file, and the users table decides who signs in; tidied later.
        eprintln!("LocalBooks: could not tidy the key header after a password change: {e}");
    }
    result
}

/// Signs in, unlocking an encrypted file first. The error for a wrong password or unknown user name
/// is the same whether the file is encrypted or not.
pub async fn sign_in(db: &Db, username: &str, password: &str) -> Result<Session, String> {
    let was_locked = db.is_locked().await;
    db.unlock_with_password(username, password).await?;
    let (pool, _) = db.pool_and_session().await?;
    match users::sign_in(&pool, username, password).await {
        Ok(session) => {
            if was_locked {
                // Leftover slots from an interrupted password change go now.
                let id = session.user.id;
                if let Err(e) = db.update_key_header(|header| keeping_newest(header, id)).await {
                    eprintln!("LocalBooks: could not tidy the key header: {e}");
                }
            }
            Ok(session)
        }
        Err(e) => {
            if was_locked {
                db.relock().await;
            }
            Err(e)
        }
    }
}

/// A forgotten Admin password: the recovery key unlocks an encrypted file, then sets the Admin's
/// new password as usual, and the Admin's slot follows it.
pub async fn recover(db: &Db, recovery_key: &str, username: &str, new_password: &str) -> Result<Session, String> {
    crate::crypto::check_password_rules(new_password)?;
    let was_locked = db.is_locked().await;
    db.unlock_with_recovery_key(recovery_key).await?;
    let (pool, _) = db.pool_and_session().await?;
    let session = match users::recover(&pool, recovery_key, username, new_password).await {
        Ok(session) => session,
        Err(e) => {
            if was_locked {
                db.relock().await;
            }
            return Err(e);
        }
    };
    let id = session.user.id;
    if let Some(slot) = db.new_password_slot(id, &session.user.username, new_password).await? {
        db.update_key_header(|header| keys::with_user_slot(&header, id, slot)).await?;
    }
    Ok(session)
}

/// An Admin adds a user; on an encrypted file the user gets a slot for their temporary password.
pub async fn add_user(db: &Db, admin: &Session, input: NewUserInput) -> Result<UserView, String> {
    let (pool, _) = db.pool_and_session().await?;
    let password = input.password.clone();
    let user = users::add(&pool, admin, input).await?;
    let id = user.id;
    let added = async {
        if let Some(slot) = db.new_password_slot(id, &user.username, &password).await? {
            db.update_key_header(|header| keys::with_user_slot(&header, id, slot)).await?;
        }
        Ok::<(), String>(())
    }
    .await;
    added.map_err(|e| format!("USER_NO_FILE_KEY: {} was added but can't open the encrypted company file yet ({e}); set a new temporary password for them", user.username))?;
    Ok(user)
}

/// An Admin changes a user: a new temporary password gets a slot, and a user turned off loses theirs.
pub async fn update_user(db: &Db, admin: &Session, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let (pool, _) = db.pool_and_session().await?;
    let turning_off = !input.is_active;
    let user = match input.new_password.clone() {
        Some(password) if input.is_active => {
            let username = users::list(&pool).await?.into_iter().find(|u| u.id == id).map(|u| u.username).ok_or_else(|| "USER_NOT_FOUND: no such user".to_string())?;
            with_new_password(db, id, &username, &password, users::update(&pool, admin, id, input)).await?
        }
        _ => users::update(&pool, admin, id, input).await?,
    };
    if turning_off {
        db.update_key_header(|header| keys::without_user(&header, id)).await?;
    }
    Ok(user)
}

/// The signed-in user changes their own password; their slot follows it.
pub async fn change_own_password(db: &Db, session: &mut Session, current: &str, next: &str) -> Result<UserView, String> {
    let (pool, _) = db.pool_and_session().await?;
    let (id, name) = (session.user.id, session.user.username.clone());
    with_new_password(db, id, &name, next, users::change_own_password(&pool, session, current, next)).await
}

/// An Admin issues a new recovery key; on an encrypted file it replaces the recovery slot too.
pub async fn replace_recovery_key(db: &Db, admin: &Session) -> Result<crate::crypto::RecoveryKey, String> {
    let (pool, _) = db.pool_and_session().await?;
    let recovery = users::replace_recovery_key(&pool, admin).await?;
    if let Some(slot) = db.new_recovery_slot(recovery.as_str()).await? {
        db.update_key_header(|header| keys::with_recovery_slot(&header, slot)).await?;
    }
    Ok(recovery)
}

/// Turns users on. An encrypted file already has users (that is how it is unlocked), so this is
/// refused there, rather than failing on the locked file's empty stand-in database.
pub async fn turn_on(db: &Db, username: &str, display_name: Option<&str>, password: &str) -> Result<(Session, crate::crypto::RecoveryKey), String> {
    if db.is_encrypted().await {
        return Err("USERS_ALREADY_ON: this company file is encrypted, so it already has users".into());
    }
    let (pool, _) = db.pool_and_session().await?;
    users::turn_on(&pool, username, display_name, password).await
}

/// The last resort when no Admin password or recovery key is left. Refused for an encrypted file:
/// without a password or the recovery key its books can't be read at all.
pub async fn remove_all_users(db: &Db, confirmation: &str) -> Result<(), String> {
    if db.is_encrypted().await {
        return Err("FILE_ENCRYPTED_NO_RESET: this company file is encrypted, so it can't be opened without a password or the recovery key, and removing users can't bring it back".into());
    }
    let (pool, _) = db.pool_and_session().await?;
    users::remove_all_users(&pool, confirmation).await
}

#[cfg(test)]
mod tests;
