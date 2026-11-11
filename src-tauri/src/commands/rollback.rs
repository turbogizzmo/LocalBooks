//! Help › Reinstall Previous Version (gap R13; see `rollback.rs`).

use super::prelude::*;
use serde::Serialize;
use crate::rollback::{self, PreviousRelease};
use tauri_plugin_updater::UpdaterExt;

handlers![get_previous_release, reinstall_previous_release];

/// The release before this one and, when the open company file can't go back to it, why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    pub current_version: String,
    pub previous: Option<PreviousRelease>,
    /// Set when reinstalling would leave the open books unreadable.
    pub blocked: Option<String>,
}

async fn plan(app: &AppHandle) -> Result<(RollbackPlan, Option<sqlx::SqlitePool>), String> {
    let current = app.package_info().version.to_string();
    let previous = rollback::previous_release(&rollback::fetch_releases().await?, &current);
    let db = app.state::<Db>();
    let pool = if db.is_open().await { Some(db.pool().await) } else { None };
    let blocked = match (&previous, &pool) {
        (Some(p), Some(pool)) => {
            let known = rollback::fetch_migrations(&p.tag).await?;
            rollback::schema_blocker(p, &rollback::unknown_migrations(pool, &known).await?)
        }
        _ => None,
    };
    Ok((RollbackPlan { current_version: current, previous, blocked }, pool))
}

#[tauri::command]
pub async fn get_previous_release(app: AppHandle) -> Result<RollbackPlan, String> {
    Ok(plan(&app).await?.0)
}

/// Backs up the open company file, then installs the previous release through the updater (its
/// signature checked as for any update) and restarts into it.
#[tauri::command]
pub async fn reinstall_previous_release(app: AppHandle) -> Result<String, String> {
    let (plan, pool) = plan(&app).await?;
    let previous = plan.previous.ok_or("ROLLBACK_NONE: there is no earlier release to go back to")?;
    if let Some(reason) = plan.blocked {
        return Err(reason);
    }
    let url = tauri::Url::parse(&rollback::manifest_url(&previous.tag)).map_err(|e| format!("ROLLBACK_UPDATE: {e}"))?;
    let wanted = previous.version.clone();
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| format!("ROLLBACK_UPDATE: {e}"))?
        // Going back is the point: accept exactly the previous release, older than this one.
        .version_comparator(move |current, remote| remote.version < current && remote.version.to_string() == wanted)
        .build()
        .map_err(|e| format!("ROLLBACK_UPDATE: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("ROLLBACK_UPDATE: {e}"))?
        .ok_or_else(|| format!("ROLLBACK_UPDATE: Tillhouse {} has no installer for this computer", previous.version))?;
    // The release is there for this computer: back the books up before anything is installed.
    let backup = match pool {
        Some(_) => Some(crate::backup::backup_now(&app.state::<Db>()).await?),
        None => None,
    };
    // Downloads it, checks its signature against the key built into this app, and installs it.
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| format!("ROLLBACK_UPDATE: the download couldn't be installed ({e}). Nothing was changed"))?;
    let saved = backup.map(|b| format!(" Your books were backed up first as {}.", b.file_name)).unwrap_or_default();
    Ok(format!("Tillhouse {} is installed and starts when you restart.{saved}", previous.version))
}
