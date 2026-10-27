use super::*;
use serde_json::json;

fn window(view: &str) -> SavedWindow {
    SavedWindow { page: Page { view: view.into(), action: None, data: None }, bounds: None, maximized: false, minimized: false }
}

fn report(id: &str) -> SavedWindow {
    SavedWindow { page: Page { view: "reports".into(), action: Some("open".into()), data: Some(json!({ "report": id })) }, bounds: None, maximized: false, minimized: false }
}

fn snap(windows: Vec<SavedWindow>) -> Snapshot {
    Snapshot { windows, frame: None }
}

/// A fresh folder for one test.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-desktop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const KEY: &str = "/books/Acme.localbooks";
const KNOWN: fn(&str) -> bool = |view| ["dashboard", "reports", "sales/invoices", "accounting/register"].contains(&view);

#[test]
fn save_when_closing_company_is_the_default_and_keeps_the_open_windows() {
    let mut file = DesktopFile::default();
    assert_eq!(file.window_mode, WindowMode::Multiple);
    assert!(company_closing(&mut file, KEY, snap(vec![window("dashboard"), report("pl"), window("sales/invoices")])));
    assert_eq!(file.companies[KEY].mode, SaveMode::SaveWhenClosing);
    let restored = to_restore(&file, KEY, KNOWN);
    assert_eq!(restored, vec![window("dashboard"), report("pl"), window("sales/invoices")]);
    // The next close replaces them with what is open then.
    assert!(company_closing(&mut file, KEY, snap(vec![window("accounting/register")])));
    assert_eq!(to_restore(&file, KEY, KNOWN), vec![window("accounting/register")]);
    assert!(!company_closing(&mut file, KEY, snap(vec![window("accounting/register")])), "nothing changed");
}

#[test]
fn save_current_desktop_reopens_the_same_windows_whatever_is_open_at_close() {
    let mut file = DesktopFile::default();
    save_current(&mut file, KEY, snap(vec![window("dashboard"), report("ar")]));
    assert_eq!(file.companies[KEY].mode, SaveMode::KeepSaved);
    assert!(!company_closing(&mut file, KEY, snap(vec![window("sales/invoices")])));
    assert_eq!(to_restore(&file, KEY, KNOWN), vec![window("dashboard"), report("ar")]);
    // "Keep previously saved desktop" changes nothing; going back to saving at close does at the next close.
    set_mode(&mut file, KEY, SaveMode::KeepSaved);
    assert_eq!(to_restore(&file, KEY, KNOWN).len(), 2);
    set_mode(&mut file, KEY, SaveMode::SaveWhenClosing);
    assert_eq!(to_restore(&file, KEY, KNOWN).len(), 2);
    company_closing(&mut file, KEY, snap(vec![window("sales/invoices")]));
    assert_eq!(to_restore(&file, KEY, KNOWN), vec![window("sales/invoices")]);
}

#[test]
fn dont_save_the_desktop_forgets_the_windows_and_restores_none() {
    let mut file = DesktopFile::default();
    company_closing(&mut file, KEY, snap(vec![window("dashboard"), report("pl")]));
    set_mode(&mut file, KEY, SaveMode::DontSave);
    assert!(file.companies[KEY].windows.is_empty());
    assert!(!company_closing(&mut file, KEY, snap(vec![report("bs")])));
    assert!(to_restore(&file, KEY, KNOWN).is_empty());
}

#[test]
fn a_screen_that_no_longer_exists_is_skipped_and_the_rest_restore() {
    let mut file = DesktopFile::default();
    company_closing(&mut file, KEY, snap(vec![window("dashboard"), window("accounting/retired-screen"), report("pl"), window("")]));
    let restored = to_restore(&file, KEY, KNOWN);
    assert_eq!(restored, vec![window("dashboard"), report("pl")]);
    assert!(to_restore(&file, "/books/Other.localbooks", KNOWN).is_empty(), "another company has its own desktop");
}

#[test]
fn desktops_keep_a_bounded_number_of_small_windows() {
    let mut file = DesktopFile::default();
    let mut big = window("reports");
    big.page.data = Some(json!({ "blob": "x".repeat(5000) }));
    let many: Vec<_> = std::iter::once(big).chain((0..30).map(|_| window("dashboard"))).collect();
    company_closing(&mut file, KEY, snap(many));
    let saved = &file.companies[KEY].windows;
    assert_eq!(saved.len(), MAX_WINDOWS);
    assert_eq!(saved[0].page.data, None, "an oversized request is dropped, the window kept");
}

#[test]
fn desktops_are_kept_per_company_file_and_user() {
    let dir = scratch("keys");
    let file = dir.join("Acme.localbooks");
    std::fs::write(&file, b"").unwrap();
    let plain = key(&file, None);
    assert_eq!(plain, key(&dir.join(".").join("Acme.localbooks"), None), "two spellings of one file share a desktop");
    assert_ne!(key(&file, Some(1)), key(&file, Some(2)));
    assert_ne!(key(&file, Some(1)), plain);
}

#[test]
fn desktops_round_trip_through_their_file_and_a_damaged_file_means_none() {
    let dir = scratch("file");
    let mut saved = report("pl");
    saved.bounds = Some(Bounds { x: 40.0, y: 60.0, width: 900.0, height: 700.0 });
    update(&dir, |file| {
        file.window_mode = WindowMode::One;
        file.show_open_window_list = true;
        company_closing(file, KEY, snap(vec![window("dashboard"), saved.clone()]));
    })
    .unwrap();
    let file = read(&dir);
    assert_eq!(file.window_mode, WindowMode::One);
    assert!(file.show_open_window_list);
    assert_eq!(to_restore(&file, KEY, KNOWN), vec![window("dashboard"), saved]);
    std::fs::write(file_path(&dir), b"{ not json").unwrap();
    assert_eq!(read(&dir), DesktopFile::default());
}

#[test]
fn windows_reopen_where_they_were_while_that_spot_is_still_on_a_screen() {
    let laptop = Bounds { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 };
    let external = Bounds { x: 1440.0, y: 0.0, width: 1920.0, height: 1080.0 };
    let on_external = Bounds { x: 1600.0, y: 100.0, width: 1000.0, height: 700.0 };
    assert_eq!(fit(on_external, &[laptop, external]), Some(on_external));
    // The external screen was unplugged: the system places the window.
    assert_eq!(fit(on_external, &[laptop]), None);
    // Larger than the screen it is on: made to fit it.
    let huge = Bounds { x: 100.0, y: 50.0, width: 3000.0, height: 2000.0 };
    assert_eq!(fit(huge, &[laptop]), Some(Bounds { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 }));
    // Hanging off the right edge but with its title bar showing: pulled back on.
    assert_eq!(fit(Bounds { x: 1300.0, y: 10.0, width: 800.0, height: 600.0 }, &[laptop]), Some(Bounds { x: 640.0, y: 10.0, width: 800.0, height: 600.0 }));
    assert_eq!(fit(Bounds { x: 0.0, y: 0.0, width: f64::NAN, height: 10.0 }, &[laptop]), None);
}

#[test]
fn window_titles_name_what_the_window_shows() {
    assert_eq!(window_title(Some("Invoice 1001 · Acme"), Some("Acme Ltd")), "Invoice 1001 · Acme — Acme Ltd");
    assert_eq!(window_title(Some("Checking Register"), None), "Checking Register — LocalBooks");
    assert_eq!(window_title(Some("  "), Some("Acme Ltd")), "Acme Ltd — LocalBooks");
    assert_eq!(window_title(None, Some("Acme Ltd")), "Acme Ltd — LocalBooks");
    assert_eq!(window_title(None, None), "LocalBooks");
}

#[test]
fn tiling_and_cascading_fill_the_screen() {
    let area = Bounds { x: 0.0, y: 25.0, width: 1500.0, height: 875.0 };
    let tiled = arrange(Arrangement::TileVertically, 3, area);
    assert_eq!(tiled.iter().map(|b| (b.x, b.width)).collect::<Vec<_>>(), vec![(0.0, 500.0), (500.0, 500.0), (1000.0, 500.0)]);
    assert!(tiled.iter().all(|b| b.y == 25.0 && b.height == 875.0));
    let stacked = arrange(Arrangement::TileHorizontally, 2, area);
    assert_eq!(stacked.iter().map(|b| (b.y, b.height)).collect::<Vec<_>>(), vec![(25.0, 437.0), (462.0, 437.0)]);
    let cascaded = arrange(Arrangement::Cascade, 3, area);
    assert_eq!(cascaded.iter().map(|b| (b.x, b.y)).collect::<Vec<_>>(), vec![(0.0, 25.0), (28.0, 53.0), (56.0, 81.0)]);
    for b in &cascaded {
        assert!(b.x + b.width <= area.x + area.width && b.y + b.height <= area.y + area.height, "{b:?} fits");
    }
    assert!(arrange(Arrangement::Cascade, 0, area).is_empty());
}

#[test]
fn the_view_mode_and_the_localbooks_window_are_kept_with_each_company() {
    let mut file = DesktopFile::default();
    assert_eq!(window_mode(&file, Some(KEY)), WindowMode::Multiple, "Multiple Windows by default, as in QuickBooks");
    set_window_mode(&mut file, Some(KEY), WindowMode::One);
    assert_eq!(window_mode(&file, Some(KEY)), WindowMode::One);
    assert_eq!(window_mode(&file, Some("/books/New.localbooks")), WindowMode::One, "the last choice for a company with none yet");
    set_window_mode(&mut file, Some("/books/Other.localbooks"), WindowMode::Multiple);
    assert_eq!(window_mode(&file, Some(KEY)), WindowMode::One, "each company keeps its own");
    let frame = Bounds { x: 10.0, y: 20.0, width: 1300.0, height: 850.0 };
    let mut minimized = report("ar");
    minimized.minimized = true;
    company_closing(&mut file, KEY, Snapshot { windows: vec![window("dashboard"), minimized.clone()], frame: Some(frame) });
    assert_eq!(file.companies[KEY].frame, Some(frame));
    assert_eq!(to_restore(&file, KEY, KNOWN), vec![window("dashboard"), minimized]);
    // Quitting with no window to measure keeps the place saved before.
    company_closing(&mut file, KEY, snap(vec![window("dashboard")]));
    assert_eq!(file.companies[KEY].frame, Some(frame));
}

#[test]
fn the_left_pane_and_what_a_company_opens_on_round_trip_through_their_file() {
    let dir = scratch("start");
    assert_eq!(read(&dir).sidebar, SidebarMode::Menu, "the full menu by default");
    assert_eq!(read(&dir).start_on, StartOn::LastWindows, "the saved desktop by default");
    let mut refused = None;
    update(&dir, |file| {
        set_preferences(file, Some(KEY), PreferenceChange { sidebar: Some(SidebarMode::OpenWindows), start_on: Some(StartOn::Screen), start_screen: Some("sales/invoices".into()), ..Default::default() }).unwrap();
        // Refused changes leave everything as it was.
        refused = Some(set_preferences(file, None, PreferenceChange { sidebar: Some(SidebarMode::Hidden), start_screen: Some("../etc".into()), ..Default::default() }));
    })
    .unwrap();
    assert!(refused.unwrap().unwrap_err().starts_with("DESKTOP_START"));
    let file = read(&dir);
    assert_eq!((file.sidebar, file.start_on, file.start_screen.as_deref()), (SidebarMode::OpenWindows, StartOn::Screen, Some("sales/invoices")));
    let text = std::fs::read_to_string(file_path(&dir)).unwrap();
    assert!(text.contains("\"sidebar\": \"open-windows\"") && text.contains("\"startOn\": \"screen\""), "{text}");

    // Other choices keep the screen chosen, for when Screen is picked again; the View mode is kept too.
    let mut file = read(&dir);
    set_preferences(&mut file, Some(KEY), PreferenceChange { sidebar: Some(SidebarMode::Hidden), start_on: Some(StartOn::NoWindows), window_mode: Some(WindowMode::One), ..Default::default() }).unwrap();
    assert_eq!((file.sidebar, file.start_on, file.start_screen.as_deref()), (SidebarMode::Hidden, StartOn::NoWindows, Some("sales/invoices")));
    assert_eq!(window_mode(&file, Some(KEY)), WindowMode::One);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_on_a_screen_needs_a_screen() {
    let mut file = DesktopFile::default();
    let err = set_preferences(&mut file, None, PreferenceChange { start_on: Some(StartOn::Screen), ..Default::default() }).unwrap_err();
    assert!(err.starts_with("DESKTOP_START"), "{err}");
    assert_eq!(file, DesktopFile::default());
    for bad in ["", "   ", "Sales/Invoices", "a b", &"x".repeat(200)] {
        assert!(set_preferences(&mut file, None, PreferenceChange { start_screen: Some(bad.to_string()), ..Default::default() }).is_err(), "{bad:?}");
    }
    // A file written before these preferences reads as the defaults.
    let old: DesktopFile = serde_json::from_str(r#"{"windowMode":"one"}"#).unwrap();
    assert_eq!((old.sidebar, old.start_on, old.start_screen), (SidebarMode::Menu, StartOn::LastWindows, None));
}
