//! Two-step sign-in (gap R26, [`crate::mfa`]): the code step of signing in, setting up an
//! authenticator app, and the Admin's company setting and resets.
//!
//! A user with an authenticator who gives the right password is **not signed in yet**: the session
//! is held here, in memory only, with their opened secret, until the right code arrives (or five
//! minutes pass, or they cancel). The window shows the code step meanwhile; nothing in the books
//! can be read, because the gate sees nobody signed in. On an encrypted company file the password
//! has already unlocked the file at that point (its key slots need only the password); cancelling
//! locks it again.

use super::prelude::*;
use super::users::{complete_sign_in, signed_in_changed, throttle, throttle_file, Remember};
use crate::db::SessionSlot;
use crate::mfa::{self, PendingSetup, Policy, Secret, SetupView};
use crate::sign_in_throttle::Who;

handlers![
    get_two_step_sign_in,
    finish_two_step_sign_in,
    cancel_two_step_sign_in,
    begin_two_step_setup,
    finish_two_step_setup,
    get_my_two_step,
    remove_my_two_step,
    get_two_step_policy,
    set_two_step_policy,
    reset_two_step,
];

/// How long the code step waits for a code before the password has to be given again.
const CODE_STEP_LIMIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// How long a set-up waits for its first code.
const SETUP_LIMIT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// A sign-in whose password was right, waiting for its code.
struct PendingSignIn {
    /// The open company file's sign-in slot: a code is only accepted for the file it started on.
    slot: SessionSlot,
    session: Session,
    secret: Secret,
    remember: Remember,
    started: std::time::Instant,
}

static PENDING: tokio::sync::Mutex<Option<PendingSignIn>> = tokio::sync::Mutex::const_new(None);

struct Setup {
    slot: SessionSlot,
    pending: PendingSetup,
    started: std::time::Instant,
}

static SETUP: tokio::sync::Mutex<Option<Setup>> = tokio::sync::Mutex::const_new(None);

/// Forgets a sign-in waiting for its code (signing out, another sign-in starting).
pub(super) async fn forget_pending() {
    PENDING.lock().await.take();
    SETUP.lock().await.take();
}

/// After a right password: `Some(session)` to sign in now when the user has no authenticator;
/// otherwise holds the session for the code step and returns `None`. A secret that can't be opened
/// is refused (and an encrypted file locked again): it never lets the user past the code.
pub(super) async fn hold_for_code(db: &Db, session: Session, password: &str, remember: Remember) -> Result<Option<Session>, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let secret = {
        let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
        mfa::open_secret(&mut conn, session.user.id, password).await
    };
    match secret {
        Ok(None) => {
            PENDING.lock().await.take();
            Ok(Some(session))
        }
        Ok(Some(secret)) => {
            *PENDING.lock().await = Some(PendingSignIn { slot, session, secret, remember, started: std::time::Instant::now() });
            Ok(None)
        }
        Err(e) => {
            db.relock().await;
            Err(e)
        }
    }
}

/// Ends a held sign-in when its time runs out, even if nobody comes back to the screen: an encrypted
/// file the password unlocked locks again, so its page key doesn't stay in memory.
pub(super) fn expire_later(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CODE_STEP_LIMIT).await;
        expire_if_stale(&app.state::<Db>(), CODE_STEP_LIMIT).await;
    });
}

/// Forgets a held sign-in past its time and locks an encrypted file again when nobody is signed in.
async fn expire_if_stale(db: &Db, limit: std::time::Duration) {
    let stale = {
        let mut pending = PENDING.lock().await;
        let stale = pending.as_ref().is_some_and(|p| p.started.elapsed() >= limit);
        if stale {
            pending.take();
        }
        stale
    };
    if !stale {
        return;
    }
    let signed_in = match db.pool_and_session().await {
        Ok((_, slot)) => slot.lock().await.is_some(),
        Err(_) => true,
    };
    if !signed_in {
        db.relock().await;
    }
}

/// The user name a sign-in on this file is waiting on a code for, if any (the sign-in screen
/// shows the code step for it, also after a remembered sign-in).
#[tauri::command]
pub async fn get_two_step_sign_in(db: State<'_, Db>) -> Result<Option<String>, String> {
    let Ok((_, slot)) = db.pool_and_session().await else { return Ok(None) };
    let pending = PENDING.lock().await;
    Ok(pending
        .as_ref()
        .filter(|p| std::sync::Arc::ptr_eq(&p.slot, &slot) && p.started.elapsed() < CODE_STEP_LIMIT)
        .map(|p| p.session.user.username.clone()))
}

/// The code step: an authenticator code or a backup code finishes the sign-in. Wrong codes are
/// throttled like passwords (four free tries, then a growing wait) and, past the free tries,
/// recorded in the audit trail.
#[tauri::command]
pub async fn finish_two_step_sign_in(app: AppHandle, db: State<'_, Db>, code: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    let mut guard = PENDING.lock().await;
    let pending = guard.as_ref().filter(|p| std::sync::Arc::ptr_eq(&p.slot, &slot)).ok_or_else(|| "MFA_NO_SIGN_IN: sign in with your password first".to_string())?;
    if pending.started.elapsed() >= CODE_STEP_LIMIT || db.is_locked().await {
        guard.take();
        drop(guard);
        db.relock().await;
        return Err("MFA_EXPIRED: that took too long; sign in with your password again".into());
    }
    if slot.lock().await.is_some() {
        // Someone else signed in meanwhile (the recovery key, say): this held sign-in is stale.
        guard.take();
        return Err("MFA_NO_SIGN_IN: sign in with your password first".into());
    }
    let (user_id, username) = (pending.session.user.id, pending.session.user.username.clone());
    let checked = throttle(&app)
        .attempt(&file, Who::Code(&username), |e| e.starts_with("MFA_CODE_WRONG"), mfa::check_code(&pool, user_id, &pending.secret, &code, mfa::now_unix()))
        .await;
    if let Err(e) = checked {
        if e.starts_with("MFA_CODE_WRONG") && throttle(&app).failures(&file, Who::Code(&username)) >= crate::sign_in_throttle::FREE_TRIES {
            if let Err(audit) = mfa::record_failed(&pool, user_id, &username).await {
                eprintln!("LocalBooks: could not record failed two-step codes in the audit trail: {audit}");
            }
        }
        return Err(e);
    }
    let pending = guard.take().expect("checked above");
    drop(guard);
    complete_sign_in(&app, &db, pending.session, pending.remember).await
}

/// The user a sign-in on this file is held for, if it is still current: `(id, user name, started)`.
pub(super) async fn held_sign_in(db: &Db) -> Option<(i64, String, std::time::Instant)> {
    let (_, slot) = db.pool_and_session().await.ok()?;
    let pending = PENDING.lock().await;
    pending
        .as_ref()
        .filter(|p| std::sync::Arc::ptr_eq(&p.slot, &slot) && p.started.elapsed() < CODE_STEP_LIMIT)
        .map(|p| (p.session.user.id, p.session.user.username.clone(), p.started))
}

/// Finishes the held sign-in `started` at that moment, once its second step was passed another way
/// (this computer's Touch ID or Windows Hello, gap R29). Refused when it ran out of time, was
/// cancelled or replaced meanwhile, or someone else signed in.
pub(super) async fn finish_held(app: &AppHandle, db: &Db, started: std::time::Instant) -> Result<UserView, String> {
    let (_, slot) = db.pool_and_session().await?;
    let mut guard = PENDING.lock().await;
    let current = guard.as_ref().is_some_and(|p| std::sync::Arc::ptr_eq(&p.slot, &slot) && p.started == started);
    if !current {
        return Err("MFA_NO_SIGN_IN: sign in with your password first".into());
    }
    if started.elapsed() >= CODE_STEP_LIMIT || db.is_locked().await {
        guard.take();
        drop(guard);
        db.relock().await;
        return Err("MFA_EXPIRED: that took too long; sign in with your password again".into());
    }
    if slot.lock().await.is_some() {
        guard.take();
        return Err("MFA_NO_SIGN_IN: sign in with your password first".into());
    }
    let pending = guard.take().expect("checked above");
    drop(guard);
    complete_sign_in(app, db, pending.session, pending.remember).await
}

/// Back to the sign-in screen from the code step: forgets the held sign-in and locks an
/// encrypted file again.
#[tauri::command]
pub async fn cancel_two_step_sign_in(db: State<'_, Db>) -> Result<(), String> {
    let had = PENDING.lock().await.take().is_some();
    let signed_in = match db.pool_and_session().await {
        Ok((_, slot)) => slot.lock().await.is_some(),
        Err(_) => false,
    };
    if had && !signed_in {
        db.relock().await;
    }
    Ok(())
}

/// Re-reads the signed-in user (two-step status changed) and recomputes the gate.
async fn refresh_signed_in(db: &Db) -> Result<Option<UserView>, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let mut guard = slot.lock().await;
    let Some(session) = guard.as_mut() else { return Ok(None) };
    if let Some(user) = users::list(&pool).await?.into_iter().find(|u| u.id == session.user.id) {
        session.user = user;
    }
    let user = session.user.clone();
    drop(guard);
    db.refresh_gate().await;
    Ok(Some(user))
}

/// Settings › Users & Passwords › Two-step sign-in › Set up: after the password again, a new secret
/// as a QR code and as text. Nothing is saved until [`finish_two_step_setup`].
#[tauri::command]
pub async fn begin_two_step_setup(app: AppHandle, db: State<'_, Db>, password: String) -> Result<SetupView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    let (pending, view) = {
        let guard = slot.lock().await;
        let session = guard.as_ref().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
        let name = session.user.username.clone();
        // The password is checked with the same wait as sign-in, so an unattended session can't
        // be used to guess it.
        throttle(&app).attempt(&file, Who::User(&name), |e| e.starts_with("PASSWORD_WRONG"), mfa::begin_setup(&pool, session, &password)).await?
    };
    *SETUP.lock().await = Some(Setup { slot, pending, started: std::time::Instant::now() });
    Ok(view)
}

/// The first code from the app proves it saved the secret: saves it and returns the ten backup
/// codes, shown once.
#[tauri::command]
pub async fn finish_two_step_setup(db: State<'_, Db>, code: String) -> Result<Vec<String>, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let me = slot.lock().await.as_ref().map(|s| s.user.id).ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    let codes = {
        let mut guard = SETUP.lock().await;
        let setup = guard
            .as_ref()
            .filter(|s| std::sync::Arc::ptr_eq(&s.slot, &slot) && s.pending.user_id == me && s.started.elapsed() < SETUP_LIMIT)
            .ok_or_else(|| "MFA_SETUP_EXPIRED: start setting up two-step sign-in again".to_string())?;
        let codes = mfa::finish_setup(&pool, &setup.pending, &code, mfa::now_unix()).await?;
        guard.take();
        codes
    };
    refresh_signed_in(&db).await?;
    Ok(codes)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyTwoStep {
    pub policy: Policy,
    pub enabled: bool,
    /// The company requires it for this user, so they can't turn it off themselves.
    pub required: bool,
    pub backup_codes_left: i64,
}

/// The signed-in user's own two-step sign-in, for Settings › Users & Passwords.
#[tauri::command]
pub async fn get_my_two_step(db: State<'_, Db>) -> Result<MyTwoStep, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let session = guard.as_ref().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    let policy = mfa::policy(&mut conn).await?;
    Ok(MyTwoStep {
        policy,
        enabled: mfa::has_factor(&mut conn, session.user.id).await?,
        required: policy.required_for(session.user.role),
        backup_codes_left: mfa::backup_codes_left(&mut conn, session.user.id).await?,
    })
}

/// The signed-in user turns their own two-step sign-in off (when the company doesn't require it).
#[tauri::command]
pub async fn remove_my_two_step(app: AppHandle, db: State<'_, Db>, password: String) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    {
        let guard = slot.lock().await;
        let session = guard.as_ref().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
        let name = session.user.username.clone();
        // This computer's sign-in secrets (gap R29) go with the app; read them before the rows go.
        let computers = super::two_step_computer::store(&app).ok();
        let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
        let ids = match &computers {
            Some(store) => mfa::device::here_keys(&mut conn, store, session.user.id).await?,
            None => Vec::new(),
        };
        drop(conn);
        throttle(&app).attempt(&file, Who::User(&name), |e| e.starts_with("PASSWORD_WRONG"), mfa::remove_own(&pool, session, &password)).await?;
        if let Some(store) = &computers {
            mfa::device::forget_keys(store, &ids).await;
        }
    }
    refresh_signed_in(&db).await?;
    Ok(())
}

#[tauri::command]
pub async fn get_two_step_policy(db: State<'_, Db>) -> Result<Policy, String> {
    let (pool, _) = db.pool_and_session().await?;
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    mfa::policy(&mut conn).await
}

/// Admin: the company's two-step sign-in setting. Off removes everyone's authenticator.
#[tauri::command]
pub async fn set_two_step_policy(app: AppHandle, db: State<'_, Db>, policy: Policy) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    {
        let guard = slot.lock().await;
        mfa::set_policy(&pool, Session::require_admin(guard.as_ref())?, policy).await?;
    }
    refresh_signed_in(&db).await?;
    // A stricter setting may now hold this Admin at the set-up step; every window reloads.
    signed_in_changed(&app);
    Ok(())
}

/// Admin: **Reset two-step sign-in** for a user who lost their phone.
#[tauri::command]
pub async fn reset_two_step(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    let mine = {
        let guard = slot.lock().await;
        let admin = Session::require_admin(guard.as_ref())?;
        mfa::reset_by_admin(&pool, admin, id).await?;
        admin.user.id == id
    };
    refresh_signed_in(&db).await?;
    if mine {
        signed_in_changed(&app);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
