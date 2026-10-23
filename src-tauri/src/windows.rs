//! More than one window on the open company, as in classic desktop accounting software: Window ›
//! New Window (Cmd/Ctrl+Shift+N), and a register or report opened in a window of its own.
//!
//! Every window is the same app on the same backend state, so they all read and write the one open
//! company file. A write in any window emits `ledger-changed` to all of them, so each refreshes what
//! it shows; opening another company file reloads every window. The page a new window starts on is
//! carried in its URL fragment (`#view=…&action=…&data=…`), which the app reads at startup.

use std::sync::atomic::{AtomicUsize, Ordering};

use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

/// The window LocalBooks opens at launch.
pub const MAIN: &str = "main";
/// Labels of the extra windows; `capabilities/default.json` grants them the main window's access.
const EXTRA_PREFIX: &str = "window-";
/// Sent to the focused window when Window › New Window is chosen, so it opens the page it shows.
#[cfg(target_os = "macos")]
const NEW_WINDOW_REQUESTED: &str = "new-window-requested";
#[cfg(target_os = "macos")]
const MENU_NEW_WINDOW: &str = "new-window";

static NEXT_WINDOW: AtomicUsize = AtomicUsize::new(1);

/// The app page for a new window: `view` is a navigation id ("reports"), `action` and `data` the
/// request that view should open with (as from the app's own navigation).
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

/// Opens another window on the open company, titled like the others.
pub fn open(app: &AppHandle, title: &str, view: Option<&str>, action: Option<&str>, data: Option<&serde_json::Value>) -> Result<String, String> {
    let label = loop {
        let label = format!("{EXTRA_PREFIX}{}", NEXT_WINDOW.fetch_add(1, Ordering::Relaxed));
        if app.get_webview_window(&label).is_none() {
            break label;
        }
    };
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(start_url(view, action, data).into()))
        .title(title)
        .inner_size(1280.0, 820.0)
        .min_inner_size(1100.0, 640.0)
        .build()
        .map_err(|e| format!("WINDOW_OPEN: cannot open another window: {e}"))?;
    Ok(label)
}

/// Brings LocalBooks forward: the main window, or another one if the main window was closed.
pub fn focus(app: &AppHandle) {
    let windows = app.webview_windows();
    if let Some(window) = windows.get(MAIN).or_else(|| windows.values().next()) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// macOS: the standard menu bar, with New Window (Cmd+Shift+N) at the top of the Window menu.
#[cfg(target_os = "macos")]
pub fn install_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, WINDOW_SUBMENU_ID};

    let menu = Menu::default(app)?;
    if let Some(window_menu) = menu.get(WINDOW_SUBMENU_ID).and_then(|item| item.as_submenu().cloned()) {
        window_menu.insert(&MenuItem::with_id(app, MENU_NEW_WINDOW, "New Window", true, Some("CmdOrCtrl+Shift+N"))?, 0)?;
        window_menu.insert(&PredefinedMenuItem::separator(app)?, 1)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

/// Window › New Window: the focused window opens a copy of the page it shows; with no LocalBooks
/// window focused, a new window opens on the Dashboard.
#[cfg(target_os = "macos")]
pub fn on_menu_event(app: &AppHandle, id: &str) {
    if id != MENU_NEW_WINDOW {
        return;
    }
    let focused = app.webview_windows().into_values().find(|w| w.is_focused().unwrap_or(false));
    match focused {
        Some(window) => {
            use tauri::Emitter;
            let _ = window.emit_to(window.label(), NEW_WINDOW_REQUESTED, ());
        }
        None => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let title = crate::commands::window_title(&app).await;
                if let Err(err) = open(&app, &title, None, None, None) {
                    eprintln!("{err}");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_urls_carry_the_page_in_the_fragment() {
        assert_eq!(start_url(None, None, None), "index.html");
        assert_eq!(start_url(Some("reports"), None, None), "index.html#view=reports");
        let data = serde_json::json!({ "accountId": 7, "startDate": "2026-01-01" });
        let url = start_url(Some("accounting/register"), Some("open"), Some(&data));
        assert_eq!(url, "index.html#view=accounting%2Fregister&action=open&data=%7B%22accountId%22%3A7%2C%22startDate%22%3A%222026-01-01%22%7D");
        assert_eq!(start_url(Some("reports"), None, Some(&serde_json::Value::Null)), "index.html#view=reports");
    }
}
