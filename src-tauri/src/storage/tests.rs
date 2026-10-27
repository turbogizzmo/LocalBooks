use super::*;

#[test]
fn launch_arguments_name_the_company_file_to_open() {
    let args = |list: &[&str]| company_file_from_args(list.iter().map(std::ffi::OsString::from));
    assert_eq!(args(&["localbooks"]), None);
    assert_eq!(args(&["/Applications/LocalBooks.localbooks", "--verbose"]), None, "the executable itself is never the file");
    assert_eq!(args(&["localbooks", "--flag", "/books/Acme Ltd.LocalBooks"]), Some(PathBuf::from("/books/Acme Ltd.LocalBooks")));
    assert_eq!(args(&["localbooks", "/books/notes.db"]), None);
    #[cfg(unix)]
    assert_eq!(args(&["localbooks", "file:///books/Acme%20Ltd.localbooks"]), Some(PathBuf::from("/books/Acme Ltd.localbooks")));
    assert!(is_company_file(Path::new("x.LOCALBOOKS")));
    assert!(!is_company_file(Path::new("localbooks")));
}

#[test]
fn classifies_network_synced_and_local_locations() {
    let k = |path: &str, fs: Option<&str>| classify_parts(path, fs);
    assert_eq!(k("/Volumes/Accounting", Some("smbfs")).kind, StorageKind::Network);
    assert_eq!(k("/mnt/books", Some("nfs")).detail, "Network drive (nfs)");
    assert_eq!(k(r"\\nas\books", None).kind, StorageKind::Network);
    assert_eq!(k(r"\\?\UNC\nas\books", None).kind, StorageKind::Network);
    assert_eq!(k(r"\\?\C:\Users\ana\Books", None).kind, StorageKind::Local, "verbatim local path");
    assert_eq!(k("/Users/ana/Library/CloudStorage/Dropbox/Books", Some("apfs")).detail, "Synced folder (Dropbox)");
    assert_eq!(k("/Users/ana/Library/CloudStorage/OneDrive-Personal/Books", Some("apfs")).detail, "Synced folder (OneDrive)");
    assert_eq!(k("/Users/ana/Library/Mobile Documents/com~apple~CloudDocs/Books", Some("apfs")).detail, "Synced folder (iCloud Drive)");
    assert_eq!(k("/Users/ana/Dropbox/Books", Some("apfs")).kind, StorageKind::Synced);
    assert_eq!(k("C:/Users/ana/OneDrive - Acme/Books", None).detail, "Synced folder (OneDrive)");
    let local = k("/Users/ana/Documents/Books", Some("apfs"));
    assert_eq!((local.kind, local.detail.as_str()), (StorageKind::Local, "Local disk (apfs)"));
    assert_eq!(k("/Users/ana/Documents/Boxing Club", Some("apfs")).kind, StorageKind::Local, "'Boxing' is not Box");
}

#[test]
fn mapped_drive_paths_split_and_join_to_unc() {
    assert_eq!(split_drive_letter(r"Z:\Accounting\localbooks.db"), Some(("Z:", r"\Accounting\localbooks.db")));
    assert_eq!(split_drive_letter("z:"), Some(("z:", "")));
    assert_eq!(split_drive_letter(r"\\nas\books"), None);
    assert_eq!(split_drive_letter("/Users/ana"), None);
    assert_eq!(split_drive_letter("Zz:/x"), None);
    assert_eq!(join_unc(r"\\nas\books", r"\2026\q1"), r"\\nas\books\2026\q1");
    assert_eq!(join_unc(r"\\nas\books\", r"\2026"), r"\\nas\books\2026");
    assert_eq!(join_unc(r"\\nas\books", "/2026/q1"), r"\\nas\books\2026\q1");
    assert_eq!(join_unc(r"\\nas\books", ""), r"\\nas\books");
    // The joined path is then recognized as a network share.
    assert_eq!(classify_parts(&join_unc(r"\\nas\books", r"\2026"), None).kind, StorageKind::Network);
    // Non-Windows paths pass through unchanged.
    assert_eq!(resolve_network_path(Path::new("/tmp/books")), PathBuf::from("/tmp/books"));
}

#[cfg(windows)]
#[test]
fn local_windows_drives_are_not_rewritten() {
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    assert_eq!(mapped_drive_remote(&system), None, "a local drive has no network connection");
    let path = PathBuf::from(format!(r"{system}\Windows"));
    assert_eq!(resolve_network_path(&path), path);
}

#[test]
fn real_temp_dir_is_local_and_writable() {
    let dir = std::env::temp_dir();
    assert_eq!(classify(&dir).kind, StorageKind::Local);
    check_writable_dir(&dir).unwrap();
    assert!(check_writable_dir(&dir.join("definitely-missing-folder-xyz")).unwrap_err().starts_with("STORAGE_MISSING"));
}

#[test]
fn lock_blocks_other_live_holders_and_releases_on_drop() {
    let dir = std::env::temp_dir().join(format!("localbooks-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join(DB_FILE_NAME);

    // Another computer holds a fresh lock: refused.
    std::fs::write(lock_path(&db), br#"{"host":"accounting-pc","pid":4242,"acquired_at":"2026-10-13 08:00"}"#).unwrap();
    let err = FileLock::acquire(&db).err().expect("held elsewhere");
    assert!(err.starts_with("STORAGE_IN_USE: this company file is open on accounting-pc"), "{err}");

    // An abandoned lock from a process that no longer exists on this host is taken over at once
    // (Unix can check the PID; elsewhere the lock is taken over once its heartbeat goes stale).
    #[cfg(unix)]
    {
        let stale = format!(r#"{{"host":"{}","pid":999999,"acquired_at":"2026-10-13 08:00"}}"#, host_name());
        std::fs::write(lock_path(&db), stale).unwrap();
    }
    #[cfg(not(unix))]
    std::fs::remove_file(lock_path(&db)).unwrap();
    let lock = FileLock::acquire(&db).expect("takes over");
    assert!(lock_path(&db).exists());
    drop(lock);
    assert!(!lock_path(&db).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn config_round_trips_and_defaults() {
    let dir = std::env::temp_dir().join(format!("localbooks-config-{}", std::process::id()));
    assert_eq!(read_config(&dir), AppConfig::default());
    let cfg = AppConfig { database_directory: Some(PathBuf::from("/Volumes/Accounting/Books")), ..AppConfig::default() };
    write_config(&dir, &cfg).unwrap();
    assert_eq!(read_config(&dir), cfg);

    // Updating one setting keeps the others.
    update_config(&dir, |c| c.start_with_no_company = true).unwrap();
    let read = read_config(&dir);
    assert!(read.start_with_no_company);
    assert_eq!(read.database_directory, cfg.database_directory);
    // A config.json from before the recent list existed still reads.
    std::fs::write(config_path(&dir), br#"{"database_directory":"/Volumes/Accounting/Books"}"#).unwrap();
    assert_eq!(read_config(&dir), cfg);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn recent(path: &str, opened: &str) -> RecentCompany {
    RecentCompany { path: PathBuf::from(path), company_name: Some(format!("Co {path}")), last_opened: opened.into(), file_size: 4096 }
}

#[test]
fn recent_list_moves_reopened_files_to_the_top_without_duplicates() {
    let mut list = Vec::new();
    add_recent(&mut list, recent("/books/a.localbooks", "2026-10-01 09:00"));
    add_recent(&mut list, recent("/books/b.localbooks", "2026-10-02 09:00"));
    add_recent(&mut list, recent("/books/c.localbooks", "2026-10-03 09:00"));
    let paths = |list: &[RecentCompany]| list.iter().map(|r| r.path.display().to_string()).collect::<Vec<_>>();
    assert_eq!(paths(&list), ["/books/c.localbooks", "/books/b.localbooks", "/books/a.localbooks"], "newest first");

    // Reopening a file moves it to the top and refreshes what is remembered, once.
    let mut renamed = recent("/books/a.localbooks", "2026-10-04 09:00");
    renamed.company_name = Some("Acme Holdings".into());
    add_recent(&mut list, renamed.clone());
    assert_eq!(paths(&list), ["/books/a.localbooks", "/books/c.localbooks", "/books/b.localbooks"]);
    assert_eq!(list[0], renamed);

    // Removing takes the entry off the list only.
    assert!(remove_recent(&mut list, Path::new("/books/c.localbooks")));
    assert!(!remove_recent(&mut list, Path::new("/books/c.localbooks")));
    assert_eq!(paths(&list), ["/books/a.localbooks", "/books/b.localbooks"]);
}

#[test]
fn recent_list_is_capped_and_dedupes_the_same_file_spelled_differently() {
    let dir = std::env::temp_dir().join(format!("localbooks-recent-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let file = dir.join("Acme.localbooks");
    std::fs::write(&file, b"x").unwrap();

    let mut list = Vec::new();
    for n in 0..MAX_RECENT_COMPANIES + 3 {
        add_recent(&mut list, recent(&format!("/books/{n}.localbooks"), "2026-10-01 09:00"));
    }
    assert_eq!(list.len(), MAX_RECENT_COMPANIES);
    assert_eq!(list[0].path, PathBuf::from(format!("/books/{}.localbooks", MAX_RECENT_COMPANIES + 2)));
    assert!(!list.iter().any(|r| r.path == Path::new("/books/0.localbooks")), "the oldest fall off");

    add_recent(&mut list, RecentCompany::opened_now(&file, None));
    add_recent(&mut list, RecentCompany::opened_now(&dir.join("sub").join("..").join("Acme.localbooks"), None));
    assert_eq!(list.iter().filter(|r| r.path.ends_with("Acme.localbooks")).count(), 1, "one entry per file");
    assert_eq!(list.len(), MAX_RECENT_COMPANIES);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn finds_company_files_in_ordinary_folders_only() {
    let root = std::env::temp_dir().join(format!("localbooks-find-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["Documents/Clients/2026", ".hidden", "node_modules/pkg", "Shared Books", "a/b/c/d"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for file in [
        "Documents/Acme.localbooks",
        "Documents/Clients/2026/Beta Ltd.LOCALBOOKS",
        "Shared Books/localbooks.db",
        "Documents/notes.db",
        ".hidden/Secret.localbooks",
        "node_modules/pkg/x.localbooks",
        "a/b/c/d/Deep.localbooks",
    ] {
        std::fs::write(root.join(file), b"x").unwrap();
    }
    let far = std::time::Instant::now() + Duration::from_secs(30);
    let names = |found: Vec<FoundCompanyFile>| {
        let mut names: Vec<String> = found.into_iter().map(|f| f.file_name).collect();
        names.sort();
        names
    };
    assert_eq!(names(find_company_files(std::slice::from_ref(&root), 3, 100, far)), ["Acme.localbooks", "Beta Ltd.LOCALBOOKS", "localbooks.db"]);
    assert_eq!(names(find_company_files(std::slice::from_ref(&root), 4, 100, far)).len(), 4, "deeper search finds a/b/c/d");
    assert_eq!(find_company_files(std::slice::from_ref(&root), 4, 2, far).len(), 2, "stops at the limit");
    assert!(find_company_files(std::slice::from_ref(&root), 4, 100, std::time::Instant::now()).is_empty(), "stops at the deadline");
    // The same folder searched twice lists each file once.
    assert_eq!(find_company_files(&[root.clone(), root.join("Documents")], 3, 100, far).len(), 3);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn recent_list_marks_missing_files_and_reports_the_current_ones() {
    let dir = std::env::temp_dir().join(format!("localbooks-recent-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let here = dir.join("Here.localbooks");
    std::fs::write(&here, vec![0u8; 1234]).unwrap();
    let gone = dir.join("Gone.localbooks");

    let list = vec![RecentCompany::opened_now(&here, None), recent(&gone.display().to_string(), "2026-09-30 17:45")];
    let views = describe_recent(&list);
    assert_eq!(views[0].company_name, "Here.localbooks", "an unnamed file is listed by its file name");
    assert_eq!(views[0].file_size, 1234);
    assert_eq!(views[0].location, dir.display().to_string());
    assert!(!views[0].missing && views[0].last_modified.is_some());

    assert!(views[1].missing, "a file that is gone stays listed, marked missing");
    assert_eq!((views[1].last_modified.as_deref(), views[1].file_size), (None, 4096), "its remembered size is kept");
    assert_eq!(views[1].last_opened, "2026-09-30 17:45");
    std::fs::remove_dir_all(&dir).unwrap();
}
