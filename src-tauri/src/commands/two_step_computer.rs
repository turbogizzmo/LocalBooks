//! Two-step sign-in on this computer (gap R29, [`crate::mfa::device`]): Touch ID or Windows Hello
//! in place of the authenticator app's code, on a computer the user added. The app stays; this is
//! only offered beside it.

use super::prelude::*;
use crate::mfa::{self, device};
use crate::platform_auth::{self, Kind};

handlers![
    get_two_step_computer_sign_in,
    finish_two_step_sign_in_with_computer,
    get_my_two_step_computers,
    add_this_computer_two_step,
    remove_two_step_computer,
];

/// This computer's credential store for sign-in secrets.
pub(super) fn store(app: &AppHandle) -> Result<SecretStore, String> {
    let (_, dir) = crate::data_dirs(app).map_err(|e| format!("SIGN_IN_CONFIG: {e}"))?;
    Ok(device::store(&dir))
}

/// The window the system's prompt belongs to (Windows opens it in front of that window).
fn parent_of(window: &tauri::WebviewWindow) -> Option<isize> {
    #[cfg(windows)]
    {
        window.hwnd().ok().map(|h| h.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

/// Asks the system to confirm the person, off the async workers (the prompt waits on them).
async fn confirm(parent: Option<isize>, reason: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || platform_auth::confirm_blocking(parent, &reason)).await.map_err(|e| format!("MFA_DEVICE: {e}"))?
}

async fn available() -> bool {
    platform_auth::kind() != Kind::None && tokio::task::spawn_blocking(platform_auth::available_blocking).await.unwrap_or(false)
}

/// At the code step: which check this computer offers for the held sign-in (`TOUCH_ID` or
/// `WINDOWS_HELLO`), or `None` when it isn't set up here for that user, so the screen shows only
/// the code box.
#[tauri::command]
pub async fn get_two_step_computer_sign_in(app: AppHandle, db: State<'_, Db>) -> Result<Option<Kind>, String> {
    let Some((user_id, _, _)) = super::mfa::held_sign_in(&db).await else { return Ok(None) };
    if platform_auth::kind() == Kind::None {
        return Ok(None);
    }
    let (pool, _) = db.pool_and_session().await?;
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    let here = device::set_up_here(&mut conn, &store(&app)?, user_id).await?;
    Ok(here.then_some(platform_auth::kind()))
}

/// The code step passed with Touch ID or Windows Hello: the system confirms the person, then this
/// computer's secret is checked against the company file, and the held sign-in finishes.
#[tauri::command]
pub async fn finish_two_step_sign_in_with_computer(app: AppHandle, window: tauri::WebviewWindow, db: State<'_, Db>) -> Result<UserView, String> {
    let (user_id, username, started) = super::mfa::held_sign_in(&db).await.ok_or_else(|| "MFA_NO_SIGN_IN: sign in with your password first".to_string())?;
    let (pool, _) = db.pool_and_session().await?;
    let store = store(&app)?;
    {
        let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
        if !device::set_up_here(&mut conn, &store, user_id).await? {
            return Err(device::NOT_HERE.into());
        }
    }
    confirm(parent_of(&window), format!("sign in to LocalBooks as {username}")).await?;
    device::check(&pool, &store, user_id).await?;
    super::mfa::finish_held(&app, &db, started).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyComputers {
    /// What this computer offers (`NONE` on Linux).
    pub kind: Kind,
    /// The system can ask right now (Touch ID or Windows Hello is set up).
    pub available: bool,
    /// The name this computer is added under.
    pub computer_name: String,
    pub computers: Vec<device::Computer>,
}

/// Settings › Users & Passwords: the computers set up to sign the signed-in user in.
#[tauri::command]
pub async fn get_my_two_step_computers(app: AppHandle, db: State<'_, Db>) -> Result<MyComputers, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let user_id = slot.lock().await.as_ref().map(|s| s.user.id).ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    let computers = device::list(&mut conn, &store(&app)?, user_id).await?;
    Ok(MyComputers { kind: platform_auth::kind(), available: available().await, computer_name: crate::storage::host_name(), computers })
}

/// Adds this computer for the signed-in user: their password again (with the sign-in wait), then
/// the system's own check, then the secret is saved here and its hash in the company file.
#[tauri::command]
pub async fn add_this_computer_two_step(app: AppHandle, window: tauri::WebviewWindow, db: State<'_, Db>, password: String) -> Result<device::Computer, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let file = super::users::throttle_file(&db, &pool).await;
    let (user_id, name) = slot.lock().await.as_ref().map(|s| (s.user.id, s.user.username.clone())).ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    super::users::throttle(&app)
        .attempt(&file, crate::sign_in_throttle::Who::User(&name), |e| e.starts_with("PASSWORD_WRONG"), mfa::check_own_password(&pool, user_id, &password))
        .await?;
    if !available().await {
        return Err(platform_auth::UNSUPPORTED.into());
    }
    // The prompt can take a while: nothing is held locked meanwhile.
    confirm(parent_of(&window), "let this computer sign you in to LocalBooks".into()).await?;
    let guard = slot.lock().await;
    let session = guard.as_ref().filter(|s| s.user.id == user_id).ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    device::add(&pool, &store(&app)?, session, platform_auth::kind(), &crate::storage::host_name()).await
}

/// Removes one of the signed-in user's computers (this one or another). The app stays.
#[tauri::command]
pub async fn remove_two_step_computer(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let session = guard.as_ref().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
    device::remove(&pool, &store(&app)?, session, id).await
}
