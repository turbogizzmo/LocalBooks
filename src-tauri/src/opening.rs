//! Opening a company file without a blank or frozen window (gap R28, `docs/plans/opening-screen.md`).
//!
//! Every path that makes a company file live (the launch, File › Open, the recent list, a second
//! launch's file, Restore, New Company, Change Company File Location, Close Company) takes its turn
//! with [`begin`]. Only one open runs at a time, and while one does, the window is told which file
//! it is and what step it is on (the `company-opening` event, `null` when it ends), so it shows
//! *Opening <name>…* instead of nothing. The open path reports its slow steps with [`note`].

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Emitted with an [`OpeningState`] while a company file opens (each time its step changes), and
/// with `null` when the open ends, whether it worked or not.
pub const COMPANY_OPENING: &str = "company-opening";

/// The step shown until the open path reports a slower one.
pub const STEP_OPENING: &str = "Opening the company file";
/// The pre-upgrade copy (see `db::prepare`).
pub const STEP_SAVING_COPY: &str = "Saving a copy before updating the company file";
/// Schema migrations.
pub const STEP_UPDATING: &str = "Updating the company file for this version";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningState {
    /// What to call the file: its company name from the recent list, or else its file name.
    pub file_name: String,
    pub step: String,
}

/// Managed state: the open in progress, if any, and the turn every open waits for.
#[derive(Default)]
pub struct Opening {
    state: Mutex<Option<OpeningState>>,
    turn: Arc<tokio::sync::Mutex<()>>,
}

impl Opening {
    pub fn current(&self) -> Option<OpeningState> {
        self.state.lock().expect("opening state").clone()
    }

    /// Waits for any open in progress, then holds off the next one, without announcing anything
    /// (Close Company opens nothing).
    pub async fn wait_turn(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.turn.clone().lock_owned().await
    }
}

/// The app, for [`note`], which the open path calls without one.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Lets [`note`] reach the window. Called once, in `setup`.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// An open's turn. Dropping it ends the open: the state clears and the window hears `null`.
pub struct Turn {
    app: AppHandle,
    _turn: tokio::sync::OwnedMutexGuard<()>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        set(&self.app, None);
    }
}

/// Refused while the window shows why the launch could not open the company file.
pub const STARTUP_FAILED: &str = "STARTUP_FAILED: the company file could not be opened at launch; quit LocalBooks and open it again";

/// Waits for any open in progress to finish, then announces this one: `path`, called `name` (the
/// company name, when it is known), on its first step. Refused once the launch's open has failed:
/// the window is showing why, and nothing else may open.
pub async fn begin(app: &AppHandle, path: &Path, name: Option<String>) -> Result<Turn, String> {
    let turn = app.state::<Opening>().turn.clone().lock_owned().await;
    if app.try_state::<crate::commands::StartupFailure>().is_some() {
        return Err(STARTUP_FAILED.into());
    }
    set(app, Some(OpeningState { file_name: display_name(path, name), step: STEP_OPENING.to_string() }));
    Ok(Turn { app: app.clone(), _turn: turn })
}

/// The launch's turn, taken in `setup` before the window can ask what is opening and before a second
/// launch can hand a file over (nothing else can hold the turn yet). `file`: the file about to open
/// and its company name, announced; `None` when the company was left closed, which still holds the
/// turn until the launch is done, so a file handed over meanwhile opens after it, not under it.
pub fn begin_at_launch(app: &AppHandle, file: Option<(&Path, Option<String>)>) -> Turn {
    let opening = app.state::<Opening>();
    let turn = opening.turn.clone().try_lock_owned().expect("nothing opens before the launch's open");
    if let Some((path, name)) = file {
        set(app, Some(OpeningState { file_name: display_name(path, name), step: STEP_OPENING.to_string() }));
    }
    Turn { app: app.clone(), _turn: turn }
}

/// The open in progress is now on `step`. Does nothing when no open is in progress (a backup or a
/// check that opens a file on the side).
pub fn note(step: &str) {
    let Some(app) = APP.get() else { return };
    let Some(opening) = app.try_state::<Opening>() else { return };
    let changed = {
        let mut state = opening.state.lock().expect("opening state");
        match state.as_mut() {
            Some(current) if current.step != step => {
                current.step = step.to_string();
                Some(current.clone())
            }
            _ => None,
        }
    };
    if let Some(state) = changed {
        let _ = app.emit(COMPANY_OPENING, Some(state));
    }
}

fn set(app: &AppHandle, state: Option<OpeningState>) {
    if let Some(opening) = app.try_state::<Opening>() {
        *opening.state.lock().expect("opening state") = state.clone();
    }
    let _ = app.emit(COMPANY_OPENING, state);
}

/// `name` when there is one, else the file's name without its folder (`localbooks.db` for the
/// usual company file, which says little, so its folder's name is added).
fn display_name(path: &Path, name: Option<String>) -> String {
    if let Some(name) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        return name;
    }
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
    if file == crate::storage::DB_FILE_NAME {
        if let Some(folder) = path.parent().and_then(|p| p.file_name()) {
            return format!("{file} in {}", folder.to_string_lossy());
        }
    }
    file
}

/// The company name the recent list (in `config_dir`) has for `path`, if any.
pub fn recent_name(config_dir: &Path, path: &Path) -> Option<String> {
    crate::storage::read_config(config_dir).recent_companies.into_iter().find(|r| crate::storage::same_file(&r.path, path)).and_then(|r| r.company_name)
}

#[cfg(test)]
mod tests;
