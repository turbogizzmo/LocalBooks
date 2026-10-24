//! The company items at the top of the menu bar's File menu, as in classic desktop accounting
//! software: New Company…, Open or Restore Company…, Open Previous Company ▸ (the recent list), and
//! Close Company.
//!
//! Opening a recent file and closing the company happen here; New Company and Open or Restore need
//! the page (a name prompt, the file dialogs), so those are sent to the focused window as
//! `file-menu-requested` with the action ("new" or "open").

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::storage::RecentCompanyView;

/// Sent to the focused window for File › New Company… ("new") and Open or Restore Company… ("open").
const FILE_MENU_REQUESTED: &str = "file-menu-requested";
const NEW_COMPANY: &str = "file-new-company";
const OPEN_COMPANY: &str = "file-open-company";
const CLOSE_COMPANY: &str = "file-close-company";
const NO_RECENT: &str = "file-no-recent";
/// A recent company's menu id is this prefix followed by its path.
const RECENT_PREFIX: &str = "file-recent:";

/// The items whose contents follow the open company.
struct CompanyItems {
    recent: Submenu<Wry>,
    close: MenuItem<Wry>,
}

/// Adds the company items to the top of the File menu (creating a File menu where the platform's
/// default menu has none).
pub fn add_company_items(app: &AppHandle, menu: &Menu<Wry>) -> tauri::Result<()> {
    let file_menu = menu.items()?.into_iter().filter_map(|item| item.as_submenu().cloned()).find(|sub| sub.text().is_ok_and(|t| t == "File"));
    let file_menu = match file_menu {
        Some(file_menu) => file_menu,
        None => {
            let file_menu = Submenu::new(app, "File", true)?;
            // After the application menu on macOS; first elsewhere.
            menu.insert(&file_menu, if cfg!(target_os = "macos") { 1 } else { 0 })?;
            file_menu
        }
    };
    let recent = Submenu::new(app, "Open Previous Company", true)?;
    let close = MenuItem::with_id(app, CLOSE_COMPANY, "Close Company", false, None::<&str>)?;
    let items: [&dyn IsMenuItem<Wry>; 6] = [
        &MenuItem::with_id(app, NEW_COMPANY, "New Company…", true, None::<&str>)?,
        &MenuItem::with_id(app, OPEN_COMPANY, "Open or Restore Company…", true, Some("CmdOrCtrl+O"))?,
        &recent,
        &PredefinedMenuItem::separator(app)?,
        &close,
        &PredefinedMenuItem::separator(app)?,
    ];
    file_menu.insert_items(&items, 0)?;
    app.manage(CompanyItems { recent, close });
    Ok(())
}

/// Lists the recent company files under Open Previous Company (a missing file is shown greyed out)
/// and enables Close Company while a company is open.
pub fn refresh(app: &AppHandle, recent: &[RecentCompanyView], company_open: bool) {
    let Some(items) = app.try_state::<CompanyItems>() else { return };
    if let Err(err) = fill(app, &items, recent, company_open) {
        eprintln!("cannot update the File menu: {err}");
    }
}

fn fill(app: &AppHandle, items: &CompanyItems, recent: &[RecentCompanyView], company_open: bool) -> tauri::Result<()> {
    items.close.set_enabled(company_open)?;
    for item in items.recent.items()? {
        items.recent.remove(&item)?;
    }
    if recent.is_empty() {
        items.recent.append(&MenuItem::with_id(app, NO_RECENT, "No Recent Companies", false, None::<&str>)?)?;
    }
    for company in recent {
        let label = if company.missing {
            format!("{} — {} (not found)", company.company_name, company.path)
        } else {
            format!("{} — {}", company.company_name, company.path)
        };
        items.recent.append(&MenuItem::with_id(app, format!("{RECENT_PREFIX}{}", company.path), label, !company.missing, None::<&str>)?)?;
    }
    Ok(())
}

/// Carries out a File menu choice; other menu ids are ignored.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        NEW_COMPANY => ask_page(app, "new"),
        OPEN_COMPANY => ask_page(app, "open"),
        CLOSE_COMPANY => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = crate::commands::close_company(&app).await {
                    eprintln!("{err}");
                }
            });
        }
        _ => {
            if let Some(path) = id.strip_prefix(RECENT_PREFIX) {
                let (app, path) = (app.clone(), std::path::PathBuf::from(path));
                tauri::async_runtime::spawn(async move { crate::commands::handle_hand_off(&app, crate::instance::HandOff::Open { path }).await });
            }
        }
    }
}

/// Asks the focused window (or the main one) to run a File menu action that needs the page.
fn ask_page(app: &AppHandle, action: &str) {
    let windows = app.webview_windows();
    let target = windows
        .values()
        .find(|w| w.is_focused().unwrap_or(false))
        .or_else(|| windows.get(crate::windows::MAIN))
        .or_else(|| windows.values().next());
    if let Some(window) = target {
        let _ = window.emit_to(window.label(), FILE_MENU_REQUESTED, action);
        crate::windows::focus(app);
    }
}
