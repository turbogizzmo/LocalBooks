//! The saved desktop and the window preferences, as QuickBooks Desktop keeps them in Edit ›
//! Preferences › Desktop View › My Preferences: View (One Window or Multiple Windows) and Desktop
//! (Save when closing company, Save current desktop, Don't save the desktop, Keep previously saved
//! desktop).
//!
//! A desktop is the set of windows open on a company: each window's page (a navigation id with the
//! request it opened with) and where it was on screen. It is kept per company file and per signed-in
//! user, on this computer, in `desktops.json` beside `config.json`, so reopening the company brings
//! the same windows back. Nothing here touches the books.

#![allow(clippy::float_arithmetic, reason = "window positions and sizes on screen, in points; no amounts are computed here")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many windows a desktop keeps; any more are left out.
pub const MAX_WINDOWS: usize = 12;
/// A page's request larger than this (serialized) is not kept: pages carry ids and dates, not data.
const MAX_DATA_BYTES: usize = 4096;
/// How much of a window must be on a screen, in points, for it to reopen where it was.
const MIN_VISIBLE: f64 = 80.0;

/// Desktop View › Desktop: what happens to the open windows when the company closes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SaveMode {
    /// "Save when closing company": the windows open at close reopen next time (QuickBooks' default).
    #[default]
    SaveWhenClosing,
    /// "Save current desktop", then shown as "Keep previously saved desktop": the windows saved at
    /// that moment reopen every time, whatever is open at close.
    KeepSaved,
    /// "Don't save the desktop": the company opens on the Home page alone.
    DontSave,
}

/// Desktop View › View, and View › One Window / Multiple Windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowMode {
    /// Lists open a transaction or report in place, in the window that shows the list.
    One,
    /// Lists open each transaction or report in a window of its own (QuickBooks' default).
    #[default]
    Multiple,
}

/// A window's page: a navigation id ("sales/invoices") and the request it opens with, as the app's
/// own navigation passes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub view: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Where a window is on screen, in points (logical pixels): its outer position and inner size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One window of a saved desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedWindow {
    pub page: Page,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub maximized: bool,
}

/// A company's desktop for one user. The first window is the one the company opens in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompanyDesktop {
    #[serde(default)]
    pub mode: SaveMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<SavedWindow>,
}

/// `desktops.json`: the window preferences and every company's saved desktop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopFile {
    #[serde(default)]
    pub window_mode: WindowMode,
    /// View › Open Window List: the list of open windows beside the sidebar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_open_window_list: bool,
    /// By [`key`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub companies: BTreeMap<String, CompanyDesktop>,
}

pub fn file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("desktops.json")
}

/// Reads `desktops.json`; a missing or unreadable file means defaults (nothing saved).
pub fn read(config_dir: &Path) -> DesktopFile {
    std::fs::read(file_path(config_dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn write(config_dir: &Path, file: &DesktopFile) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("DESKTOP_SAVE: cannot create {}: {e}", config_dir.display()))?;
    let body = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
    crate::export_engine::write_atomically(&file_path(config_dir), &body).map_err(|e| e.replace("EXPORT_WRITE", "DESKTOP_SAVE"))
}

/// Reads `desktops.json`, applies `change`, and writes it back.
pub fn update(config_dir: &Path, change: impl FnOnce(&mut DesktopFile)) -> Result<(), String> {
    let mut file = read(config_dir);
    change(&mut file);
    write(config_dir, &file)
}

/// The key a company's desktop is kept under: the company file (resolved, so two spellings of one
/// file share a desktop) and, when the file has users, who is signed in.
pub fn key(company_file: &Path, user_id: Option<i64>) -> String {
    let path = company_file.canonicalize().unwrap_or_else(|_| company_file.to_path_buf());
    match user_id {
        Some(id) => format!("{}#user{id}", path.display()),
        None => path.display().to_string(),
    }
}

/// The windows worth keeping: at most [`MAX_WINDOWS`], each with a page and a small request.
fn keepable(windows: Vec<SavedWindow>) -> Vec<SavedWindow> {
    windows
        .into_iter()
        .filter(|w| !w.page.view.is_empty())
        .map(|mut w| {
            if w.page.data.as_ref().is_some_and(|d| d.is_null() || d.to_string().len() > MAX_DATA_BYTES) {
                w.page.data = None;
            }
            w
        })
        .take(MAX_WINDOWS)
        .collect()
}

/// The company closes with `windows` open: under "Save when closing company" they become its
/// desktop. Whether anything changed.
pub fn company_closing(file: &mut DesktopFile, key: &str, windows: Vec<SavedWindow>) -> bool {
    let desktop = file.companies.entry(key.to_string()).or_default();
    if desktop.mode != SaveMode::SaveWhenClosing {
        return false;
    }
    let windows = keepable(windows);
    let changed = desktop.windows != windows;
    desktop.windows = windows;
    changed
}

/// "Save current desktop" (and Window › Workspace › Save Workspace on the Mac): `windows` become the
/// desktop that reopens every time, until the setting is changed.
pub fn save_current(file: &mut DesktopFile, key: &str, windows: Vec<SavedWindow>) {
    let desktop = file.companies.entry(key.to_string()).or_default();
    desktop.mode = SaveMode::KeepSaved;
    desktop.windows = keepable(windows);
}

/// Chooses what happens at close. "Don't save the desktop" forgets the saved windows; "Keep
/// previously saved desktop" keeps them as they are, and so does "Save when closing company" until
/// the company next closes.
pub fn set_mode(file: &mut DesktopFile, key: &str, mode: SaveMode) {
    let desktop = file.companies.entry(key.to_string()).or_default();
    desktop.mode = mode;
    if mode == SaveMode::DontSave {
        desktop.windows.clear();
    }
}

/// The desktop the company reopens with: its saved windows whose page is one the app has
/// (`known_view`), none under "Don't save the desktop". A screen that no longer exists is skipped,
/// never in the way of the others.
pub fn to_restore(file: &DesktopFile, key: &str, known_view: impl Fn(&str) -> bool) -> Vec<SavedWindow> {
    let Some(desktop) = file.companies.get(key) else { return Vec::new() };
    if desktop.mode == SaveMode::DontSave {
        return Vec::new();
    }
    desktop.windows.iter().filter(|w| known_view(&w.page.view)).take(MAX_WINDOWS).cloned().collect()
}

/// `saved` moved onto the screens there are now (`screens`, in points): unchanged when enough of
/// its top edge is on one of them, made to fit a screen it overlaps when it is larger, and `None`
/// (open where the system puts it) when it would be off every screen, as after a monitor is unplugged.
pub fn fit(saved: Bounds, screens: &[Bounds]) -> Option<Bounds> {
    if !(saved.width > 0.0 && saved.height > 0.0) || ![saved.x, saved.y, saved.width, saved.height].iter().all(|v| v.is_finite()) {
        return None;
    }
    let overlap = |s: &Bounds| {
        let w = (saved.x + saved.width).min(s.x + s.width) - saved.x.max(s.x);
        // The title bar: the top of the window, where it is dragged from.
        let h = (saved.y + MIN_VISIBLE).min(s.y + s.height) - saved.y.max(s.y);
        w >= MIN_VISIBLE && h >= MIN_VISIBLE / 2.0
    };
    let screen = screens.iter().find(|s| overlap(s))?;
    let width = saved.width.min(screen.width);
    let height = saved.height.min(screen.height);
    let x = saved.x.clamp(screen.x, screen.x + screen.width - width);
    let y = saved.y.clamp(screen.y, screen.y + screen.height - height);
    Some(Bounds { x, y, width, height })
}

/// A window's title: what it shows ("Invoice 1001 · Acme", "Checking Register") and the company,
/// so the Window menu and the Open Window List read clearly; the company alone when the window
/// names nothing more.
pub fn window_title(content: Option<&str>, company: Option<&str>) -> String {
    let content = content.map(str::trim).filter(|c| !c.is_empty());
    match (content, company) {
        (Some(content), Some(company)) => format!("{content} — {company}"),
        (Some(content), None) => format!("{content} — LocalBooks"),
        (None, Some(company)) => format!("{company} — LocalBooks"),
        (None, None) => "LocalBooks".to_string(),
    }
}

/// Windows laid out over `area` (a screen's work area, in points), in the order given: Window ›
/// Tile Vertically puts them side by side, Tile Horizontally one above the other, and Cascade
/// staggers them so each title bar shows.
pub fn arrange(arrangement: Arrangement, count: usize, area: Bounds) -> Vec<Bounds> {
    if count == 0 {
        return Vec::new();
    }
    let n = count as f64;
    match arrangement {
        Arrangement::TileVertically => {
            let width = (area.width / n).floor();
            (0..count).map(|i| Bounds { x: area.x + width * i as f64, y: area.y, width, height: area.height }).collect()
        }
        Arrangement::TileHorizontally => {
            let height = (area.height / n).floor();
            (0..count).map(|i| Bounds { x: area.x, y: area.y + height * i as f64, width: area.width, height }).collect()
        }
        Arrangement::Cascade => {
            const STEP: f64 = 28.0;
            // Each window is the work area less the room for every step, so the last one still fits.
            let steps = (n - 1.0).min(((area.height * 0.4) / STEP).floor().max(0.0));
            let width = (area.width - STEP * steps).max(area.width * 0.5);
            let height = (area.height - STEP * steps).max(area.height * 0.5);
            (0..count)
                .map(|i| {
                    let step = (i as f64) % (steps + 1.0);
                    Bounds { x: area.x + STEP * step, y: area.y + STEP * step, width, height }
                })
                .collect()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    Cascade,
    TileVertically,
    TileHorizontally,
}

#[cfg(test)]
mod tests;
