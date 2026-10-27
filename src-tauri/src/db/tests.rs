use super::*;
use crate::storage::AppConfig;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-{tag}-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S%f")));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn entries(db: &Db) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&db.pool().await).await.unwrap()
}

async fn post_one(db: &Db) {
    let pool = db.pool().await;
    let id = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (checking, services) = (id("1010").await, id("4100").await);
    crate::ledger::post_journal_entry(
        &pool,
        crate::ledger::JournalEntry {
            id: None,
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: String::new(),
            lines: vec![crate::ledger::JournalLine::debit(checking, 100), crate::ledger::JournalLine::credit(services, 100)],
            currency: None,
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_posts_on_a_wal_file_all_succeed() {
    let tmp = TempDir::new("concurrent");
    let db = Db::startup(test_config(&tmp.0)).await.unwrap();
    assert_eq!(db.status().await.unwrap().journal_mode, "wal");
    let pool = db.pool().await;
    let id = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (checking, services) = (id("1010").await, id("4100").await);

    // Each post reads (closing date, accounts) before it writes; with deferred transactions some
    // of these failed with SQLITE_BUSY_SNAPSHOT instead of waiting for the write lock.
    let posts = (0..24).map(|i| {
        let pool = pool.clone();
        tokio::spawn(async move {
            crate::ledger::post_journal_entry(
                &pool,
                crate::ledger::JournalEntry {
                    id: None,
                    date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
                    description: format!("concurrent {i}"),
                    lines: vec![crate::ledger::JournalLine::debit(checking, 100 + i), crate::ledger::JournalLine::credit(services, 100 + i)],
                    currency: None,
                },
            )
            .await
        })
    });
    for post in posts.collect::<Vec<_>>() {
        post.await.unwrap().expect("every concurrent post succeeds");
    }
    assert_eq!(entries(&db).await, 24);
}

#[tokio::test]
async fn relocate_moves_opens_or_starts_new_and_persists_the_choice() {
    let tmp = TempDir::new("relocate");
    let cfg = test_config(&tmp.0);
    let db = Db::startup(cfg.clone()).await.unwrap();
    post_one(&db).await;
    assert!(db.status().await.unwrap().is_default);
    assert_eq!(db.status().await.unwrap().company_name, None, "the usual file has no name until the user sets one");
    assert_eq!(db.status().await.unwrap().journal_mode, "wal");

    // Move the current books to a new folder: same data, new location, old file untouched.
    let moved_dir = tmp.0.join("shared");
    std::fs::create_dir_all(&moved_dir).unwrap();
    let status = db.relocate(&moved_dir, RelocateMode::parse(Some("MOVE"), false).unwrap()).await.unwrap();
    assert!(!status.is_default);
    assert_eq!(status.company_name.as_deref(), Some("shared"), "a localbooks.db elsewhere is called by its folder");
    assert_eq!(entries(&db).await, 1);
    post_one(&db).await;
    assert!(cfg.default_path.exists());
    assert_eq!(storage::read_config(&cfg.config_dir).database_directory.as_deref(), Some(moved_dir.as_path()));

    // A restart honours config.json.
    db.shutdown().await;
    drop(db);
    let db = Db::startup(cfg.clone()).await.unwrap();
    assert_eq!(db.path().await.unwrap(), moved_dir.join(DB_FILE_NAME));
    assert_eq!(entries(&db).await, 2);

    // Start a new, empty company file elsewhere: migrated schema, seeded chart, no entries.
    let new_dir = tmp.0.join("second-company");
    std::fs::create_dir_all(&new_dir).unwrap();
    assert!(RelocateMode::parse(Some("OPEN"), false).is_err());
    db.relocate(&new_dir, RelocateMode::parse(None, false).unwrap()).await.unwrap();
    assert_eq!(entries(&db).await, 0);
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chart_of_accounts").fetch_one(&db.pool().await).await.unwrap();
    assert!(accounts > 20);

    // Open an existing company file (the moved one) after it passes inspection.
    assert!(RelocateMode::parse(Some("MOVE"), true).is_err());
    db.relocate(&moved_dir, RelocateMode::parse(None, true).unwrap()).await.unwrap();
    assert_eq!(entries(&db).await, 2);
    assert!(db.relocate(&moved_dir, RelocateMode::OpenExisting).await.unwrap_err().starts_with("STORAGE_UNCHANGED"));

    // A damaged file is refused and the current books stay open.
    let bad_dir = tmp.0.join("bad");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(bad_dir.join(DB_FILE_NAME), b"not a database at all, just text").unwrap();
    assert!(db.relocate(&bad_dir, RelocateMode::OpenExisting).await.unwrap_err().starts_with("RESTORE_NOT_SQLITE"));
    assert_eq!(entries(&db).await, 2);

    // Going back to the default folder clears the override.
    db.relocate(cfg.default_path.parent().unwrap(), RelocateMode::OpenExisting).await.unwrap();
    assert!(db.status().await.unwrap().is_default);
    assert_eq!(storage::read_config(&cfg.config_dir), AppConfig::default());
}

#[tokio::test]
async fn localbooks_files_open_in_place_for_the_session() {
    let tmp = TempDir::new("mount");
    let cfg = test_config(&tmp.0);

    // A company file saved elsewhere, e.g. an export the user double-clicks.
    let acme = tmp.0.join("clients").join("Acme Ltd.localbooks");
    let maker = Db::open_at(acme.clone(), cfg.clone()).await.unwrap();
    post_one(&maker).await;
    post_one(&maker).await;
    maker.shutdown().await;
    drop(maker);

    // Launched with the file: it opens in place, and the saved location is untouched.
    let db = Db::startup_with_file(cfg.clone(), Some(acme.clone())).await.unwrap();
    let status = db.status().await.unwrap();
    assert!(status.opened_from_file && !status.is_default);
    assert_eq!(status.file_name, "Acme Ltd.localbooks");
    assert_eq!(status.company_name.as_deref(), Some("Acme Ltd"), "an unnamed file is called by its file name");
    crate::preferences::set(&db.pool().await, "company_name", "Acme Holdings").await.unwrap();
    assert_eq!(db.status().await.unwrap().company_name.as_deref(), Some("Acme Holdings"), "a named company is called by its name");
    assert_eq!(entries(&db).await, 2);
    assert_eq!(storage::read_config(&cfg.config_dir), AppConfig::default());

    // Opening another file after launch (macOS hands files over this way) swaps the books.
    db.mount(&cfg.default_path).await.unwrap_err(); // the default file does not exist yet
    let beta = tmp.0.join("Beta.localbooks");
    let maker = Db::open_at(beta.clone(), cfg.clone()).await.unwrap();
    maker.shutdown().await;
    drop(maker);
    let status = db.mount(&beta).await.unwrap();
    assert_eq!(status.file_name, "Beta.localbooks");
    assert_eq!(entries(&db).await, 0);

    // A file that is not a company file is refused and the open books stay in use.
    let fake = tmp.0.join("Fake.localbooks");
    std::fs::write(&fake, b"definitely not sqlite").unwrap();
    assert!(db.mount(&fake).await.unwrap_err().starts_with("RESTORE_NOT_SQLITE"));
    assert_eq!(db.status().await.unwrap().file_name, "Beta.localbooks");
    db.shutdown().await;
    drop(db);

    // A launch file that cannot be opened falls back to the usual books and says why.
    let db = Db::startup_with_file(cfg.clone(), Some(tmp.0.join("Gone.localbooks"))).await.unwrap();
    let status = db.status().await.unwrap();
    assert!(status.is_default && !status.opened_from_file);
    let warning = status.startup_warning.unwrap();
    assert!(warning.contains("Gone.localbooks") && warning.contains("does not exist"), "{warning}");
}

#[tokio::test]
async fn new_company_files_are_created_named_and_never_replace_a_file() {
    let tmp = TempDir::new("create");
    let cfg = test_config(&tmp.0);
    let db = Db::startup(cfg.clone()).await.unwrap();
    post_one(&db).await;

    let acme = tmp.0.join("clients").join("Acme Ltd.localbooks");
    assert!(db.create(&acme, "   ", None).await.unwrap_err().starts_with("NEW_COMPANY_NAME"));
    let status = db.create(&acme, " Acme Ltd ", None).await.unwrap();
    assert_eq!((status.company_name.as_deref(), status.file_name.as_str()), (Some("Acme Ltd"), "Acme Ltd.localbooks"));
    assert!(status.opened_from_file && !status.is_default);
    assert_eq!(entries(&db).await, 0, "a new company starts empty");
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chart_of_accounts").fetch_one(&db.pool().await).await.unwrap();
    assert!(accounts > 20, "with the default chart of accounts");
    assert_eq!(storage::read_config(&cfg.config_dir), AppConfig::default(), "creating a company does not change the saved location");

    // An existing file is never replaced, and the open company stays open.
    assert!(db.create(&cfg.default_path, "Other", None).await.unwrap_err().starts_with("NEW_COMPANY_EXISTS"));
    assert_eq!(db.status().await.unwrap().file_name, "Acme Ltd.localbooks");

    // Switching back finds the first company's books intact.
    db.mount(&cfg.default_path).await.unwrap();
    assert_eq!(entries(&db).await, 1);
    db.mount(&acme).await.unwrap();
    assert_eq!(db.status().await.unwrap().company_name.as_deref(), Some("Acme Ltd"));
}

#[tokio::test]
async fn closing_releases_the_file_and_its_lock_and_refuses_company_work() {
    let tmp = TempDir::new("close");
    let cfg = test_config(&tmp.0);
    let db = Db::closed(cfg.clone());
    assert!(!db.is_open().await && db.status().await.is_none());
    assert!(!db.close().await, "nothing to close");

    // A company in a synced folder is opened single-user, with a lock file.
    let shared = tmp.0.join("Dropbox").join("Shared Co.localbooks");
    let status = db.create(&shared, "Shared Co", None).await.unwrap();
    assert_eq!(status.storage.kind, storage::StorageKind::Synced);
    assert_eq!(status.journal_mode, "delete");
    assert!(storage::lock_path(&shared).exists());
    post_one(&db).await;

    assert!(db.close().await);
    assert!(!db.is_open().await && db.status().await.is_none());
    assert!(!storage::lock_path(&shared).exists(), "closing releases the lock");
    // Nothing reaches a company file while none is open.
    assert!(db.path().await.unwrap_err().starts_with("NO_COMPANY_OPEN"));
    assert!(crate::backup::backup_now(&db).await.unwrap_err().starts_with("NO_COMPANY_OPEN"));
    assert!(sqlx::query("CREATE TABLE stray (x)").execute(&db.pool().await).await.is_err(), "the closed placeholder refuses writes");
    assert!(crate::backup::auto_backup(&db).await.unwrap().is_none());

    // The closed file opens anywhere else at once (here: another Db, as on another computer).
    let other = Db::closed(test_config(&tmp.0.join("other")));
    other.mount(&shared).await.unwrap();
    assert_eq!(entries(&other).await, 1);
    assert!(storage::lock_path(&shared).exists());
    other.close().await;
    assert!(!storage::lock_path(&shared).exists());

    // A local company file is checkpointed on close (no WAL left behind) and reopens.
    let local = tmp.0.join("Local Co.localbooks");
    db.create(&local, "Local Co", None).await.unwrap();
    post_one(&db).await;
    db.close().await;
    assert!(!PathBuf::from(format!("{}-wal", local.display())).exists());
    db.mount(&local).await.unwrap();
    assert_eq!(entries(&db).await, 1);
}

#[tokio::test]
async fn a_company_closed_at_quit_stays_closed_at_the_next_launch() {
    let tmp = TempDir::new("start-closed");
    let cfg = test_config(&tmp.0);
    storage::update_config(&cfg.config_dir, |c| c.start_with_no_company = true).unwrap();
    let db = Db::startup_with_file(cfg.clone(), None).await.unwrap();
    assert!(!db.is_open().await);
    assert!(!cfg.default_path.exists(), "no company file is created");

    // A file the operating system hands over still opens.
    let acme = tmp.0.join("Acme.localbooks");
    Db::closed(cfg.clone()).create(&acme, "Acme", None).await.unwrap();
    let db = Db::startup_with_file(cfg.clone(), Some(acme)).await.unwrap();
    assert_eq!(db.status().await.unwrap().company_name.as_deref(), Some("Acme"));
}

#[tokio::test]
async fn startup_falls_back_when_the_configured_folder_is_missing() {
    let tmp = TempDir::new("fallback");
    let cfg = test_config(&tmp.0);
    storage::write_config(&cfg.config_dir, &AppConfig { database_directory: Some(tmp.0.join("unplugged-nas")), ..AppConfig::default() }).unwrap();
    let db = Db::startup(cfg.clone()).await.unwrap();
    let status = db.status().await.unwrap();
    assert!(status.is_default);
    assert!(status.startup_warning.unwrap().contains("is not available"));
    assert!(!tmp.0.join("unplugged-nas").exists(), "never creates an empty company file on a missing drive");
}

/// `Pool::close` alone can return with a connection still open (see `close_pool`), which on Windows
/// keeps the file from being moved or removed. Queries finishing just before the close make it likely.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_pool_closes_every_connection() {
    let tmp = TempDir::new("close-pool");
    let path = tmp.0.join("Close.localbooks");
    for round in 0..30 {
        let pool = open(&path).await.unwrap();
        let reads: Vec<_> = (0..4)
            .map(|_| tokio::spawn({ let pool = pool.clone(); async move { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap() } }))
            .collect();
        for read in reads {
            read.await.unwrap();
        }
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await.unwrap();
        close_pool(&pool).await;
        assert_eq!(pool.size(), 0, "round {round}: a connection was left open");
        // Nothing else has the file, so it can leave WAL mode (which removes its -wal and -shm).
        leave_wal(&path).await.unwrap_or_else(|e| panic!("round {round}: {e}"));
        assert!(!PathBuf::from(format!("{}-wal", path.display())).exists());
    }
}

/// A swap that cannot happen leaves the books exactly as they were, and open.
#[tokio::test]
async fn a_failed_swap_leaves_the_books_in_place() {
    let tmp = TempDir::new("swap-fail");
    let db = Db::open_at(tmp.0.join("Company.localbooks"), test_config(&tmp.0)).await.unwrap();
    post_one(&db).await;
    let missing = tmp.0.join("no-such-file.db");
    let err = db.swap_in(&missing, &missing).await.unwrap_err();
    assert!(err.starts_with("RESTORE_SWAP") && err.contains("left in place"), "{err}");
    assert_eq!(entries(&db).await, 1);
    post_one(&db).await;
    assert_eq!(entries(&db).await, 2);
    let names: Vec<String> = std::fs::read_dir(&tmp.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!names.iter().any(|n| n.contains(".replaced-")), "{names:?}");
}

/// A replaced file that could not be removed at the time (a scanner had it open) goes the next time
/// the company file opens.
#[tokio::test]
async fn replaced_copies_left_behind_are_removed_at_the_next_open() {
    let tmp = TempDir::new("replaced");
    let path = tmp.0.join("Company.localbooks");
    let stale = PathBuf::from(format!("{}{REPLACED_INFIX}1-2", path.display()));
    let unrelated = tmp.0.join("Other.localbooks.replaced-1-2");
    std::fs::write(&stale, b"old").unwrap();
    std::fs::write(&unrelated, b"keep").unwrap();
    let db = Db::open_at(path, test_config(&tmp.0)).await.unwrap();
    assert!(!stale.exists());
    assert!(unrelated.exists());
    db.shutdown().await;
}
