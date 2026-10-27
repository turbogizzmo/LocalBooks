use super::*;
use std::path::PathBuf;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};
use chrono::NaiveDate;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-{tag}-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn post_sale(pool: &SqlitePool, cents: i64) {
    let id = |n: &'static str| async move {
        sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(pool).await.unwrap()
    };
    let (checking, services) = (id("1010").await, id("4100").await);
    post_journal_entry(
        pool,
        JournalEntry {
            id: None,
            date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: "Sale".into(),
            lines: vec![JournalLine::debit(checking, cents), JournalLine::credit(services, cents)],
            currency: None,
        },
    )
    .await
    .unwrap();
}

async fn total_posted(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents), 0) FROM v_posted_lines").fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn opening_an_older_company_file_saves_a_copy_before_upgrading_it() {
    let tmp = TempDir::new("pre-upgrade");
    let live = tmp.0.join("Acme.localbooks");
    let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap();
    // Versions can skip a number while migrations are developed in parallel.
    let previous_version = MIGRATOR.iter().map(|m| m.version).filter(|v| *v < latest).max().unwrap();
    {
        // A file from the previous release: every migration but the latest, with a sale posted
        // as that release's schema allows (plain SQL, since today's posting code writes newer columns).
        let options = SqliteConnectOptions::new().filename(&live).create_if_missing(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
        let previous = sqlx::migrate::Migrator {
            migrations: MIGRATOR.iter().filter(|m| m.version < latest).cloned().collect::<Vec<_>>().into(),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        db::migrate_with(&pool, &previous).await.unwrap();
        let entry: i64 = sqlx::query_scalar(
            "INSERT INTO journal_entries (entry_number, entry_date, memo) VALUES (1, '2026-10-01', 'Sale \u{2014} \"cash\"') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents, credit_cents)
                 SELECT ?1, 1, id, 12345, 0 FROM chart_of_accounts WHERE account_number = '1010'
                 UNION ALL SELECT ?1, 2, id, 0, 12345 FROM chart_of_accounts WHERE account_number = '4100'",
        )
        .bind(entry)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE journal_entries SET status = 'POSTED', posted_at = '2026-10-01T09:00:00.000Z' WHERE id = ?1")
            .bind(entry)
            .execute(&pool)
            .await
            .unwrap();
        crate::db::close_pool(&pool).await;
    }
    let config = db::test_config(&tmp.0);
    let db = Db::open_at(live.clone(), config.clone()).await.unwrap();
    assert_eq!(total_posted(&db.pool().await).await, 12_345, "the upgraded file keeps its entries");
    let audit = crate::ledger::verify_ledger_audit(&db.pool().await).await.unwrap();
    assert!(audit.intact, "entries posted before the upgrade still match their snapshots: {:?}", audit.findings);

    let copies: Vec<BackupFile> = list_backups(&config.backups_dir).unwrap().into_iter().filter(|b| b.kind == "PRE_UPGRADE").collect();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].file_name.starts_with(&format!("pre-upgrade-Acme-v{previous_version}-")), "{}", copies[0].file_name);
    // The copy is the file as it was: still at the old schema, with the same books.
    let copy = open_read_only(Path::new(&copies[0].path)).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations").fetch_one(&copy).await.unwrap();
    assert_eq!(version, previous_version);
    assert_eq!(total_posted(&copy).await, 12_345);
    crate::db::close_pool(&copy).await;

    // An up-to-date file is opened without another copy.
    db.shutdown().await;
    let again = Db::open_at(live, config.clone()).await.unwrap();
    assert_eq!(list_backups(&config.backups_dir).unwrap().iter().filter(|b| b.kind == "PRE_UPGRADE").count(), 1);
    again.shutdown().await;
}

#[tokio::test]
async fn export_then_restore_round_trips_and_keeps_a_safety_copy() {
    let tmp = TempDir::new("backup");
    let live = tmp.0.join("live.db");
    let db = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    post_sale(&db.pool().await, 10_000).await;
    crate::preferences::set(&db.pool().await, "company_name", "Round Trip Co").await.unwrap();

    let exported = tmp.0.join("exports").join("books.lbk.db");
    snapshot(&db.pool().await, &live, &exported).await.expect("export");
    assert!(snapshot(&db.pool().await, &live, &live).await.unwrap_err().starts_with("BACKUP_SAME_FILE"));

    let info = inspect(&exported).await.expect("valid backup");
    assert_eq!((info.company_name.as_deref(), info.journal_entries, info.needs_upgrade), (Some("Round Trip Co"), 1, false));

    // Books move on after the backup...
    post_sale(&db.pool().await, 99_999).await;
    assert_eq!(total_posted(&db.pool().await).await, 109_999);

    // ...and restoring brings back exactly the exported state, keeping a safety copy of what was replaced.
    let result = restore(&db, &exported).await.expect("restores");
    assert_eq!(total_posted(&db.pool().await).await, 10_000);
    let safety = inspect(Path::new(&result.safety_backup.path)).await.unwrap();
    assert_eq!(safety.journal_entries, 2);
    assert!(list_backups(db.backups_dir()).unwrap().iter().any(|b| b.kind == "PRE_RESTORE"));

    // The restored database is fully usable.
    post_sale(&db.pool().await, 1).await;
    assert_eq!(total_posted(&db.pool().await).await, 10_001);
}

#[tokio::test]
async fn restore_rejects_foreign_newer_tampered_and_unbalanced_files() {
    let tmp = TempDir::new("reject");
    let live = tmp.0.join("live.db");
    let db = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    let reject = |path: PathBuf| {
        let db = &db;
        async move { restore(db, &path).await.unwrap_err() }
    };

    let text = tmp.0.join("notes.db");
    std::fs::write(&text, "definitely not sqlite, just some text padding").unwrap();
    assert!(reject(text).await.starts_with("RESTORE_NOT_SQLITE"));

    let other = tmp.0.join("other.db");
    {
        let pool = SqlitePoolOptions::new()
            .connect_with(SqliteConnectOptions::new().filename(&other).create_if_missing(true))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE notes (id INTEGER PRIMARY KEY)").execute(&pool).await.unwrap();
        crate::db::close_pool(&pool).await;
    }
    assert!(reject(other).await.starts_with("RESTORE_NOT_LOCALBOOKS"));

    // Build variants of a genuine backup and alter them outside the app.
    let make = |name: &str, sql: &'static str| {
        let path = tmp.0.join(name);
        let db = &db;
        let live = live.clone();
        async move {
            snapshot(&db.pool().await, &live, &path).await.unwrap();
            let pool = SqlitePoolOptions::new().connect_with(SqliteConnectOptions::new().filename(&path)).await.unwrap();
            for stmt in sql.split(';').filter(|s| !s.trim().is_empty()) {
                sqlx::query(stmt).execute(&pool).await.unwrap();
            }
            crate::db::close_pool(&pool).await;
            path
        }
    };
    post_sale(&db.pool().await, 5_000).await;
    let newer = make("newer.db", "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'future', 1, x'00', 0)").await;
    assert!(reject(newer).await.starts_with("RESTORE_NEWER"));
    let tampered = make("tampered.db", "UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 1").await;
    assert!(reject(tampered).await.starts_with("RESTORE_TAMPERED"));
    let unbalanced = make("unbalanced.db", "DROP TRIGGER trg_jl_update_draft_only; UPDATE journal_lines SET debit_cents = debit_cents + 1 WHERE debit_cents > 0").await;
    assert!(reject(unbalanced).await.starts_with("RESTORE_UNBALANCED"));

    // None of the rejected files touched the live books.
    assert_eq!(total_posted(&db.pool().await).await, 5_000);
    assert!(list_backups(db.backups_dir()).unwrap().iter().all(|b| b.kind != "PRE_RESTORE"));
}

#[tokio::test]
async fn auto_backup_runs_daily_and_prunes() {
    let tmp = TempDir::new("auto");
    let live = tmp.0.join("live.db");
    let db = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    assert!(auto_backup(&db).await.unwrap().is_some(), "first run backs up");
    assert!(auto_backup(&db).await.unwrap().is_none(), "second run the same day is skipped");

    crate::preferences::set(&db.pool().await, "auto_backup_keep", "2").await.unwrap();
    for i in 0..3 {
        std::fs::write(db.backups_dir().join(format!("auto-20000101-00000{i}.db")), b"old").unwrap();
    }
    auto_backup(&db).await.unwrap();
    assert_eq!(list_backups(db.backups_dir()).unwrap().iter().filter(|b| b.kind == "AUTO").count(), 2);

    crate::preferences::set(&db.pool().await, "auto_backup_enabled", "false").await.unwrap();
    for f in list_backups(db.backups_dir()).unwrap() {
        std::fs::remove_file(f.path).unwrap();
    }
    assert!(auto_backup(&db).await.unwrap().is_none());
}

/// Windows refuses to move or remove a file that is still open, so a handle left open at swap time
/// shows up there as an intermittent RESTORE_SWAP failure. Twenty restores in a row, each just after
/// queries on the live file, must all succeed and leave nothing behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(not(windows), ignore = "Windows stress test; run with --ignored elsewhere")]
async fn twenty_restores_in_a_row_all_swap_cleanly() {
    let tmp = TempDir::new("stress");
    let live = tmp.0.join("Company.localbooks");
    let db = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    post_sale(&db.pool().await, 10_000).await;
    let exported = tmp.0.join("exports").join("books.db");
    snapshot(&db.pool().await, &live, &exported).await.unwrap();

    let started = std::time::Instant::now();
    let mut failures = Vec::new();
    for round in 1..=20 {
        post_sale(&db.pool().await, 1).await;
        // Queries that finish just before the swap: their connections are still on their way back.
        let pool = db.pool().await;
        let reads: Vec<_> = (0..4).map(|_| tokio::spawn({ let pool = pool.clone(); async move { total_posted(&pool).await } })).collect();
        for read in reads {
            read.await.unwrap();
        }
        match restore(&db, &exported).await {
            Ok(_) => assert_eq!(total_posted(&db.pool().await).await, 10_000, "round {round}"),
            Err(e) => failures.push(format!("round {round}: {e}")),
        }
    }
    eprintln!("20 restores took {:?}", started.elapsed());
    assert!(failures.is_empty(), "{} of 20 restores failed: {failures:#?}", failures.len());
    let left: Vec<String> = [tmp.0.clone(), db.backups_dir().to_path_buf()]
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).unwrap().flatten())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".replaced-") || n.contains("staging"))
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    db.shutdown().await;
}
