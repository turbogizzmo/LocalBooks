//! Users and signing in.

use super::prelude::*;
use super::company_files::COMPANY_FILE_CHANGED;

handlers![
    get_sign_in_state,
    sign_in_remembered,
    forget_remembered_sign_in,
    touch_session,
    sign_out_if_idle,
    turn_on_users,
    sign_in,
    sign_out,
    list_users,
    add_user,
    update_user,
    change_my_password,
    recover_with_recovery_key,
    replace_recovery_key,
    remove_all_users,
];

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInState {
    /// The file asks who is signing in.
    pub users_enabled: bool,
    pub user: Option<UserView>,
    /// Minutes without activity before signing out (0 = never), when users are on.
    pub auto_lock_minutes: u32,
    /// This computer remembers a sign-in for this company file.
    pub remembered_here: bool,
    /// The whole file is encrypted (gap R1): there is no "remove all users" way back.
    pub encrypted: bool,
}

/// Sign-ins remembered on this computer ("Remember me on this computer"), one per company file, keyed by
/// the file's data-key id: `{"username": …, "password": …}` in the operating system's credential store.
pub(super) fn remembered_sign_ins(app: &AppHandle) -> Result<SecretStore, String> {
    let (_, dir) = crate::data_dirs(app).map_err(|e| format!("SIGN_IN_CONFIG: {e}"))?;
    Ok(SecretStore::for_this_computer(&dir, "LocalBooks sign-in", "remembered-sign-ins.json", "the remembered sign-in"))
}

async fn file_key_id(pool: &sqlx::SqlitePool) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT key_id FROM data_key WHERE id = 1").fetch_optional(pool).await.map_err(ledger::db_err)
}

/// The wait after failed sign-ins (gap R27), kept in this computer's settings folder.
pub(super) fn throttle(app: &AppHandle) -> &'static crate::sign_in_throttle::Throttle {
    static THROTTLE: std::sync::OnceLock<crate::sign_in_throttle::Throttle> = std::sync::OnceLock::new();
    THROTTLE.get_or_init(|| crate::sign_in_throttle::Throttle::new(crate::data_dirs(app).ok().map(|(_, dir)| dir).as_deref()))
}

/// What names the open company file for remembered sign-ins and the sign-in wait: an encrypted
/// file's id from its key header (readable while it is locked), else its data key's id. Both are
/// shared by the file's copies.
pub(super) async fn file_identity(db: &Db, pool: &sqlx::SqlitePool) -> Result<Option<String>, String> {
    match db.encrypted_file_id().await {
        Some(id) => Ok(Some(id)),
        None => file_key_id(pool).await,
    }
}

/// Which company file a wait belongs to.
pub(super) async fn throttle_file(db: &Db, pool: &sqlx::SqlitePool) -> String {
    file_identity(db, pool).await.ok().flatten().unwrap_or_else(|| "(no key)".into())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct RememberedSignIn {
    username: String,
    password: String,
}

/// Signing in or out changes what every window may show, so each reloads (as for another file).
pub(super) fn signed_in_changed(app: &AppHandle) {
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
}

/// Whether the open company file has users and who is signed in; `None` with no company open.
#[tauri::command]
pub async fn get_sign_in_state(app: AppHandle, db: State<'_, Db>) -> Result<Option<SignInState>, String> {
    let Ok((pool, session)) = db.pool_and_session().await else { return Ok(None) };
    // A locked encrypted file has users (that is how it is unlocked) and nothing readable yet.
    if db.is_locked().await {
        let remembered_here = match db.encrypted_file_id().await {
            Some(id) => remembered_sign_ins(&app)?.get(&id).await.ok().flatten().is_some(),
            None => false,
        };
        return Ok(Some(SignInState { users_enabled: true, user: None, auto_lock_minutes: users::DEFAULT_AUTO_LOCK_MINUTES, remembered_here, encrypted: true }));
    }
    let users_enabled = users::users_enabled(&pool).await?;
    let user = session.lock().await.as_ref().map(|s| s.user.clone());
    let auto_lock_minutes = if users_enabled { users::auto_lock_minutes(&pool).await } else { 0 };
    let remembered_here = match file_identity(&db, &pool).await? {
        Some(id) if users_enabled => remembered_sign_ins(&app)?.get(&id).await.ok().flatten().is_some(),
        _ => false,
    };
    Ok(Some(SignInState { users_enabled, user, auto_lock_minutes, remembered_here, encrypted: db.is_encrypted().await }))
}

/// Startup with users on: signs in with the sign-in this computer remembers for the file, once per
/// opening (never after a sign-out or idle lock). A remembered password that no longer works is
/// forgotten. Whether someone was signed in.
#[tauri::command]
pub async fn sign_in_remembered(app: AppHandle, db: State<'_, Db>) -> Result<Option<UserView>, String> {
    if !db.take_remembered_attempt().await {
        return Ok(None);
    }
    let (pool, slot) = db.pool_and_session().await?;
    let Some(id) = file_identity(&db, &pool).await? else { return Ok(None) };
    let store = remembered_sign_ins(&app)?;
    let Some(saved) = store.get(&id).await?.and_then(|json| serde_json::from_str::<RememberedSignIn>(&json).ok()) else { return Ok(None) };
    match crate::file_cipher::user_slots::sign_in(&db, &saved.username, &saved.password).await {
        Ok(session) if !session.user.must_change_password => {
            // A remembered password never skips two-step sign-in: the sign-in screen asks for the code.
            let Some(session) = super::mfa::hold_for_code(&db, session, &saved.password, Remember::Keep).await.ok().flatten() else {
                super::mfa::expire_later(&app);
                return Ok(None);
            };
            let user = session.user.clone();
            *slot.lock().await = Some(session);
            db.refresh_gate().await;
            Ok(Some(user))
        }
        Ok(_) => {
            db.relock().await;
            store.delete(&id).await?;
            Ok(None)
        }
        Err(_) => {
            store.delete(&id).await?;
            Ok(None)
        }
    }
}

/// Settings › Users & Passwords: stop remembering the sign-in on this computer.
#[tauri::command]
pub async fn forget_remembered_sign_in(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let (pool, _) = db.pool_and_session().await?;
    if let Some(id) = file_identity(&db, &pool).await? {
        remembered_sign_ins(&app)?.delete(&id).await?;
    }
    Ok(())
}

/// Activity from a window (typing in a form sends no other command): keeps the sign-in alive. The
/// check in front of every command does the work.
#[tauri::command]
pub async fn touch_session() -> Result<(), String> {
    Ok(())
}

/// A window that has seen no activity asks whether the company's idle limit has passed (counting
/// every window); if so, signs out. Whether it did.
#[tauri::command]
pub async fn sign_out_if_idle(app: AppHandle, db: State<'_, Db>) -> Result<bool, String> {
    if !db.idle_expired() {
        return Ok(false);
    }
    sign_out_after_idle(&app).await;
    Ok(true)
}

/// The check in front of the commands found the user idle past the limit: sign them out everywhere.
pub async fn sign_out_after_idle(app: &AppHandle) {
    let Some(db) = app.try_state::<Db>() else { return };
    let Ok((_, slot)) = db.pool_and_session().await else { return };
    // Their windows are their desktop, saved for when they sign in again.
    windows::save_on_close(app).await;
    if slot.lock().await.take().is_none() {
        return; // another command got here first
    }
    db.forbid_remembered_sign_in().await;
    // An encrypted file closes again, so its page key leaves memory.
    db.relock().await;
    db.refresh_gate().await;
    signed_in_changed(app);
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersTurnedOn {
    pub user: UserView,
    /// Shown once; LocalBooks does not keep it.
    pub recovery_key: String,
}

#[tauri::command]
pub async fn turn_on_users(db: State<'_, Db>, username: String, display_name: Option<String>, password: String) -> Result<UsersTurnedOn, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let (session, recovery) = crate::file_cipher::user_slots::turn_on(&db, &username, display_name.as_deref(), &password).await?;
    // The tax IDs it just encrypted may still sit in plain in free pages and the journal.
    if let Err(err) = crate::db::compact(&pool).await {
        eprintln!("Tillhouse: could not compact the company file after turning on users: {err}");
    }
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    // No reload here: the window shows the recovery key first.
    Ok(UsersTurnedOn { user, recovery_key: recovery.as_str().to_string() })
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, db: State<'_, Db>, username: String, password: String, remember: bool) -> Result<UserView, String> {
    let (pool, _) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    let session = throttle(&app)
        .attempt(&file, crate::sign_in_throttle::Who::User(&username), |e| e.starts_with("SIGN_IN_FAILED"), crate::file_cipher::user_slots::sign_in(&db, &username, &password))
        .await?;
    let remember = if remember { Remember::Save(zeroize::Zeroizing::new(password.clone())) } else { Remember::Forget };
    // A user with two-step sign-in (gap R26) isn't signed in yet: the screen asks for their code.
    let Some(session) = super::mfa::hold_for_code(&db, session, &password, remember.clone()).await? else {
        super::mfa::expire_later(&app);
        return Err(crate::mfa::CODE_REQUIRED.into());
    };
    complete_sign_in(&app, &db, session, remember).await
}

/// What a finished sign-in does with "Remember me on this computer".
#[derive(Clone)]
pub(super) enum Remember {
    /// Remember this password for the file.
    Save(zeroize::Zeroizing<String>),
    /// Stop remembering any password for the file.
    Forget,
    /// Leave what is remembered alone (a remembered sign-in).
    Keep,
}

/// Signs `session` in to the open file: remembers (or forgets) the password, reloads every window.
pub(super) async fn complete_sign_in(app: &AppHandle, db: &Db, session: Session, remember: Remember) -> Result<UserView, String> {
    // The pool changes when an encrypted file is unlocked.
    let (pool, slot) = db.pool_and_session().await?;
    if let Some(id) = file_identity(db, &pool).await? {
        let store = remembered_sign_ins(app)?;
        match remember {
            Remember::Save(password) => {
                let saved = RememberedSignIn { username: session.user.username.clone(), password: password.to_string() };
                store.put(&id, &serde_json::to_string(&saved).map_err(|e| e.to_string())?).await?;
            }
            Remember::Forget => store.delete(&id).await?,
            Remember::Keep => {}
        }
    }
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    // As in QuickBooks, each user's desktop is their own: theirs reopens, not the last user's windows.
    windows::restore_on_next_load(app, true);
    signed_in_changed(app);
    Ok(user)
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let (_, slot) = db.pool_and_session().await?;
    // Their windows are their desktop, saved for when they sign in again.
    windows::save_on_close(&app).await;
    *slot.lock().await = None;
    // A sign-in waiting for its code ends too.
    super::mfa::forget_pending().await;
    // Signing out means someone else may sign in next: no automatic sign-in until the file reopens.
    db.forbid_remembered_sign_in().await;
    db.relock().await;
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_users(db: State<'_, Db>) -> Result<Vec<UserView>, String> {
    let (pool, slot) = db.pool_and_session().await?;
    Session::require_admin(slot.lock().await.as_ref())?;
    users::list(&pool).await
}

#[tauri::command]
pub async fn add_user(db: State<'_, Db>, input: NewUserInput) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let _ = pool;
    let guard = slot.lock().await;
    crate::file_cipher::user_slots::add_user(&db, Session::require_admin(guard.as_ref())?, input).await
}

#[tauri::command]
pub async fn update_user(db: State<'_, Db>, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let _ = pool;
    let guard = slot.lock().await;
    crate::file_cipher::user_slots::update_user(&db, Session::require_admin(guard.as_ref())?, id, input).await
}

#[tauri::command]
pub async fn change_my_password(app: AppHandle, db: State<'_, Db>, current: String, next: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    let user = {
        let mut guard = slot.lock().await;
        let session = guard.as_mut().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
        // The current password is checked with the same wait as sign-in, so an unattended session
        // can't be used to guess it.
        let name = session.user.username.clone();
        throttle(&app)
            .attempt(&file, crate::sign_in_throttle::Who::User(&name), |e| e.starts_with("PASSWORD_WRONG"), crate::file_cipher::user_slots::change_own_password(&db, session, &current, &next))
            .await?
    };
    // A temporary password no longer holds the user at the sign-in screen.
    db.refresh_gate().await;
    Ok(user)
}

/// A forgotten Admin password: the recovery key sets a new one and signs that Admin in.
#[tauri::command]
pub async fn recover_with_recovery_key(app: AppHandle, db: State<'_, Db>, recovery_key: String, username: String, new_password: String) -> Result<UserView, String> {
    let (pool, _) = db.pool_and_session().await?;
    let file = throttle_file(&db, &pool).await;
    // Only a wrong key counts: not a typo in its format, nor a right key with another user's name.
    let session = throttle(&app)
        .attempt(&file, crate::sign_in_throttle::Who::RecoveryKey, |e| e.starts_with("KEY_WRONG_RECOVERY"), crate::file_cipher::user_slots::recover(&db, &recovery_key, &username, &new_password))
        .await?;
    let (_, slot) = db.pool_and_session().await?;
    // A sign-in that was waiting for its code ends: this Admin is signed in now.
    super::mfa::forget_pending().await;
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(user)
}

#[tauri::command]
pub async fn replace_recovery_key(db: State<'_, Db>) -> Result<String, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let _ = pool;
    let recovery = crate::file_cipher::user_slots::replace_recovery_key(&db, Session::require_admin(guard.as_ref())?).await?;
    Ok(recovery.as_str().to_string())
}

/// The last resort on the sign-in screen when no Admin password or recovery key is left.
#[tauri::command]
pub async fn remove_all_users(app: AppHandle, db: State<'_, Db>, confirmation: String) -> Result<(), String> {
    let (_, slot) = db.pool_and_session().await?;
    crate::file_cipher::user_slots::remove_all_users(&db, &confirmation).await?;
    *slot.lock().await = None;
    db.refresh_gate().await;
    signed_in_changed(&app);
    notify(&app);
    Ok(())
}
