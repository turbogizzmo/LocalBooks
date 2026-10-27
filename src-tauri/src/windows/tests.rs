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

fn page(view: &str) -> Page {
    Page { view: view.into(), action: None, data: None }
}

#[test]
fn the_desktop_reopens_once_and_not_after_opening_without_restoring_windows() {
    let mut reg = Registry::default();
    assert!(!reg.take_restore(), "nothing to reopen before a company opens");
    reg.restore_pending = true;
    assert!(reg.take_restore(), "the first window to load reopens the desktop");
    assert!(!reg.take_restore(), "the windows it opens do not reopen it again");
    // File › Open Company Without Restoring Windows… skips it this once.
    reg.restore_pending = false;
    assert!(!reg.take_restore());
}

#[test]
fn windows_are_listed_in_order_with_the_main_window_first_and_the_active_one_marked() {
    let mut reg = Registry::default();
    reg.report("window-2", page("reports"), Some("Profit & Loss".into()), None, false);
    reg.report(MAIN, page("dashboard"), Some("Dashboard".into()), None, false);
    reg.report("window-3", page("sales/invoices"), Some("Invoice 1001 · Acme".into()), None, true);
    // A window reporting again keeps its place.
    reg.report("window-2", page("reports"), Some("Balance Sheet".into()), None, false);
    let list = list_of(&reg);
    assert_eq!(list.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), ["Dashboard", "Balance Sheet", "Invoice 1001 · Acme"]);
    assert_eq!(list.iter().filter(|w| w.active).map(|w| w.label.as_str()).collect::<Vec<_>>(), ["window-3"]);
}

#[test]
fn a_closed_window_leaves_the_list_but_the_last_one_stays_to_be_saved() {
    let mut reg = Registry::default();
    reg.report(MAIN, page("dashboard"), None, None, false);
    reg.report("window-2", page("reports"), None, None, true);
    reg.closed("window-2");
    assert_eq!(reg.windows.len(), 1);
    assert_eq!(reg.focused, None);
    // Quitting by closing the last window: it is still there for the desktop to save.
    reg.closed(MAIN);
    assert_eq!(reg.windows.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), [MAIN]);
    assert_eq!(list_of(&reg)[0].name, "LocalBooks", "a window that names nothing is listed as LocalBooks");
}

#[test]
fn titles_follow_the_company_and_what_the_window_shows() {
    let mut reg = Registry::default();
    assert_eq!(title(&reg, Some("Checking Register")), "No Company Open — LocalBooks");
    reg.company_open = true;
    reg.company = Some("Acme Ltd".into());
    assert_eq!(title(&reg, Some("Checking Register")), "Checking Register — Acme Ltd");
    assert_eq!(title(&reg, None), "Acme Ltd — LocalBooks");
}
