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
    /// This Mac remembers a sign-in for this company file.
    pub remembered_here: bool,
}

/// Sign-ins remembered on this computer ("Remember me on this Mac"), one per company file, keyed by
/// the file's data-key id: `{"username": …, "password": …}` in the operating system's credential store.
fn remembered_sign_ins(app: &AppHandle) -> Result<SecretStore, String> {
    let (_, dir) = crate::data_dirs(app).map_err(|e| format!("SIGN_IN_CONFIG: {e}"))?;
    Ok(SecretStore::for_this_computer(&dir, "LocalBooks sign-in", "remembered-sign-ins.json", "the remembered sign-in"))
}

async fn file_key_id(pool: &sqlx::SqlitePool) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT key_id FROM data_key WHERE id = 1").fetch_optional(pool).await.map_err(ledger::db_err)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RememberedSignIn {
    username: String,
    password: String,
}

/// Signing in or out changes what every window may show, so each reloads (as for another file).
fn signed_in_changed(app: &AppHandle) {
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
}

/// Whether the open company file has users and who is signed in; `None` with no company open.
#[tauri::command]
pub async fn get_sign_in_state(app: AppHandle, db: State<'_, Db>) -> Result<Option<SignInState>, String> {
    let Ok((pool, session)) = db.pool_and_session().await else { return Ok(None) };
    let users_enabled = users::users_enabled(&pool).await?;
    let user = session.lock().await.as_ref().map(|s| s.user.clone());
    let auto_lock_minutes = if users_enabled { users::auto_lock_minutes(&pool).await } else { 0 };
    let remembered_here = match file_key_id(&pool).await? {
        Some(id) if users_enabled => remembered_sign_ins(&app)?.get(&id).await.ok().flatten().is_some(),
        _ => false,
    };
    Ok(Some(SignInState { users_enabled, user, auto_lock_minutes, remembered_here }))
}

/// Startup with users on: signs in with the sign-in this Mac remembers for the file, once per
/// opening (never after a sign-out or idle lock). A remembered password that no longer works is
/// forgotten. Whether someone was signed in.
#[tauri::command]
pub async fn sign_in_remembered(app: AppHandle, db: State<'_, Db>) -> Result<Option<UserView>, String> {
    if !db.take_remembered_attempt().await {
        return Ok(None);
    }
    let (pool, slot) = db.pool_and_session().await?;
    let Some(id) = file_key_id(&pool).await? else { return Ok(None) };
    let store = remembered_sign_ins(&app)?;
    let Some(saved) = store.get(&id).await?.and_then(|json| serde_json::from_str::<RememberedSignIn>(&json).ok()) else { return Ok(None) };
    match users::sign_in(&pool, &saved.username, &saved.password).await {
        Ok(session) if !session.user.must_change_password => {
            let user = session.user.clone();
            *slot.lock().await = Some(session);
            db.refresh_gate().await;
            Ok(Some(user))
        }
        _ => {
            store.delete(&id).await?;
            Ok(None)
        }
    }
}

/// Settings › Users & Passwords: stop remembering the sign-in on this Mac.
#[tauri::command]
pub async fn forget_remembered_sign_in(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let (pool, _) = db.pool_and_session().await?;
    if let Some(id) = file_key_id(&pool).await? {
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
    if slot.lock().await.take().is_none() {
        return; // another command got here first
    }
    db.forbid_remembered_sign_in().await;
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
    let (session, recovery) = users::turn_on(&pool, &username, display_name.as_deref(), &password).await?;
    // The tax IDs it just encrypted may still sit in plain in free pages and the journal.
    if let Err(err) = crate::db::compact(&pool).await {
        eprintln!("LocalBooks: could not compact the company file after turning on users: {err}");
    }
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    // No reload here: the window shows the recovery key first.
    Ok(UsersTurnedOn { user, recovery_key: recovery.as_str().to_string() })
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, db: State<'_, Db>, username: String, password: String, remember: bool) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let session = users::sign_in(&pool, &username, &password).await?;
    if let Some(id) = file_key_id(&pool).await? {
        let store = remembered_sign_ins(&app)?;
        if remember {
            let saved = RememberedSignIn { username: session.user.username.clone(), password };
            store.put(&id, &serde_json::to_string(&saved).map_err(|e| e.to_string())?).await?;
        } else {
            store.delete(&id).await?;
        }
    }
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(user)
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let (_, slot) = db.pool_and_session().await?;
    *slot.lock().await = None;
    // Signing out means someone else may sign in next: no automatic sign-in until the file reopens.
    db.forbid_remembered_sign_in().await;
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
    let guard = slot.lock().await;
    users::add(&pool, Session::require_admin(guard.as_ref())?, input).await
}

#[tauri::command]
pub async fn update_user(db: State<'_, Db>, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    users::update(&pool, Session::require_admin(guard.as_ref())?, id, input).await
}

#[tauri::command]
pub async fn change_my_password(db: State<'_, Db>, current: String, next: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let user = {
        let mut guard = slot.lock().await;
        let session = guard.as_mut().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
        users::change_own_password(&pool, session, &current, &next).await?
    };
    // A temporary password no longer holds the user at the sign-in screen.
    db.refresh_gate().await;
    Ok(user)
}

/// A forgotten Admin password: the recovery key sets a new one and signs that Admin in.
#[tauri::command]
pub async fn recover_with_recovery_key(app: AppHandle, db: State<'_, Db>, recovery_key: String, username: String, new_password: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let session = users::recover(&pool, &recovery_key, &username, &new_password).await?;
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
    let recovery = users::replace_recovery_key(&pool, Session::require_admin(guard.as_ref())?).await?;
    Ok(recovery.as_str().to_string())
}

/// The last resort on the sign-in screen when no Admin password or recovery key is left.
#[tauri::command]
pub async fn remove_all_users(app: AppHandle, db: State<'_, Db>, confirmation: String) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    users::remove_all_users(&pool, &confirmation).await?;
    *slot.lock().await = None;
    db.refresh_gate().await;
    signed_in_changed(&app);
    notify(&app);
    Ok(())
}
