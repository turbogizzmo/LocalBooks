//! Windows, as in QuickBooks Desktop for Windows: forms, registers and reports open as windows
//! *inside* the one LocalBooks window (the page's window manager, `src/components/WindowManager.jsx`),
//! with the Window menu (Close All, Cascade, Tile Vertically, Tile Horizontally, Arrange Icons and
//! the numbered list of open windows), View › Open Window List, View › One Window / Multiple
//! Windows, and the saved desktop that reopens with the company (see [`crate::desktop`]).
//!
//! Window › New Window still opens a second LocalBooks window on the same company, for a second
//! screen. Every LocalBooks window is the same app on the same backend state, so they all read and
//! write the one open company file; a write in any of them emits `ledger-changed` to all. The page a
//! new LocalBooks window starts on is carried in its URL fragment (`#view=…&action=…&data=…`).
//!
//! Each LocalBooks window reports the windows open inside it ([`report_children`]): that titles it,
//! fills the Window menu, and is what the desktop saves.

#![allow(clippy::float_arithmetic, reason = "window positions and sizes on screen, in points; no amounts are computed here")]

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu, WINDOW_SUBMENU_ID};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry};

use crate::db::Db;
use crate::desktop::{self, Bounds, Page, SaveMode, SavedWindow, Snapshot, WindowMode};

/// The window LocalBooks opens at launch.
pub const MAIN: &str = "main";
/// Labels of the extra LocalBooks windows; `capabilities/default.json` grants them the main one's access.
const EXTRA_PREFIX: &str = "window-";
/// Sent to the focused LocalBooks window when Window › New Window is chosen, so it opens the page it shows.
const NEW_WINDOW_REQUESTED: &str = "new-window-requested";
/// Sent to every LocalBooks window with the window preferences ([`WindowPreferences`]) when they change.
const WINDOW_PREFERENCES_CHANGED: &str = "window-preferences-changed";
/// Sent to a LocalBooks window with a Window menu command for the windows inside it: "close-all",
/// "close" (the active one), "cascade", "tile-vertically", "tile-horizontally", "arrange-icons",
/// or "focus:<id>".
const WINDOW_COMMAND: &str = "window-command";

const MENU_NEW_WINDOW: &str = "new-window";
const MENU_CLOSE_WINDOW: &str = "window-close";
/// A Window menu command for the page: this prefix, then the command.
const MENU_COMMAND_PREFIX: &str = "window-cmd:";
/// A window in the Window menu's list: this prefix, then its id.
const MENU_FOCUS_PREFIX: &str = "window-focus:";
const MENU_OPEN_WINDOW_LIST: &str = "view-open-window-list";
const MENU_ONE_WINDOW: &str = "view-one-window";
const MENU_MULTIPLE_WINDOWS: &str = "view-multiple-windows";
const MENU_WORKSPACE_AUTOSAVE: &str = "workspace-autosave";
const MENU_WORKSPACE_MANUAL: &str = "workspace-manual";
const MENU_WORKSPACE_SAVE: &str = "workspace-save";

/// The Window menu's arrangement commands, in QuickBooks' order.
const ARRANGEMENTS: [(&str, &str); 5] =
    [("close-all", "Close All"), ("cascade", "Cascade"), ("tile-vertically", "Tile Vertically"), ("tile-horizontally", "Tile Horizontally"), ("arrange-icons", "Arrange Icons")];

/// The smallest a LocalBooks window can be made.
const MIN_SIZE: (f64, f64) = (760.0, 520.0);
const DEFAULT_SIZE: (f64, f64) = (1280.0, 820.0);

/// A window inside a LocalBooks window, as the page reports it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildWindow {
    pub id: String,
    /// What it shows, for its title and the window lists: "Invoice 1001 · Acme".
    pub name: String,
    pub page: Page,
    /// Its place in the work area.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub maximized: bool,
}

/// What LocalBooks knows about each LocalBooks window, in the order they opened.
#[derive(Default)]
pub struct OpenWindows(Mutex<Registry>);

#[derive(Default)]
struct Registry {
    windows: Vec<(String, Entry)>,
    focused: Option<String>,
    /// The open company's name, for titles; `None` while the company is unnamed.
    company: Option<String>,
    company_open: bool,
    /// The company was just opened: the first LocalBooks window to ask gets its desktop back.
    restore_pending: bool,
    next_label: usize,
}

#[derive(Clone, Default)]
struct Entry {
    /// The windows inside it, in order of opening.
    children: Vec<ChildWindow>,
    active: Option<String>,
    /// Where it is on screen.
    frame: Option<Bounds>,
}

impl Entry {
    fn active_child(&self) -> Option<&ChildWindow> {
        self.active.as_ref().and_then(|id| self.children.iter().find(|c| &c.id == id))
    }
}

impl Registry {
    fn entry(&mut self, label: &str) -> &mut Entry {
        let at = match self.windows.iter().position(|(l, _)| l == label) {
            Some(i) => i,
            None => {
                // The main window heads the list: its windows are the desktop.
                let at = if label == MAIN { 0 } else { self.windows.len() };
                self.windows.insert(at, (label.to_string(), Entry::default()));
                at
            }
        };
        &mut self.windows[at].1
    }

    /// A LocalBooks window reports the windows inside it and which is active.
    fn report(&mut self, label: &str, children: Vec<ChildWindow>, active: Option<String>, frame: Option<Bounds>) {
        let entry = self.entry(label);
        entry.children = children;
        entry.active = active;
        if frame.is_some() {
            entry.frame = frame;
        }
    }

    /// A LocalBooks window closed. The last one stays listed, so quitting by closing it still
    /// saves its windows as the desktop.
    fn closed(&mut self, label: &str) {
        if self.windows.len() > 1 || self.windows.first().is_some_and(|(l, _)| l != label) {
            self.windows.retain(|(l, _)| l != label);
        }
        if self.focused.as_deref() == Some(label) {
            self.focused = None;
        }
    }

    /// Whether the window asking now should reopen the desktop: only the first to ask after the
    /// company opened (and none after it opened without restoring windows).
    fn take_restore(&mut self) -> bool {
        std::mem::take(&mut self.restore_pending)
    }

    /// The LocalBooks window the menus are about: the focused one, else the main one.
    fn menu_target(&self) -> Option<&(String, Entry)> {
        self.focused.as_ref().and_then(|f| self.windows.iter().find(|(l, _)| l == f)).or_else(|| self.windows.first())
    }

    /// What the desktop saves: the windows inside the main LocalBooks window (the active one last,
    /// so it reopens on top) and where that window was.
    fn snapshot(&self) -> Snapshot {
        let Some((_, entry)) = self.windows.first() else { return Snapshot::default() };
        let mut children = entry.children.clone();
        if let Some(i) = entry.active.as_ref().and_then(|id| children.iter().position(|c| &c.id == id)) {
            let active = children.remove(i);
            children.push(active);
        }
        Snapshot {
            windows: children.into_iter().map(|c| SavedWindow { page: c.page, bounds: c.bounds, maximized: c.maximized, minimized: c.minimized }).collect(),
            frame: entry.frame,
        }
    }
}

fn registry(app: &AppHandle) -> std::sync::MutexGuard<'_, Registry> {
    app.state::<OpenWindows>().inner().0.lock().unwrap_or_else(|e| e.into_inner())
}

/// The app page for a new LocalBooks window: `view` is a navigation id ("reports"), `action` and
/// `data` the request that view should open with (as from the app's own navigation).
fn start_url(view: Option<&str>, action: Option<&str>, data: Option<&serde_json::Value>) -> String {
    let mut pairs = Url::parse("localbooks://window/").expect("static url");
    {
        let mut query = pairs.query_pairs_mut();
        if let Some(view) = view {
            query.append_pair("view", view);
        }
        if let Some(action) = action {
            query.append_pair("action", action);
        }
        if let Some(data) = data.filter(|d| !d.is_null()) {
            query.append_pair("data", &data.to_string());
        }
    }
    match pairs.query().filter(|q| !q.is_empty()) {
        Some(fragment) => format!("index.html#{fragment}"),
        None => "index.html".to_string(),
    }
}

/// The title a LocalBooks window has while `content` is the window active inside it.
fn title(registry: &Registry, content: Option<&str>) -> String {
    if registry.company_open {
        desktop::window_title(content, registry.company.as_deref())
    } else {
        "No Company Open — LocalBooks".to_string()
    }
}

/// Window › New Window: another LocalBooks window on the open company at `page` (or the Home page).
pub fn open(app: &AppHandle, page: Option<&Page>) -> Result<String, String> {
    let (label, window_title) = {
        let mut reg = registry(app);
        let label = loop {
            reg.next_label += 1;
            let label = format!("{EXTRA_PREFIX}{}", reg.next_label);
            if app.get_webview_window(&label).is_none() {
                break label;
            }
        };
        (label, title(&reg, None))
    };
    let url = start_url(page.map(|p| p.view.as_str()), page.and_then(|p| p.action.as_deref()), page.and_then(|p| p.data.as_ref()));
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(window_title)
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .build()
        .map_err(|e| format!("WINDOW_OPEN: cannot open another window: {e}"))?;
    Ok(label)
}

/// The screens there are, in points.
fn screens(app: &AppHandle) -> Vec<Bounds> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let (pos, size) = (m.position().to_logical::<f64>(scale), m.size().to_logical::<f64>(scale));
            Bounds { x: pos.x, y: pos.y, width: size.width, height: size.height }
        })
        .collect()
}

/// Where `window` is on screen, in points; `None` while it is minimized or maximized (it then
/// keeps the place it had).
fn frame_of(window: &WebviewWindow) -> Option<Bounds> {
    if window.is_minimized().unwrap_or(false) || window.is_maximized().unwrap_or(false) {
        return None;
    }
    let scale = window.scale_factor().ok()?;
    let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some(Bounds { x: pos.x, y: pos.y, width: size.width, height: size.height })
}

/// The LocalBooks window a menu choice is for: the focused one, else the main one, else any.
pub fn target(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = app.webview_windows();
    windows
        .values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| windows.get(MAIN))
        .or_else(|| windows.values().next())
        .cloned()
}

/// Brings `window` forward.
pub fn focus_window(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Brings LocalBooks forward: the main window, or another one if the main window was closed.
pub fn focus(app: &AppHandle) {
    let windows = app.webview_windows();
    if let Some(window) = windows.get(MAIN).or_else(|| windows.values().next()) {
        focus_window(window);
    }
}

/// File › Close Company, or another company opening: closes every LocalBooks window but one (the
/// main window, or the focused one, else any, if the main window was closed).
pub fn close_extra(app: &AppHandle) {
    let windows = app.webview_windows();
    let keep = if windows.contains_key(MAIN) {
        Some(MAIN.to_string())
    } else {
        windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.values().next()).map(|w| w.label().to_string())
    };
    for (label, window) in &windows {
        if Some(label) != keep.as_ref() {
            let _ = window.destroy();
        }
    }
}

// ------------------------------------------------------------------------------------------------
// What each LocalBooks window shows
// ------------------------------------------------------------------------------------------------

/// A LocalBooks window reports the windows open inside it and the active one: it is retitled after
/// the active one, and the Window menu and what the desktop would save follow.
pub fn report_children(app: &AppHandle, window: &WebviewWindow, children: Vec<ChildWindow>, active: Option<String>) {
    // Window getters wait for the main thread, so they are read before the registry is locked.
    let frame = frame_of(window);
    let focused = window.is_focused().unwrap_or(false);
    let label = window.label().to_string();
    let window_title = {
        let mut reg = registry(app);
        reg.report(&label, children, active, frame);
        if focused {
            reg.focused = Some(label.clone());
        }
        let content = reg.entry(&label).active_child().map(|c| c.name.clone());
        title(&reg, content.as_deref())
    };
    let _ = window.set_title(&window_title);
    refresh_window_list(app);
}

/// Gives `label` back the title for what it shows (after printing, say).
pub fn retitle(app: &AppHandle, label: &str) {
    let window_title = {
        let mut reg = registry(app);
        let content = reg.entry(label).active_child().map(|c| c.name.clone());
        title(&reg, content.as_deref())
    };
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_title(&window_title);
    }
}

/// Another company file became the live one (or none did): every LocalBooks window is retitled.
pub fn set_company(app: &AppHandle, open: bool, name: Option<String>) {
    {
        let mut reg = registry(app);
        reg.company_open = open;
        reg.company = name;
    }
    for label in app.webview_windows().into_keys() {
        retitle(app, &label);
    }
}

/// LocalBooks window events: focus picks which window's list the Window menu shows, moves and
/// resizes are remembered for the desktop, and a closed window leaves the list (the last one stays,
/// so quitting by closing it still saves it).
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label().to_string();
    match event {
        WindowEvent::Focused(true) => {
            registry(app).focused = Some(label);
            refresh_window_list(app);
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let Some(frame) = app.get_webview_window(&label).and_then(|w| frame_of(&w)) else { return };
            let mut reg = registry(app);
            if let Some((_, e)) = reg.windows.iter_mut().find(|(l, _)| *l == label) {
                e.frame = Some(frame);
            }
        }
        WindowEvent::Destroyed => {
            registry(app).closed(&label);
            refresh_window_list(app);
        }
        _ => {}
    }
}

// ------------------------------------------------------------------------------------------------
// The saved desktop
// ------------------------------------------------------------------------------------------------

/// Where `desktops.json` is and the key the open company's desktop is under; `None` with no
/// company open, or when the file has users and nobody is signed in.
async fn desktop_key(app: &AppHandle) -> Option<(std::path::PathBuf, String)> {
    let db = app.try_state::<Db>()?;
    let (pool, session) = db.pool_and_session().await.ok()?;
    let path = db.path().await.ok()?;
    let user = session.lock().await.as_ref().map(|s| s.user.id);
    if user.is_none() && crate::users::users_enabled(&pool).await.unwrap_or(false) {
        return None;
    }
    Some((db.config_dir().to_path_buf(), desktop::key(&path, user)))
}

fn config_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.try_state::<Db>().map(|db| db.config_dir().to_path_buf())
}

/// The company is closing (File › Close Company, another company opening, quitting): under "Save
/// when closing company" the windows open inside LocalBooks become its desktop.
pub async fn save_on_close(app: &AppHandle) {
    let Some((dir, key)) = desktop_key(app).await else { return };
    let now = registry(app).snapshot();
    if now.windows.is_empty() && now.frame.is_none() {
        return;
    }
    if let Err(err) = desktop::update(&dir, |file| {
        desktop::company_closing(file, &key, now);
    }) {
        eprintln!("cannot save the desktop: {err}");
    }
}

/// The next LocalBooks window to load the open company reopens its saved desktop (`true`), or none
/// does (the company was opened without restoring windows).
pub fn restore_on_next_load(app: &AppHandle, restore: bool) {
    registry(app).restore_pending = restore;
}

/// The desktop a LocalBooks window starts with.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    /// The windows to open inside it, in order (the last is active); empty for an empty desktop.
    pub windows: Vec<SavedWindow>,
    /// "Don't save the desktop": the company opens on the Home page.
    pub home_only: bool,
}

/// The first LocalBooks window to load after the company opened asks for its desktop: it is moved
/// back where it was and given the windows to reopen inside it. `known_views` are the pages the
/// app has; a saved window on any other is skipped. `None` when there is nothing to restore (the
/// window then opens as it would anyway).
pub async fn take_restore(app: &AppHandle, caller: &WebviewWindow, known_views: &[String]) -> Option<Restored> {
    if !registry(app).take_restore() {
        return None;
    }
    let (dir, key) = desktop_key(app).await?;
    let file = desktop::read(&dir);
    let saved = file.companies.get(&key)?;
    if saved.mode == SaveMode::DontSave {
        return Some(Restored { windows: Vec::new(), home_only: true });
    }
    if let Some(frame) = saved.frame.and_then(|b| desktop::fit(b, &screens(app))) {
        let _ = caller.set_position(LogicalPosition::new(frame.x, frame.y));
        let _ = caller.set_size(LogicalSize::new(frame.width.max(MIN_SIZE.0), frame.height.max(MIN_SIZE.1)));
    }
    let windows = desktop::to_restore(&file, &key, |view| known_views.iter().any(|k| k == view));
    (!windows.is_empty()).then_some(Restored { windows, home_only: false })
}

/// Desktop View as the Preferences screen and the menus show it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopView {
    pub mode: SaveMode,
    /// How many windows the saved desktop has.
    pub saved_windows: usize,
    pub window_mode: WindowMode,
    pub show_open_window_list: bool,
    /// Whether the desktop can be saved now (not while the file has users and nobody is signed in).
    pub available: bool,
}

pub async fn desktop_view(app: &AppHandle) -> DesktopView {
    let file = config_dir(app).as_deref().map(desktop::read).unwrap_or_default();
    let key = desktop_key(app).await;
    let key = key.as_ref().map(|(_, k)| k.as_str());
    let company = key.and_then(|k| file.companies.get(k));
    DesktopView {
        mode: company.map(|c| c.mode).unwrap_or_default(),
        saved_windows: company.map_or(0, |c| c.windows.len()),
        window_mode: desktop::window_mode(&file, key),
        show_open_window_list: file.show_open_window_list,
        available: key.is_some(),
    }
}

/// Chooses Desktop View › Desktop: "save-current" saves the windows open now as the desktop that
/// reopens every time (Window › Workspace › Save Workspace on the Mac does the same).
pub async fn set_desktop_mode(app: &AppHandle, choice: &str) -> Result<DesktopView, String> {
    if !["save-current", "save-when-closing", "dont-save", "keep-saved"].contains(&choice) {
        return Err(format!("DESKTOP_MODE: unknown desktop choice {choice}"));
    }
    let (dir, key) = desktop_key(app).await.ok_or_else(|| "DESKTOP_UNAVAILABLE: sign in to save your desktop".to_string())?;
    let now = registry(app).snapshot();
    desktop::update(&dir, |file| match choice {
        "save-current" => desktop::save_current(file, &key, now),
        "save-when-closing" => desktop::set_mode(file, &key, SaveMode::SaveWhenClosing),
        "dont-save" => desktop::set_mode(file, &key, SaveMode::DontSave),
        // "Keep previously saved desktop" changes nothing.
        _ => {}
    })?;
    let view = desktop_view(app).await;
    refresh_preference_menus(app, &view);
    Ok(view)
}

/// View › One Window / Multiple Windows (kept with the open company) and View › Open Window List.
pub async fn set_window_preferences(app: &AppHandle, window_mode: Option<WindowMode>, show_open_window_list: Option<bool>) -> Result<DesktopView, String> {
    let dir = config_dir(app).ok_or_else(|| "DESKTOP_UNAVAILABLE: LocalBooks has not started".to_string())?;
    let key = desktop_key(app).await.map(|(_, k)| k);
    desktop::update(&dir, |file| {
        if let Some(mode) = window_mode {
            desktop::set_window_mode(file, key.as_deref(), mode);
        }
        if let Some(show) = show_open_window_list {
            file.show_open_window_list = show;
        }
    })?;
    let view = desktop_view(app).await;
    let _ = app.emit(WINDOW_PREFERENCES_CHANGED, WindowPreferences { window_mode: view.window_mode, show_open_window_list: view.show_open_window_list });
    refresh_preference_menus(app, &view);
    Ok(view)
}

/// View › One Window / Multiple Windows and View › Open Window List, as the page reads them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPreferences {
    pub window_mode: WindowMode,
    pub show_open_window_list: bool,
}

// ------------------------------------------------------------------------------------------------
// The Window and View menus
// ------------------------------------------------------------------------------------------------

/// The menu items whose state follows the windows and the preferences.
struct WindowMenus {
    window_menu: Submenu<Wry>,
    /// The Window menu's numbered list of the windows inside LocalBooks.
    list: Mutex<Vec<MenuItemKind<Wry>>>,
    open_window_list: CheckMenuItem<Wry>,
    one_window: CheckMenuItem<Wry>,
    multiple_windows: CheckMenuItem<Wry>,
    workspace: Option<(CheckMenuItem<Wry>, CheckMenuItem<Wry>)>,
}

/// Window: New Window, Close All, Cascade, Tile Vertically, Tile Horizontally, Arrange Icons, and
/// (on macOS) Workspace › AutoSave / Manual / Save Workspace as QuickBooks for Mac has; then the
/// numbered list of the windows inside LocalBooks. View: Open Window List, One Window, Multiple
/// Windows. On macOS, ⌘W (File › Close Window) closes the window active inside LocalBooks, as it
/// closes a form in QuickBooks for Mac, rather than LocalBooks itself.
///
/// New Window's shortcut is the menu's (Cmd+Shift+N) on macOS; on Windows and Linux the page
/// handles Ctrl+Shift+N, so the item carries none. Runs after `app_menu::install`, which makes the
/// View menu where the platform has none.
pub fn add_menus(app: &AppHandle, menu: &Menu<Wry>) -> tauri::Result<()> {
    let Some(window_menu) = menu.get(WINDOW_SUBMENU_ID).and_then(|item| item.as_submenu().cloned()) else { return Ok(()) };
    if cfg!(target_os = "macos") {
        replace_close_window(app, menu, &window_menu)?;
    }
    let accel = cfg!(target_os = "macos").then_some("CmdOrCtrl+Shift+N");
    let mut top: Vec<MenuItemKind<Wry>> = vec![
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_NEW_WINDOW, "New Window", true, accel)?),
        MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
    ];
    for (command, label) in ARRANGEMENTS {
        top.push(MenuItemKind::MenuItem(MenuItem::with_id(app, format!("{MENU_COMMAND_PREFIX}{command}"), label, true, None::<&str>)?));
    }
    top.push(MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?));
    let refs: Vec<&dyn IsMenuItem<Wry>> = top.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    window_menu.insert_items(&refs, 0)?;
    let workspace = if cfg!(target_os = "macos") {
        let autosave = CheckMenuItem::with_id(app, MENU_WORKSPACE_AUTOSAVE, "AutoSave", true, true, None::<&str>)?;
        let manual = CheckMenuItem::with_id(app, MENU_WORKSPACE_MANUAL, "Manual", true, false, None::<&str>)?;
        let save = MenuItem::with_id(app, MENU_WORKSPACE_SAVE, "Save Workspace", true, None::<&str>)?;
        let sub = Submenu::with_items(app, "Workspace", true, &[&autosave, &manual, &PredefinedMenuItem::separator(app)?, &save])?;
        window_menu.insert_items(&[&sub, &PredefinedMenuItem::separator(app)?], top.len())?;
        Some((autosave, manual))
    } else {
        None
    };

    let open_window_list = CheckMenuItem::with_id(app, MENU_OPEN_WINDOW_LIST, "Open Window List", true, false, None::<&str>)?;
    let one_window = CheckMenuItem::with_id(app, MENU_ONE_WINDOW, "One Window", true, false, None::<&str>)?;
    let multiple_windows = CheckMenuItem::with_id(app, MENU_MULTIPLE_WINDOWS, "Multiple Windows", true, true, None::<&str>)?;
    let view_menu = menu.items()?.into_iter().filter_map(|i| i.as_submenu().cloned()).find(|s| s.text().is_ok_and(|t| t.replace('&', "") == "View"));
    if let Some(view_menu) = view_menu {
        // After Show or Hide Left Icon Bar (and before macOS's Enter Full Screen), as in QuickBooks.
        let len = view_menu.items()?.len();
        let at = view_menu.items()?.iter().position(|i| matches!(i, MenuItemKind::Predefined(_))).unwrap_or(len);
        let items: [&dyn IsMenuItem<Wry>; 6] =
            [&PredefinedMenuItem::separator(app)?, &open_window_list, &PredefinedMenuItem::separator(app)?, &one_window, &multiple_windows, &PredefinedMenuItem::separator(app)?];
        let take = if at == len { 5 } else { 6 };
        view_menu.insert_items(&items[..take], at)?;
    }
    app.manage(WindowMenus { window_menu, list: Mutex::new(Vec::new()), open_window_list, one_window, multiple_windows, workspace });
    Ok(())
}

/// macOS: the system's Close Window (⌘W) would close LocalBooks itself; ours closes the window
/// active inside it (and LocalBooks only when none is open).
fn replace_close_window(app: &AppHandle, menu: &Menu<Wry>, window_menu: &Submenu<Wry>) -> tauri::Result<()> {
    let is_close = |item: &MenuItemKind<Wry>| matches!(item, MenuItemKind::Predefined(p) if p.text().is_ok_and(|t| t == "Close Window"));
    let mut file_at = None;
    for sub in menu.items()?.into_iter().filter_map(|i| i.as_submenu().cloned()) {
        let is_file = sub.text().is_ok_and(|t| t == "File");
        for (i, item) in sub.items()?.into_iter().enumerate() {
            if is_close(&item) {
                sub.remove(&item)?;
                if is_file {
                    file_at = Some((sub.clone(), i));
                }
            }
        }
    }
    // The separator the system's item leaves at the end of the Window menu goes too.
    if let Some(last) = window_menu.items()?.last().filter(|i| matches!(i, MenuItemKind::Predefined(p) if p.text().map_or(true, |t| t.is_empty()))) {
        window_menu.remove(last)?;
    }
    let close = MenuItem::with_id(app, MENU_CLOSE_WINDOW, "Close Window", true, Some("CmdOrCtrl+W"))?;
    match file_at {
        Some((file, at)) => file.insert(&close, at)?,
        None => window_menu.append(&close)?,
    }
    Ok(())
}

/// The Window menu's numbered list of the windows inside the LocalBooks window the menus are
/// about, the active one ticked.
fn refresh_window_list(app: &AppHandle) {
    let Some(menus) = app.try_state::<WindowMenus>() else { return };
    let (children, active) = {
        let reg = registry(app);
        reg.menu_target().map(|(_, e)| (e.children.clone(), e.active.clone())).unwrap_or_default()
    };
    let mut list = menus.list.lock().unwrap_or_else(|e| e.into_inner());
    for item in list.drain(..) {
        let _ = menus.window_menu.remove(&item);
    }
    for (n, child) in children.iter().enumerate() {
        let name = child.name.replace('&', "&&");
        // The first nine are numbered, as QuickBooks numbers its open windows.
        let label = if n < 9 { format!("&{} {name}", n + 1) } else { name };
        let checked = active.as_deref() == Some(child.id.as_str());
        if let Ok(item) = CheckMenuItem::with_id(app, format!("{MENU_FOCUS_PREFIX}{}", child.id), label, true, checked, None::<&str>) {
            if menus.window_menu.append(&item).is_ok() {
                list.push(MenuItemKind::Check(item));
            }
        }
    }
}

/// Ticks the View and Workspace items that match the preferences.
fn refresh_preference_menus(app: &AppHandle, view: &DesktopView) {
    let Some(menus) = app.try_state::<WindowMenus>() else { return };
    let _ = menus.open_window_list.set_checked(view.show_open_window_list);
    let _ = menus.one_window.set_checked(view.window_mode == WindowMode::One);
    let _ = menus.multiple_windows.set_checked(view.window_mode == WindowMode::Multiple);
    if let Some((autosave, manual)) = &menus.workspace {
        let _ = autosave.set_checked(view.mode == SaveMode::SaveWhenClosing);
        let _ = manual.set_checked(view.mode == SaveMode::KeepSaved);
    }
}

/// Brings the menus up to date with the saved preferences (at launch, and when a company opens).
pub async fn refresh_menus(app: &AppHandle) {
    let view = desktop_view(app).await;
    refresh_preference_menus(app, &view);
    refresh_window_list(app);
}

/// Sends a Window menu command to the LocalBooks window it is for.
fn send_command(app: &AppHandle, command: &str) {
    if let Some(window) = target(app) {
        let _ = window.emit_to(window.label(), WINDOW_COMMAND, command);
        focus_window(&window);
    }
}

/// Window and View menu choices; other menu ids are ignored.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        MENU_NEW_WINDOW => new_window(app),
        MENU_CLOSE_WINDOW => send_command(app, "close"),
        MENU_OPEN_WINDOW_LIST | MENU_ONE_WINDOW | MENU_MULTIPLE_WINDOWS => {
            let (app, id) = (app.clone(), id.to_string());
            tauri::async_runtime::spawn(async move {
                let current = desktop_view(&app).await;
                let result = match id.as_str() {
                    MENU_OPEN_WINDOW_LIST => set_window_preferences(&app, None, Some(!current.show_open_window_list)).await,
                    MENU_ONE_WINDOW => set_window_preferences(&app, Some(WindowMode::One), None).await,
                    _ => set_window_preferences(&app, Some(WindowMode::Multiple), None).await,
                };
                if let Err(err) = result {
                    eprintln!("{err}");
                }
            });
        }
        MENU_WORKSPACE_AUTOSAVE | MENU_WORKSPACE_MANUAL | MENU_WORKSPACE_SAVE => {
            let (app, id) = (app.clone(), id.to_string());
            tauri::async_runtime::spawn(async move {
                let choice = match id.as_str() {
                    MENU_WORKSPACE_AUTOSAVE => "save-when-closing",
                    MENU_WORKSPACE_MANUAL => "keep-saved",
                    _ => "save-current",
                };
                if let Err(err) = set_desktop_mode(&app, choice).await {
                    eprintln!("{err}");
                }
                // A check item ticks itself when chosen; set the ticks from what was saved.
                refresh_preference_menus(&app, &desktop_view(&app).await);
            });
        }
        _ => {
            if let Some(command) = id.strip_prefix(MENU_COMMAND_PREFIX) {
                send_command(app, command);
            } else if let Some(child) = id.strip_prefix(MENU_FOCUS_PREFIX) {
                send_command(app, &format!("focus:{child}"));
                // The item ticks itself when chosen; the page's next report sets the ticks right.
                refresh_window_list(app);
            }
        }
    }
}

/// Window › New Window: the focused LocalBooks window opens a copy of the page active in it; with
/// none focused, a new one opens on the Home page.
fn new_window(app: &AppHandle) {
    let focused = app.webview_windows().into_values().find(|w| w.is_focused().unwrap_or(false));
    match focused {
        Some(window) => {
            let _ = window.emit_to(window.label(), NEW_WINDOW_REQUESTED, ());
        }
        None => {
            if let Err(err) = open(app, None) {
                eprintln!("{err}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
