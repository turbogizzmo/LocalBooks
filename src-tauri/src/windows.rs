//! More than one window on the open company, as in QuickBooks Desktop: Window › New Window
//! (Cmd/Ctrl+Shift+N), forms, registers and reports opened in windows of their own, the Window menu
//! (Close All, Cascade, Tile, and on Windows and Linux the list of open windows; macOS lists them
//! itself), View › Open Window List, View › One Window / Multiple Windows, and the saved desktop
//! that reopens with the company (see [`crate::desktop`]).
//!
//! Every window is the same app on the same backend state, so they all read and write the one open
//! company file. A write in any window emits `ledger-changed` to all of them, so each refreshes what
//! it shows. The page a new window starts on is carried in its URL fragment
//! (`#view=…&action=…&data=…`), which the app reads at startup; each window then reports the page
//! it shows ([`report_page`]), which titles it, lists it, and is what the desktop saves.

#![allow(clippy::float_arithmetic, reason = "window positions and sizes on screen, in points; no amounts are computed here")]

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu, WINDOW_SUBMENU_ID};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry};

use crate::db::Db;
use crate::desktop::{self, Arrangement, Bounds, Page, SaveMode, SavedWindow, WindowMode};

/// The window LocalBooks opens at launch.
pub const MAIN: &str = "main";
/// Labels of the extra windows; `capabilities/default.json` grants them the main window's access.
const EXTRA_PREFIX: &str = "window-";
/// Sent to the focused window when Window › New Window is chosen, so it opens the page it shows.
const NEW_WINDOW_REQUESTED: &str = "new-window-requested";
/// Sent to every window with the open windows ([`OpenWindowView`]) whenever they change.
const OPEN_WINDOWS_CHANGED: &str = "open-windows-changed";
/// Sent to every window with the window preferences ([`WindowPreferences`]) when they change.
const WINDOW_PREFERENCES_CHANGED: &str = "window-preferences-changed";
/// Sent to a window to show a page, as the menu bar does (see `app_menu.rs`).
const MENU_NAVIGATE: &str = "menu-navigate";

const MENU_NEW_WINDOW: &str = "new-window";
const MENU_CLOSE_ALL: &str = "window-close-all";
const MENU_CASCADE: &str = "window-cascade";
const MENU_TILE_VERTICALLY: &str = "window-tile-vertically";
const MENU_TILE_HORIZONTALLY: &str = "window-tile-horizontally";
/// A window in the Window menu's list: this prefix, then its label.
const MENU_FOCUS_PREFIX: &str = "window-focus:";
const MENU_OPEN_WINDOW_LIST: &str = "view-open-window-list";
const MENU_ONE_WINDOW: &str = "view-one-window";
const MENU_MULTIPLE_WINDOWS: &str = "view-multiple-windows";
const MENU_WORKSPACE_AUTOSAVE: &str = "workspace-autosave";
const MENU_WORKSPACE_MANUAL: &str = "workspace-manual";
const MENU_WORKSPACE_SAVE: &str = "workspace-save";

/// The smallest a window can be made, so two fit side by side on a laptop screen.
const MIN_SIZE: (f64, f64) = (760.0, 520.0);
const DEFAULT_SIZE: (f64, f64) = (1280.0, 820.0);

/// What LocalBooks knows about each open window, in the order they opened.
#[derive(Default)]
pub struct OpenWindows(Mutex<Registry>);

#[derive(Default)]
struct Registry {
    windows: Vec<(String, Entry)>,
    focused: Option<String>,
    /// The open company's name, for titles; `None` while the company is unnamed.
    company: Option<String>,
    company_open: bool,
    /// The company was just opened: the first window to ask gets its desktop back.
    restore_pending: bool,
    next_label: usize,
}

#[derive(Clone, Default)]
struct Entry {
    page: Option<Page>,
    /// What the window shows, for its title and the window lists: "Invoice 1001 · Acme".
    content: Option<String>,
    bounds: Option<Bounds>,
    maximized: bool,
}

/// An open window as the Open Window List shows it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWindowView {
    pub label: String,
    pub name: String,
    pub active: bool,
}

/// View › One Window / Multiple Windows and View › Open Window List, as the page reads them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPreferences {
    pub window_mode: WindowMode,
    pub show_open_window_list: bool,
}

/// The menu items whose state follows the windows and the preferences.
struct WindowMenus {
    window_menu: Submenu<Wry>,
    /// The Window menu's list of open windows (not on macOS, which lists them itself).
    list: Mutex<Vec<MenuItemKind<Wry>>>,
    open_window_list: CheckMenuItem<Wry>,
    one_window: CheckMenuItem<Wry>,
    multiple_windows: CheckMenuItem<Wry>,
    workspace: Option<(CheckMenuItem<Wry>, CheckMenuItem<Wry>)>,
}

impl Registry {
    /// A window reports what it shows; a window new to the list joins it (the main window first,
    /// so a desktop reopens in it).
    fn report(&mut self, label: &str, page: Page, content: Option<String>, live: Option<(Bounds, bool)>, focused: bool) {
        let entry = match self.windows.iter().position(|(l, _)| l == label) {
            Some(i) => &mut self.windows[i].1,
            None => {
                let at = if label == MAIN { 0 } else { self.windows.len() };
                self.windows.insert(at, (label.to_string(), Entry::default()));
                &mut self.windows[at].1
            }
        };
        entry.page = Some(page);
        entry.content = content;
        if let Some((bounds, maximized)) = live {
            entry.bounds = Some(bounds);
            entry.maximized = maximized;
        }
        if focused {
            self.focused = Some(label.to_string());
        }
    }

    /// A window closed. The last one stays listed, so quitting by closing it still saves it as
    /// the desktop.
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
}

fn registry(app: &AppHandle) -> std::sync::MutexGuard<'_, Registry> {
    app.state::<OpenWindows>().inner().0.lock().unwrap_or_else(|e| e.into_inner())
}

/// The app page for a new window: `view` is a navigation id ("reports"), `action` and `data` the
/// request that view should open with (as from the app's own navigation).
fn start_url(view: Option<&str>, action: Option<&str>, data: Option<&serde_json::Value>) -> String {
    match fragment(view, action, data) {
        Some(fragment) => format!("index.html#{fragment}"),
        None => "index.html".to_string(),
    }
}

fn fragment(view: Option<&str>, action: Option<&str>, data: Option<&serde_json::Value>) -> Option<String> {
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
    pairs.query().filter(|q| !q.is_empty()).map(str::to_string)
}

/// The title a window has while it shows `content`.
fn title(registry: &Registry, content: Option<&str>) -> String {
    if registry.company_open {
        desktop::window_title(content, registry.company.as_deref())
    } else {
        "No Company Open — LocalBooks".to_string()
    }
}

/// Opens another window on the open company at `page` (or on the Dashboard), titled like the
/// others, where `saved` says when it is on a screen.
pub fn open(app: &AppHandle, page: Option<&Page>, saved: Option<&SavedWindow>) -> Result<String, String> {
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
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(window_title)
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1);
    if let Some(saved) = saved {
        if let Some(bounds) = saved.bounds.and_then(|b| desktop::fit(b, &screens(app))) {
            builder = builder.position(bounds.x, bounds.y).inner_size(bounds.width, bounds.height);
        }
        builder = builder.maximized(saved.maximized);
    }
    builder.build().map_err(|e| format!("WINDOW_OPEN: cannot open another window: {e}"))?;
    Ok(label)
}

/// The window showing `page` already, if any: a list opening a report that is open brings its
/// window forward instead of opening another.
pub fn showing(app: &AppHandle, page: &Page) -> Option<WebviewWindow> {
    let label = registry(app).windows.iter().find(|(_, e)| e.page.as_ref() == Some(page)).map(|(l, _)| l.clone())?;
    app.get_webview_window(&label)
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

/// Where `window` is, in points, and whether it is maximized.
fn bounds_of(window: &WebviewWindow) -> Option<(Bounds, bool)> {
    let scale = window.scale_factor().ok()?;
    let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some((Bounds { x: pos.x, y: pos.y, width: size.width, height: size.height }, window.is_maximized().unwrap_or(false)))
}

/// The window a menu choice is for: the focused one, else the main one, else any.
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

/// The window kept when the others close: the main window, or the focused one (else any) if the
/// main window was closed.
fn keeper(app: &AppHandle) -> Option<String> {
    let windows = app.webview_windows();
    if windows.contains_key(MAIN) {
        Some(MAIN.to_string())
    } else {
        windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.values().next()).map(|w| w.label().to_string())
    }
}

/// File › Close Company, or another company opening: closes every window but one (see [`keeper`]).
pub fn close_extra(app: &AppHandle) {
    let keep = keeper(app);
    for (label, window) in app.webview_windows() {
        if Some(&label) != keep.as_ref() {
            let _ = window.destroy();
        }
    }
}

/// Window › Close All: asks every window but one to close (each can stop to ask about unsaved
/// changes), and sends the one left to the Home page.
fn close_all(app: &AppHandle) {
    let keep = keeper(app);
    for (label, window) in app.webview_windows() {
        if Some(&label) == keep.as_ref() {
            let _ = window.emit_to(&label, MENU_NAVIGATE, serde_json::json!({ "view": "dashboard", "action": null, "data": null }));
            focus_window(&window);
        } else {
            let _ = window.close();
        }
    }
}

/// Window › Cascade / Tile Vertically / Tile Horizontally: lays out the open windows over the
/// screen the focused one is on.
fn arrange(app: &AppHandle, arrangement: Arrangement) {
    let Some(anchor) = target(app) else { return };
    let Some(monitor) = anchor.current_monitor().ok().flatten() else { return };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let (pos, size) = (work.position.to_logical::<f64>(scale), work.size.to_logical::<f64>(scale));
    let area = Bounds { x: pos.x, y: pos.y, width: size.width, height: size.height };
    let order: Vec<String> = registry(app).windows.iter().map(|(l, _)| l.clone()).collect();
    let windows: Vec<WebviewWindow> =
        order.iter().filter_map(|l| app.get_webview_window(l)).filter(|w| !w.is_minimized().unwrap_or(false) && w.is_visible().unwrap_or(true)).collect();
    for (window, b) in windows.iter().zip(desktop::arrange(arrangement, windows.len(), area)) {
        let _ = window.unmaximize();
        // The layout is for whole windows; the title bar and frame come out of the size.
        let frame = window.outer_size().ok().zip(window.inner_size().ok()).map(|(o, i)| {
            let (o, i) = (o.to_logical::<f64>(scale), i.to_logical::<f64>(scale));
            ((o.width - i.width).max(0.0), (o.height - i.height).max(0.0))
        });
        let (fw, fh) = frame.unwrap_or((0.0, 0.0));
        let _ = window.set_position(LogicalPosition::new(b.x, b.y));
        let _ = window.set_size(LogicalSize::new((b.width - fw).max(MIN_SIZE.0), (b.height - fh).max(MIN_SIZE.1)));
    }
    if let Some(window) = windows.last() {
        focus_window(window);
    }
}

// ------------------------------------------------------------------------------------------------
// What each window shows
// ------------------------------------------------------------------------------------------------

/// A window reports the page it shows and what is in it: retitles it, and updates the window lists
/// and what the desktop would save.
pub fn report_page(app: &AppHandle, window: &WebviewWindow, page: Page, content: Option<String>) {
    // Window getters wait for the main thread, so they are read before the registry is locked.
    let live = bounds_of(window);
    let focused = window.is_focused().unwrap_or(false);
    let label = window.label().to_string();
    let window_title = {
        let mut reg = registry(app);
        let window_title = title(&reg, content.as_deref());
        reg.report(&label, page, content, live, focused);
        window_title
    };
    let _ = window.set_title(&window_title);
    windows_changed(app);
}

/// Gives `label` back the title for what it shows (after printing, say).
pub fn retitle(app: &AppHandle, label: &str) {
    let window_title = {
        let reg = registry(app);
        let content = reg.windows.iter().find(|(l, _)| l == label).and_then(|(_, e)| e.content.clone());
        title(&reg, content.as_deref())
    };
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_title(&window_title);
    }
}

/// Another company file became the live one (or none did): every window is retitled for it.
pub fn set_company(app: &AppHandle, open: bool, name: Option<String>) {
    let titles: Vec<(String, String)> = {
        let mut reg = registry(app);
        reg.company_open = open;
        reg.company = name;
        let labels: Vec<String> = app.webview_windows().into_keys().collect();
        labels
            .into_iter()
            .map(|label| {
                let content = reg.windows.iter().find(|(l, _)| *l == label).and_then(|(_, e)| e.content.clone());
                let t = title(&reg, content.as_deref());
                (label, t)
            })
            .collect()
    };
    for (label, t) in titles {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.set_title(&t);
        }
    }
}

/// The open windows, in the order they opened, with the active (focused) one marked.
pub fn list(app: &AppHandle) -> Vec<OpenWindowView> {
    let reg = registry(app);
    list_of(&reg)
}

fn list_of(reg: &Registry) -> Vec<OpenWindowView> {
    reg.windows
        .iter()
        .map(|(label, e)| OpenWindowView {
            label: label.clone(),
            name: e.content.clone().filter(|c| !c.trim().is_empty()).unwrap_or_else(|| "LocalBooks".to_string()),
            active: reg.focused.as_deref() == Some(label),
        })
        .collect()
}

/// Brings the window `label` forward (the Open Window List, the Window menu).
pub fn bring_forward(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app.get_webview_window(label).ok_or_else(|| "WINDOW_GONE: that window has been closed".to_string())?;
    focus_window(&window);
    Ok(())
}

/// Window events: focus marks the active window, moves and resizes are remembered for the desktop,
/// and a closed window leaves the lists. The last window to close stays on them, so quitting by
/// closing it still saves it as the desktop.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label().to_string();
    match event {
        WindowEvent::Focused(true) => {
            registry(app).focused = Some(label);
            windows_changed(app);
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let Some(webview) = app.get_webview_window(&label) else { return };
            let Some((bounds, maximized)) = bounds_of(&webview) else { return };
            if let Some((_, e)) = registry(app).windows.iter_mut().find(|(l, _)| *l == label) {
                // A minimized window keeps the place it had.
                if !webview.is_minimized().unwrap_or(false) {
                    e.bounds = Some(bounds);
                    e.maximized = maximized;
                }
            }
        }
        WindowEvent::Destroyed => {
            registry(app).closed(&label);
            windows_changed(app);
        }
        _ => {}
    }
}

/// After any change to the open windows: every page and the Window menu are told.
fn windows_changed(app: &AppHandle) {
    let views = list(app);
    let _ = app.emit(OPEN_WINDOWS_CHANGED, &views);
    refresh_window_list(app, &views);
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

/// The windows open now, in order, as a desktop keeps them.
fn snapshot(app: &AppHandle) -> Vec<SavedWindow> {
    let entries: Vec<(String, Entry)> = registry(app).windows.clone();
    entries
        .into_iter()
        .filter_map(|(label, e)| {
            let page = e.page?;
            let live = app.get_webview_window(&label).filter(|w| !w.is_minimized().unwrap_or(false)).and_then(|w| bounds_of(&w));
            let (bounds, maximized) = match live {
                Some((b, m)) => (Some(b), m),
                None => (e.bounds, e.maximized),
            };
            Some(SavedWindow { page, bounds, maximized })
        })
        .collect()
}

/// The company is closing (File › Close Company, another company opening, quitting): under "Save
/// when closing company" its open windows become its desktop.
pub async fn save_on_close(app: &AppHandle) {
    let Some((dir, key)) = desktop_key(app).await else { return };
    let windows = snapshot(app);
    if windows.is_empty() {
        return;
    }
    if let Err(err) = desktop::update(&dir, |file| {
        desktop::company_closing(file, &key, windows);
    }) {
        eprintln!("cannot save the desktop: {err}");
    }
}

/// The next window to load the open company reopens its saved desktop (`true`), or none does (the
/// company was opened without restoring windows).
pub fn restore_on_next_load(app: &AppHandle, restore: bool) {
    registry(app).restore_pending = restore;
}

/// The first window to load after the company opened asks for its desktop: it is given the first
/// saved window's page (and place), and the rest open beside it. `known_views` are the pages the
/// app has; a saved window on any other is skipped. `None` when there is nothing to restore (the
/// window then shows the page it would anyway).
pub async fn take_restore(app: &AppHandle, caller: &WebviewWindow, known_views: &[String]) -> Option<Page> {
    if !registry(app).take_restore() {
        return None;
    }
    let (dir, key) = desktop_key(app).await?;
    let file = desktop::read(&dir);
    let dont_save = file.companies.get(&key).is_some_and(|d| d.mode == SaveMode::DontSave);
    let mut saved = desktop::to_restore(&file, &key, |view| known_views.iter().any(|k| k == view)).into_iter();
    let Some(first) = saved.next() else {
        // "Don't save the desktop": the company opens on the Home page.
        return dont_save.then(|| Page { view: "dashboard".into(), action: None, data: None });
    };
    if let Some(bounds) = first.bounds.and_then(|b| desktop::fit(b, &screens(app))) {
        let _ = caller.unmaximize();
        let _ = caller.set_position(LogicalPosition::new(bounds.x, bounds.y));
        let _ = caller.set_size(LogicalSize::new(bounds.width, bounds.height));
    }
    if first.maximized {
        let _ = caller.maximize();
    }
    // One window that fails to open never stops the others.
    for window in saved {
        if let Err(err) = open(app, Some(&window.page), Some(&window)) {
            eprintln!("cannot reopen a window of the saved desktop: {err}");
        }
    }
    Some(first.page)
}

/// Desktop View › Desktop as the Preferences screen shows it.
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
    let config_dir = app.try_state::<Db>().map(|db| db.config_dir().to_path_buf());
    let file = config_dir.as_deref().map(desktop::read).unwrap_or_default();
    let key = desktop_key(app).await;
    let company = key.as_ref().and_then(|(_, k)| file.companies.get(k));
    DesktopView {
        mode: company.map(|c| c.mode).unwrap_or_default(),
        saved_windows: company.map_or(0, |c| c.windows.len()),
        window_mode: file.window_mode,
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
    let windows = snapshot(app);
    desktop::update(&dir, |file| match choice {
        "save-current" => desktop::save_current(file, &key, windows),
        "save-when-closing" => desktop::set_mode(file, &key, SaveMode::SaveWhenClosing),
        "dont-save" => desktop::set_mode(file, &key, SaveMode::DontSave),
        // "Keep previously saved desktop" changes nothing.
        _ => {}
    })?;
    let view = desktop_view(app).await;
    refresh_preference_menus(app, &view);
    Ok(view)
}

/// View › One Window / Multiple Windows and View › Open Window List, for this computer.
pub async fn set_window_preferences(app: &AppHandle, window_mode: Option<WindowMode>, show_open_window_list: Option<bool>) -> Result<DesktopView, String> {
    let dir = app.try_state::<Db>().map(|db| db.config_dir().to_path_buf()).ok_or_else(|| "DESKTOP_UNAVAILABLE: LocalBooks has not started".to_string())?;
    desktop::update(&dir, |file| {
        if let Some(mode) = window_mode {
            file.window_mode = mode;
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

// ------------------------------------------------------------------------------------------------
// The Window and View menus
// ------------------------------------------------------------------------------------------------

/// Window: New Window, Close All, Cascade, Tile Vertically, Tile Horizontally, and (on macOS)
/// Workspace › AutoSave / Manual / Save Workspace, as QuickBooks for Mac has; on Windows and Linux
/// the list of open windows follows. View: Open Window List, One Window, Multiple Windows.
///
/// New Window's shortcut is the menu's (Cmd+Shift+N) on macOS; on Windows and Linux the page
/// handles Ctrl+Shift+N, so the item carries none. Runs after `app_menu::install`, which makes the
/// View menu where the platform has none.
pub fn add_menus(app: &AppHandle, menu: &Menu<Wry>) -> tauri::Result<()> {
    let Some(window_menu) = menu.get(WINDOW_SUBMENU_ID).and_then(|item| item.as_submenu().cloned()) else { return Ok(()) };
    let accel = cfg!(target_os = "macos").then_some("CmdOrCtrl+Shift+N");
    let top: Vec<MenuItemKind<Wry>> = vec![
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_NEW_WINDOW, "New Window", true, accel)?),
        MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_CLOSE_ALL, "Close All", true, None::<&str>)?),
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_CASCADE, "Cascade", true, None::<&str>)?),
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_TILE_VERTICALLY, "Tile Vertically", true, None::<&str>)?),
        MenuItemKind::MenuItem(MenuItem::with_id(app, MENU_TILE_HORIZONTALLY, "Tile Horizontally", true, None::<&str>)?),
        MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
    ];
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
        let at = view_menu.items()?.iter().position(|i| matches!(i, MenuItemKind::Predefined(_))).unwrap_or(view_menu.items()?.len());
        let items: [&dyn IsMenuItem<Wry>; 6] =
            [&PredefinedMenuItem::separator(app)?, &open_window_list, &PredefinedMenuItem::separator(app)?, &one_window, &multiple_windows, &PredefinedMenuItem::separator(app)?];
        let take = if at == view_menu.items()?.len() { 5 } else { 6 };
        view_menu.insert_items(&items[..take], at)?;
    }
    app.manage(WindowMenus { window_menu, list: Mutex::new(Vec::new()), open_window_list, one_window, multiple_windows, workspace });
    Ok(())
}

/// The Window menu's list of open windows, the active one checked. macOS adds its own list.
fn refresh_window_list(app: &AppHandle, views: &[OpenWindowView]) {
    if cfg!(target_os = "macos") {
        return;
    }
    let Some(menus) = app.try_state::<WindowMenus>() else { return };
    let mut list = menus.list.lock().unwrap_or_else(|e| e.into_inner());
    for item in list.drain(..) {
        let _ = menus.window_menu.remove(&item);
    }
    if !views.is_empty() {
        if let Ok(separator) = PredefinedMenuItem::separator(app) {
            if menus.window_menu.append(&separator).is_ok() {
                list.push(MenuItemKind::Predefined(separator));
            }
        }
    }
    for (n, view) in views.iter().enumerate() {
        // The first nine are numbered, as Windows programs number their open windows.
        let label = if n < 9 { format!("&{} {}", n + 1, view.name.replace('&', "&&")) } else { view.name.replace('&', "&&") };
        if let Ok(item) = CheckMenuItem::with_id(app, format!("{MENU_FOCUS_PREFIX}{}", view.label), label, true, view.active, None::<&str>) {
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
    windows_changed(app);
}

/// Window and View menu choices; other menu ids are ignored.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        MENU_NEW_WINDOW => new_window(app),
        MENU_CLOSE_ALL => close_all(app),
        MENU_CASCADE => arrange(app, Arrangement::Cascade),
        MENU_TILE_VERTICALLY => arrange(app, Arrangement::TileVertically),
        MENU_TILE_HORIZONTALLY => arrange(app, Arrangement::TileHorizontally),
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
                    // A check item ticks itself when chosen; put it back.
                    refresh_preference_menus(&app, &desktop_view(&app).await);
                }
            });
        }
        _ => {
            if let Some(label) = id.strip_prefix(MENU_FOCUS_PREFIX) {
                let _ = bring_forward(app, label);
                // The item ticks itself when chosen; the focus change sets the ticks right.
                windows_changed(app);
            }
        }
    }
}

/// Window › New Window: the focused window opens a copy of the page it shows; with no LocalBooks
/// window focused, a new window opens on the Dashboard.
fn new_window(app: &AppHandle) {
    let focused = app.webview_windows().into_values().find(|w| w.is_focused().unwrap_or(false));
    match focused {
        Some(window) => {
            let _ = window.emit_to(window.label(), NEW_WINDOW_REQUESTED, ());
        }
        None => {
            if let Err(err) = open(app, None, None) {
                eprintln!("{err}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
