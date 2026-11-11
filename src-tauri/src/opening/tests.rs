use super::*;

#[test]
fn the_opening_screen_names_the_company_or_else_the_file() {
    assert_eq!(display_name(Path::new("/books/Acme.localbooks"), Some("Acme Plumbing".into())), "Acme Plumbing");
    assert_eq!(display_name(Path::new("/books/Acme.localbooks"), Some("  ".into())), "Acme.localbooks");
    assert_eq!(display_name(Path::new("/books/Acme.localbooks"), None), "Acme.localbooks");
    // The usual company file's name says little on its own.
    assert_eq!(display_name(Path::new("/Shared/Office/localbooks.db"), None), "localbooks.db in Office");
}

#[test]
fn the_recent_list_gives_the_company_name() {
    let dir = std::env::temp_dir().join(format!("lb-opening-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Acme.localbooks");
    crate::storage::update_config(&dir, |config| {
        crate::storage::add_recent(&mut config.recent_companies, crate::storage::RecentCompany::opened_now(&file, Some("Acme Plumbing".into())));
    })
    .unwrap();
    assert_eq!(recent_name(&dir, &file).as_deref(), Some("Acme Plumbing"));
    assert_eq!(recent_name(&dir, &dir.join("Other.localbooks")), None);
    let _ = std::fs::remove_dir_all(&dir);
}
