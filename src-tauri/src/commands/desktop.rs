//! Windows and the saved desktop: what each window shows, the Open Window List, Desktop View
//! preferences, and reopening the desktop when a company opens (see `windows.rs` and `desktop.rs`).
//! None of these touch the books.

use super::prelude::*;
use crate::desktop::{Page, WindowMode};
use crate::windows::{DesktopView, OpenWindowView};

handlers![
    report_window_page,
    list_open_windows,
    focus_open_window,
    take_desktop,
    get_desktop_view,
    set_desktop_view,
    set_window_preferences,
];

/// The calling window shows `view` (with the request it opened with) and `content`, what is in it
/// ("Invoice 1001 · Acme"): it is retitled and listed, and saved with the desktop.
#[tauri::command]
pub async fn report_window_page(app: AppHandle, window: tauri::WebviewWindow, view: String, action: Option<String>, data: Option<serde_json::Value>, content: Option<String>) -> Result<(), String> {
    windows::report_page(&app, &window, Page { view, action, data }, content);
    Ok(())
}

/// View › Open Window List: every open window, the active one marked.
#[tauri::command]
pub async fn list_open_windows(app: AppHandle) -> Result<Vec<OpenWindowView>, String> {
    Ok(windows::list(&app))
}

/// Brings an open window forward.
#[tauri::command]
pub async fn focus_open_window(app: AppHandle, label: String) -> Result<(), String> {
    windows::bring_forward(&app, &label)
}

/// Asked by a window as it loads the open company: the page to show if the company has just opened
/// and its saved desktop reopens (the other saved windows open beside it), else `None`.
/// `known_views` are the pages the app has; saved windows on any other are skipped.
#[tauri::command]
pub async fn take_desktop(app: AppHandle, window: tauri::WebviewWindow, known_views: Vec<String>) -> Result<Option<Page>, String> {
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

/// View › One Window / Multiple Windows and View › Open Window List.
#[tauri::command]
pub async fn set_window_preferences(app: AppHandle, window_mode: Option<WindowMode>, show_open_window_list: Option<bool>) -> Result<DesktopView, String> {
    windows::set_window_preferences(&app, window_mode, show_open_window_list).await
}
