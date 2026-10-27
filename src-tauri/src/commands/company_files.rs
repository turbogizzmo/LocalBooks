//! Company files: location, opening and closing, the No Company Open screen, window titles and startup failure.

use super::prelude::*;
use crate::company_setup::{self, CompanySetup, SetupOptions, StarterAccount};

handlers![
    get_storage_status,
    inspect_database_directory,
    update_database_directory,
    open_window,
    open_company_file,
    get_start_screen,
    forget_recent_company,
    find_company_files,
    create_company_file,
    get_company_setup_options,
    get_starter_chart,
    restore_backup_as,
    close_company_file,
    get_startup_failure,
];

#[tauri::command]
/// The open company file, or `None` when no company is open (the No Company Open screen).
pub async fn get_storage_status(db: State<'_, Db>) -> Result<Option<StorageStatus>, String> {
    Ok(db.status().await)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryInspection {
    pub directory: String,
    pub target_path: String,
    pub storage: StorageInfo,
    pub has_company_file: bool,
    /// Present when a company file exists there and passed every check.
    pub company_file: Option<CompanyFileInfo>,
    /// Why the folder (or the company file in it) cannot be used.
    pub problem: Option<String>,
}

/// Looks at a candidate folder without changing anything, so the UI can explain the choice.
#[tauri::command]
pub async fn inspect_database_directory(new_path: String) -> Result<DirectoryInspection, String> {
    let dir = storage::resolve_network_path(&PathBuf::from(&new_path));
    let target = dir.join(storage::DB_FILE_NAME);
    let has_company_file = target.is_file();
    let mut problem = storage::check_writable_dir(&dir).err();
    let company_file = if has_company_file && problem.is_none() {
        match backup::inspect(&target).await {
            Ok(info) => Some(info),
            Err(err) => {
                problem = Some(err);
                None
            }
        }
    } else {
        None
    };
    Ok(DirectoryInspection {
        directory: dir.display().to_string(),
        target_path: target.display().to_string(),
        storage: storage::classify(&dir),
        has_company_file,
        company_file,
        problem,
    })
}

/// Points LocalBooks at `new_path/localbooks.db`. An existing file there is verified and opened;
/// otherwise `mode` chooses between `NEW` (initialize an empty company file, the default) and `MOVE`
/// (copy the current books there). The choice is saved to config.json.
#[tauri::command]
pub async fn update_database_directory(app: AppHandle, db: State<'_, Db>, new_path: String, mode: Option<String>) -> Result<StorageStatus, String> {
    let dir = storage::resolve_network_path(&PathBuf::from(new_path));
    let mode = RelocateMode::parse(mode.as_deref(), dir.join(storage::DB_FILE_NAME).is_file())?;
    let status = db.relocate(&dir, mode).await?;
    company_changed(&app).await;
    notify(&app);
    Ok(status)
}

/// Emitted when a different company file becomes the live one; the window reloads its state.
pub const COMPANY_FILE_CHANGED: &str = "company-file-changed";

/// After a different company file becomes the live one (or none does): retitles every window,
/// remembers the file at the top of the recent list (and whether the company was left closed, for
/// the next launch), and refreshes File › Open Previous Company and the Window and View menus.
pub async fn company_changed(app: &AppHandle) {
    let Some(db) = app.try_state::<Db>() else { return };
    let status = db.status().await;
    windows::set_company(app, status.is_some(), status.as_ref().and_then(|s| s.company_name.clone()));
    let updated = storage::update_config(db.config_dir(), |config| {
        config.start_with_no_company = status.is_none();
        if let Some(status) = &status {
            storage::add_recent(&mut config.recent_companies, RecentCompany::opened_now(Path::new(&status.path), status.company_name.clone()));
        }
    });
    if let Err(err) = updated {
        eprintln!("cannot update the recent company list: {err}");
    }
    crate::file_menu::refresh(app, &recent_companies(&db), status.is_some());
    windows::refresh_menus(app).await;
}

fn recent_companies(db: &Db) -> Vec<RecentCompanyView> {
    storage::describe_recent(&storage::read_config(db.config_dir()).recent_companies)
}

/// Window › New Window, or a form, register or report opened in its own window: another window on
/// the open company, starting on `view` (a navigation id) with the given request, if any. With
/// `reuse`, a window already showing that page is brought forward instead.
#[tauri::command]
pub async fn open_window(app: AppHandle, view: Option<String>, action: Option<String>, data: Option<serde_json::Value>, reuse: Option<bool>) -> Result<String, String> {
    if !app.state::<Db>().is_open().await {
        return Err(NO_COMPANY_OPEN.into());
    }
    let page = view.map(|view| crate::desktop::Page { view, action, data: data.filter(|d| !d.is_null()) });
    if let Some(window) = page.as_ref().filter(|_| reuse == Some(true)).and_then(|p| windows::showing(&app, p)) {
        windows::focus_window(&window);
        return Ok(window.label().to_string());
    }
    windows::open(&app, page.as_ref(), None)
}

/// Makes `path` the live company file for this session and tells the window to reload.
///
/// The company open until now closes first, so its desktop is saved (Desktop View preferences).
/// Once the file is open, every window but one closes, and the first window to load reopens the new
/// company's saved desktop. `windowless` opens it without restoring windows, as QuickBooks does when
/// Alt is held, so a page that fails to load does not come back (the page that asked lands on a
/// plain home page; see `CompanyFiles.jsx`).
pub async fn mount_company_file(app: &AppHandle, path: &Path, windowless: bool) -> Result<StorageStatus, String> {
    windows::save_on_close(app).await;
    let status = app.state::<Db>().mount(path).await?;
    windows::close_extra(app);
    windows::restore_on_next_load(app, !windowless);
    company_changed(app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    notify(app);
    Ok(status)
}

/// Carries out a request handed over by the operating system or a second launch: open the file
/// (reporting any problem in the app's banner) and bring the window forward.
pub async fn handle_hand_off(app: &AppHandle, request: crate::instance::HandOff) {
    if app.try_state::<StartupFailure>().is_some() {
        return; // no company file is open; the window is showing why
    }
    if let crate::instance::HandOff::Open { path } = request {
        if let Err(err) = mount_company_file(app, &path, false).await {
            app.state::<Db>().set_warning(format!("Could not open {}: {err}.", path.display()));
            let _ = app.emit(COMPANY_FILE_CHANGED, ());
        }
    }
    windows::focus(app);
}

/// File › Open: opens a company file (a `.localbooks` file or a LocalBooks backup) in place;
/// `windowless` opens it without restoring windows.
#[tauri::command]
pub async fn open_company_file(app: AppHandle, path: String, windowless: Option<bool>) -> Result<StorageStatus, String> {
    mount_company_file(&app, &storage::resolve_network_path(&PathBuf::from(path)), windowless.unwrap_or(false)).await
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScreen {
    pub recent: Vec<RecentCompanyView>,
    /// Why the last file asked for could not be opened, if it could not.
    pub warning: Option<String>,
}

/// What the No Company Open screen lists.
#[tauri::command]
pub async fn get_start_screen(db: State<'_, Db>) -> Result<StartScreen, String> {
    Ok(StartScreen { recent: recent_companies(&db), warning: db.warning() })
}

/// Edit List: takes a company file off the recent list. The file itself is left alone.
#[tauri::command]
pub async fn forget_recent_company(app: AppHandle, db: State<'_, Db>, path: String) -> Result<Vec<RecentCompanyView>, String> {
    storage::update_config(db.config_dir(), |config| {
        storage::remove_recent(&mut config.recent_companies, Path::new(&path));
    })?;
    let recent = recent_companies(&db);
    crate::file_menu::refresh(&app, &recent, db.is_open().await);
    Ok(recent)
}

/// Find a Company File: searches this computer's usual places for company files: the home folder
/// (Documents, Desktop, Downloads, …), iCloud Drive and other synced folders, and on macOS any
/// connected drive under /Volumes. Bounded in depth, count and time.
#[tauri::command]
pub async fn find_company_files(app: AppHandle) -> Result<Vec<storage::FoundCompanyFile>, String> {
    let home = app.path().home_dir().map_err(|e| format!("FIND_NO_HOME: {e}"))?;
    let mut roots = vec![home.join("Library").join("Mobile Documents").join("com~apple~CloudDocs"), home.join("Library").join("CloudStorage"), home];
    if cfg!(target_os = "macos") {
        if let Ok(volumes) = std::fs::read_dir("/Volumes") {
            roots.extend(volumes.flatten().map(|v| v.path()).filter(|p| p.is_dir() && !p.is_symlink()));
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    tauri::async_runtime::spawn_blocking(move || storage::find_company_files(&roots, 8, 500, deadline))
        .await
        .map_err(|e| format!("FIND_FAILED: {e}"))
}

/// `path` as a company file name: `.localbooks` is added when no extension was typed.
fn with_company_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension(storage::COMPANY_FILE_EXTENSION)
    } else {
        path
    }
}

/// File › New Company: creates an empty company file named `company_name` at `path` and opens it,
/// with the chart of accounts and settings chosen in the setup assistant (or the standard chart).
#[tauri::command]
pub async fn create_company_file(app: AppHandle, path: String, company_name: String, setup: Option<CompanySetup>) -> Result<StorageStatus, String> {
    let path = with_company_extension(PathBuf::from(path));
    let status = app.state::<Db>().create(&path, &company_name, setup.as_ref()).await?;
    company_changed(&app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    notify(&app);
    Ok(status)
}

/// The New Company assistant's industries and tax forms.
#[tauri::command]
pub async fn get_company_setup_options() -> Result<SetupOptions, String> {
    Ok(company_setup::options())
}

/// The suggested chart of accounts for an industry and tax form.
#[tauri::command]
pub async fn get_starter_chart(industry: String, entity_type: String) -> Result<Vec<StarterAccount>, String> {
    company_setup::starter_chart(&industry, &entity_type)
}

/// Open or Restore Company with a LocalBooks backup: copies the backup to `destination_path` (a new
/// company file) and opens the copy (`windowless`: without restoring windows), leaving the backup
/// itself unchanged.
#[tauri::command]
pub async fn restore_backup_as(app: AppHandle, source_path: String, destination_path: String, windowless: Option<bool>) -> Result<StorageStatus, String> {
    let source = PathBuf::from(source_path);
    let destination = with_company_extension(storage::resolve_network_path(&PathBuf::from(destination_path)));
    backup::inspect(&source).await?;
    if destination.exists() {
        return Err(format!("NEW_COMPANY_EXISTS: {} already exists; choose another name or folder (LocalBooks never replaces a company file)", destination.display()));
    }
    std::fs::copy(&source, &destination).map_err(|e| format!("RESTORE_COPY: cannot copy the backup to {}: {e}", destination.display()))?;
    match mount_company_file(&app, &destination, windowless.unwrap_or(false)).await {
        Ok(status) => Ok(status),
        Err(err) => {
            let _ = std::fs::remove_file(&destination);
            Err(err)
        }
    }
}

/// File › Close Company: saves the desktop (Desktop View preferences), closes the other windows on
/// the company, then the company file itself (releasing its lock), and shows the No Company Open
/// screen.
pub async fn close_company(app: &AppHandle) -> Result<(), String> {
    let db = app.try_state::<Db>().ok_or_else(|| NO_COMPANY_OPEN.to_string())?;
    windows::save_on_close(app).await;
    windows::close_extra(app);
    db.close().await;
    company_changed(app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub async fn close_company_file(app: AppHandle) -> Result<(), String> {
    close_company(&app).await
}

/// Set when the company file could not be opened at launch. The window then shows this instead
/// of the app, and no command that needs the books is available.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupFailure {
    pub message: String,
    pub company_file: PathBuf,
}

/// Appends the failure to `startup-error.log` in the app data folder, for support.
pub fn record_startup_failure(data_dir: &Path, message: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir.join("startup-error.log")) {
        let _ = writeln!(log, "{} LocalBooks {}: {message}", Local::now().format("%Y-%m-%d %H:%M:%S %z"), env!("CARGO_PKG_VERSION"));
    }
}

#[tauri::command]
pub fn get_startup_failure(app: AppHandle) -> Option<StartupFailure> {
    app.try_state::<StartupFailure>().map(|s| s.inner().clone())
}
