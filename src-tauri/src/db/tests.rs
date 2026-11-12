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
async fn the_launch_opens_off_the_ui_thread_into_the_managed_books() {
    // Gap R28: the managed books start empty and take the launch's file, and its warning, once open.
    let tmp = TempDir::new("adopt");
    let cfg = test_config(&tmp.0);
    let managed = Db::closed(cfg.clone());
    assert!(managed.status().await.is_none(), "nothing is open while the launch opens the file");
    let opened = Db::startup_with_file(cfg.clone(), Some(tmp.0.join("Gone.localbooks"))).await.unwrap();
    managed.adopt(opened).await;
    managed.refresh_gate().await;
    let status = managed.status().await.expect("the launch's file is now the managed one");
    assert!(status.is_default);
    assert!(status.startup_warning.unwrap().contains("Gone.localbooks"), "the reason the launch file didn't open carries over");
    post_one(&managed).await;
    assert_eq!(entries(&managed).await, 1, "the adopted books take writes");
    managed.shutdown().await;
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
    // Closed before it is reopened, as the app does: a pool that is only dropped closes its
    // connections in the background, and on Windows one still holding the file made the reopen
    // fail now and then, so the startup fell back to the usual company file (no name).
    let creator = Db::closed(cfg.clone());
    creator.create(&acme, "Acme", None).await.unwrap();
    assert!(creator.close().await);
    let db = Db::startup_with_file(cfg.clone(), Some(acme)).await.unwrap();
    let status = db.status().await.unwrap();
    assert_eq!(status.startup_warning, None, "the handed-over file opened");
    assert_eq!(status.company_name.as_deref(), Some("Acme"));
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
            .map(|_| {
                tokio::spawn({
                    let pool = pool.clone();
                    async move { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap() }
                })
            })
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

/// A company file's own views and triggers are not trusted to call functions with side effects.
#[tokio::test]
async fn company_files_open_with_an_untrusted_schema() {
    let tmp = TempDir::new("trusted-schema");
    let db = Db::open_at(tmp.0.join("Company.localbooks"), test_config(&tmp.0)).await.unwrap();
    let trusted: i64 = sqlx::query_scalar("PRAGMA trusted_schema").fetch_one(&db.pool().await).await.unwrap();
    assert_eq!(trusted, 0);
}

/// The launch builds the empty `Db` in Tauri's setup, on the main thread with no Tokio runtime
/// entered (gap R28). That must not panic ("this functionality requires a Tokio context").
#[test]
fn an_empty_db_can_be_made_outside_a_runtime() {
    let tmp = std::env::temp_dir().join(format!("lb-closed-{}", std::process::id()));
    let cfg = test_config(&tmp);
    let made = std::thread::spawn(move || {
        let _db = Db::closed(cfg);
    })
    .join();
    assert!(made.is_ok(), "Db::closed panicked outside a Tokio runtime");
}

/// SQLite 3.54 stopped carrying the JSON subtype out of a scalar subquery, so
/// `json_object('lines', (SELECT json_group_array(...)))` stores the array as a quoted string.
/// Every view and trigger that nests a JSON subquery wraps it in `json(...)`, which keeps the audit
/// snapshots byte-identical to the ones earlier releases wrote (gap R24).
#[tokio::test]
async fn json_subqueries_in_the_schema_stay_json() {
    let pool = test_pool().await;
    let objects: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE type IN ('view', 'trigger') AND sql IS NOT NULL").fetch_all(&pool).await.unwrap();
    let nested = regex::Regex::new(r"(?is)(json\s*\(\s*)?\(\s*SELECT\s+json_\w+").unwrap();
    let bare: Vec<&str> = objects.iter().filter(|(_, sql)| nested.captures_iter(sql).any(|c| c.get(1).is_none())).map(|(name, _)| name.as_str()).collect();
    assert!(bare.is_empty(), "wrap these JSON subqueries in json(...): {bare:?}");

    // A list change records which fields changed as an array, as before.
    let id: i64 = sqlx::query_scalar("INSERT INTO customers (name) VALUES ('Harbor Feed') RETURNING id").fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE customers SET email = 'orders@harbor.test', phone = '555-0100' WHERE id = ?1").bind(id).execute(&pool).await.unwrap();
    let changed: (String, String) = sqlx::query_as(
        "SELECT json_type(payload_snapshot, '$.changed'), json_extract(payload_snapshot, '$.changed') FROM ledger_audit_log
         WHERE action_type = 'LIST_CHANGE' AND target_entity_id = ?1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(changed, ("array".to_string(), r#"["email","phone"]"#.to_string()));
}

/// Unpacks a gzipped fixture from `src/db/fixtures` into `dir`.
fn unpack_fixture(name: &str, dir: &Path) -> PathBuf {
    use std::io::Read;
    let gz = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/db/fixtures").join(format!("{name}.gz"))).unwrap();
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(gz.as_slice()).read_to_end(&mut bytes).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A file written before migration 0177 by the SQLite before 3.54 (see `src/db/fixtures`) keeps a
/// valid hash chain and matching posting snapshots once this build opens and upgrades it, and what
/// it writes afterwards matches too. Without 0177 the same file would not verify here.
#[tokio::test]
async fn a_file_from_before_the_json_fix_still_verifies() {
    let tmp = TempDir::new("json-fix-upgrade");

    // Control: on this SQLite, the snapshot view as it was before 0177 no longer matches the file.
    let control_dir = tmp.0.join("control");
    std::fs::create_dir_all(&control_dir).unwrap();
    let control = unpack_fixture("pre-0177.localbooks", &control_dir);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(SqliteConnectOptions::new().filename(&control)).await.unwrap();
    let before = crate::ledger::verify_ledger_audit(&pool).await.unwrap();
    assert!(!before.intact, "the pre-0177 view should differ from the stored snapshots on SQLite 3.54");
    close_pool(&pool).await;

    let path = unpack_fixture("pre-0177.localbooks", &tmp.0);
    let db = Db::open_at(path, test_config(&tmp.0)).await.unwrap();
    let pool = db.pool().await;
    let newest: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations").fetch_one(&pool).await.unwrap();
    assert!(newest >= 177, "the file was upgraded through 0177");

    let check = |pool: SqlitePool| async move {
        let audit = crate::ledger::verify_ledger_audit(&pool).await.unwrap();
        assert!(audit.intact, "posting snapshots: {:?}", audit.findings);
        let chain = crate::audit_chain::verify(&pool).await.unwrap();
        assert!(chain.problems.is_empty(), "hash chain: {:?}", chain.problems);
        assert_eq!(chain.records_pending, 0);
        chain.records_sealed
    };
    let sealed = check(pool.clone()).await;
    assert!(sealed > 145, "the file's own rows and the upgrade's are all sealed: {sealed}");

    // Writing after the upgrade: a new post and a list change keep their arrays and still verify.
    let account = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (checking, sales) = (account("1010").await, account("4000").await);
    let entry = crate::ledger::JournalEntry {
        id: None,
        date: chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
        description: "After the upgrade".into(),
        lines: vec![crate::ledger::JournalLine::debit(checking, 2_500), crate::ledger::JournalLine::credit(sales, 2_500)],
        currency: None,
    };
    let posted = crate::ledger::post_journal_entry(&pool, entry).await.unwrap();
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    sqlx::query("UPDATE customers SET notes = 'Pays on the 1st' WHERE name = 'Harbor Feed'").execute(&mut *tx).await.unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT json_type(payload_snapshot, '$.lines') FROM ledger_audit_log WHERE action_type = 'POST'
         UNION ALL SELECT json_type(payload_snapshot, '$.changed') FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kinds.len(), 8, "five posts (one new) and three list changes (one new)");
    assert!(kinds.iter().all(|k| k == "array"), "{kinds:?}");
    assert!(posted > 4);
    assert!(check(pool.clone()).await > sealed);
    db.close().await;
}
