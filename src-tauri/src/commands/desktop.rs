//! Windows inside LocalBooks and the saved desktop: what each LocalBooks window has open, Window ›
//! Cascade and Tile, Desktop View preferences, and reopening the desktop when a company opens (see
//! `windows.rs` and `desktop.rs`). None of these touch the books.

use super::prelude::*;
use crate::desktop::{Arrangement, Bounds, PreferenceChange};
use crate::windows::{ChildWindow, DesktopView, Restored};

handlers![
    report_child_windows,
    arrange_windows,
    take_desktop,
    get_desktop_view,
    set_desktop_view,
    set_window_preferences,
];

/// The calling LocalBooks window reports the windows open inside it and the active one: it is
/// retitled, the Window menu lists them, and they are what the desktop saves.
#[tauri::command]
pub async fn report_child_windows(app: AppHandle, window: tauri::WebviewWindow, children: Vec<ChildWindow>, active: Option<String>) -> Result<(), String> {
    windows::report_children(&app, &window, children, active);
    Ok(())
}

/// Window › Cascade ("cascade"), Tile Vertically ("tile-vertically") or Tile Horizontally
/// ("tile-horizontally"): where `count` windows go in the work area `area`.
#[tauri::command]
pub async fn arrange_windows(arrangement: String, count: usize, area: Bounds) -> Result<Vec<Bounds>, String> {
    let arrangement = match arrangement.as_str() {
        "cascade" => Arrangement::Cascade,
        "tile-vertically" => Arrangement::TileVertically,
        "tile-horizontally" => Arrangement::TileHorizontally,
        other => return Err(format!("WINDOW_ARRANGE: unknown arrangement {other}")),
    };
    Ok(crate::desktop::arrange(arrangement, count.min(crate::desktop::MAX_WINDOWS * 4), area))
}

/// Asked by a LocalBooks window as it loads the open company: the windows to reopen inside it if
/// the company has just opened with a saved desktop, else `None`. `known_views` are the pages the
/// app has; saved windows on any other are skipped.
#[tauri::command]
pub async fn take_desktop(app: AppHandle, window: tauri::WebviewWindow, known_views: Vec<String>) -> Result<Option<Restored>, String> {
    Ok(windows::take_restore(&app, &window, &known_views).await)
}

/// Edit › Preferences › Desktop View, for this user of this company file on this computer.
#[tauri::command]
pub async fn get_desktop_view(app: AppHandle) -> Result<DesktopView, String> {
    Ok(windows::desktop_view(&app).await)
}

/// Desktop View › Desktop: `choice` is "save-when-closing", "save-current" (saves the windows open
/// now), "dont-save" or "keep-saved" (Keep previously saved desktop).
#[tauri::command]
pub async fn set_desktop_view(app: AppHandle, choice: String) -> Result<DesktopView, String> {
    windows::set_desktop_mode(&app, &choice).await
}

/// View › One Window / Multiple Windows, View › Open Window List, and Desktop View's left pane and
/// what a company opens on: only the fields of `change` given change.
#[tauri::command]
pub async fn set_window_preferences(app: AppHandle, change: PreferenceChange) -> Result<DesktopView, String> {
    windows::set_window_preferences(&app, change).await
}
