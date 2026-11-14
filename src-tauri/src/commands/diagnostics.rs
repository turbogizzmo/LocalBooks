//! Help › Save Diagnostics: a local bundle of versions, schema and the diagnostics log.

use super::prelude::*;

handlers![
    log_screen_error,
    preview_diagnostics,
    save_diagnostics,
];

/// Notes in the diagnostics log that a screen failed to draw (`ErrorBoundary.jsx`): which window,
/// the kind of error and the components involved. The screen leaves the error's message out,
/// since the log holds no business data; the line is cut and flattened here whatever is sent.
#[tauri::command]
pub fn log_screen_error(line: String) {
    crate::diagnostics::log(&crate::diagnostics::screen_error_line(&line));
}

async fn files(app: &AppHandle, db: &Db) -> Result<Vec<(String, String)>, String> {
    let path = db.path().await.ok();
    let pool = if path.is_some() { Some(db.pool().await) } else { None };
    let logs = app.path().app_log_dir().ok();
    crate::diagnostics::bundle(pool.as_ref(), path.as_deref(), logs.as_deref(), &app.package_info().version.to_string()).await
}

/// What the bundle would hold, to read before saving: (name, text).
#[tauri::command]
pub async fn preview_diagnostics(app: AppHandle, db: State<'_, Db>) -> Result<Vec<(String, String)>, String> {
    files(&app, &db).await
}

/// Asks where to save the bundle and writes it; `None` when the dialog is cancelled.
#[tauri::command]
pub async fn save_diagnostics(app: AppHandle, db: State<'_, Db>) -> Result<Option<String>, String> {
    // Where first: nothing is gathered if the dialog is cancelled.
    let name = format!("Tillhouse diagnostics {}.zip", Local::now().format("%Y-%m-%d"));
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&name).add_filter("Zip file", &["zip"]).blocking_save_file())
        .await
        .map_err(|e| format!("DIAG_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("DIAG_BAD_PATH: {e}"))?;
    let bundle = files(&app, &db).await?;
    crate::diagnostics::write_zip(&bundle, &path)?;
    Ok(Some(path.display().to_string()))
}
