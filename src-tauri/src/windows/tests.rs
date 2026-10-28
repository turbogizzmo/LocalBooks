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

fn child(id: &str, name: &str, view: &str) -> ChildWindow {
    ChildWindow {
        id: id.into(),
        name: name.into(),
        page: Page { view: view.into(), action: None, data: None },
        bounds: Some(Bounds { x: 10.0, y: 20.0, width: 600.0, height: 400.0 }),
        minimized: false,
        maximized: false,
    }
}

#[test]
fn the_desktop_reopens_once_and_not_after_opening_without_restoring_windows() {
    let mut reg = Registry::default();
    assert!(!reg.take_restore(), "nothing to reopen before a company opens");
    reg.restore_pending = true;
    assert!(reg.take_restore(), "the first window to load reopens the desktop");
    assert!(!reg.take_restore(), "a later reload does not reopen it again");
    // File › Open Company Without Restoring Windows… skips it this once.
    reg.restore_pending = false;
    assert!(!reg.take_restore());
}

#[test]
fn the_desktop_is_the_windows_inside_the_main_window_with_the_active_one_last() {
    let mut reg = Registry::default();
    let frame = Some(Bounds { x: 0.0, y: 25.0, width: 1440.0, height: 875.0 });
    reg.report("window-2", vec![child("w1", "Balance Sheet", "reports")], Some("w1".into()), None);
    let mut minimized = child("w3", "Checking Register", "accounting/register");
    minimized.minimized = true;
    reg.report(MAIN, vec![child("w1", "Home", "dashboard"), child("w2", "Invoice 1001 · Acme", "sales/invoices"), minimized], Some("w2".into()), frame);
    let snap = reg.snapshot();
    assert_eq!(snap.frame, frame);
    assert_eq!(snap.windows.iter().map(|w| w.page.view.as_str()).collect::<Vec<_>>(), ["dashboard", "accounting/register", "sales/invoices"]);
    assert!(snap.windows[1].minimized);
    assert_eq!(snap.windows[0].bounds, Some(Bounds { x: 10.0, y: 20.0, width: 600.0, height: 400.0 }));
    // A report without a place keeps the frame known before.
    reg.report(MAIN, vec![], None, None);
    assert_eq!(reg.snapshot().frame, frame);
}

#[test]
fn the_window_menu_lists_the_focused_localbooks_windows_contents() {
    let mut reg = Registry::default();
    reg.report(MAIN, vec![child("w1", "Home", "dashboard")], Some("w1".into()), None);
    reg.report("window-2", vec![child("w1", "Profit and Loss", "reports")], Some("w1".into()), None);
    assert_eq!(reg.menu_target().map(|(l, _)| l.as_str()), Some(MAIN), "the main window when none is focused");
    reg.focused = Some("window-2".into());
    assert_eq!(reg.menu_target().unwrap().1.active_child().map(|c| c.name.as_str()), Some("Profit and Loss"));
}

#[test]
fn a_closed_localbooks_window_leaves_the_list_but_the_last_one_stays_to_be_saved() {
    let mut reg = Registry::default();
    reg.report(MAIN, vec![child("w1", "Home", "dashboard")], None, None);
    reg.report("window-2", vec![], None, None);
    reg.focused = Some("window-2".into());
    reg.closed("window-2");
    assert_eq!(reg.windows.len(), 1);
    assert_eq!(reg.focused, None);
    // Quitting by closing the last window: its windows are still there for the desktop to save.
    reg.closed(MAIN);
    assert_eq!(reg.snapshot().windows.len(), 1);
}

#[test]
fn titles_follow_the_company_and_the_active_window() {
    let mut reg = Registry::default();
    assert_eq!(title(&reg, Some("Checking Register")), "No Company Open — LocalBooks");
    reg.company_open = true;
    reg.company = Some("Acme Ltd".into());
    assert_eq!(title(&reg, Some("Checking Register")), "Checking Register — Acme Ltd");
    assert_eq!(title(&reg, None), "Acme Ltd — LocalBooks");
}
