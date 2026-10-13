//! Company file portability: online snapshots, verified restores, and automatic daily backups.
//!
//! Snapshots use SQLite's `VACUUM INTO`, which reads one consistent snapshot of the database while
//! the app keeps working and writes a compact, self-contained copy (no WAL sidecar). Every snapshot
//! is written to a temp file, integrity-checked, then renamed into place.
//!
//! A restore never trusts the incoming file: it must be a LocalBooks company file whose migrations
//! all exist in this build with matching checksums, pass `integrity_check` and `foreign_key_check`,
//! and have every posted journal entry in balance. The current books are snapshotted first, the
//! backup is copied and upgraded to the current schema off to the side, and only then swapped in.

use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::db::{self, Db, MIGRATOR};
use crate::preferences;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
/// An automatic backup is taken when the newest one is older than this.
const AUTO_BACKUP_INTERVAL: Duration = Duration::from_secs(20 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: String,
    pub file_name: String,
    /// `AUTO`, `MANUAL`, `PRE_RESTORE` or `EXPORT`
    pub kind: String,
    pub size_bytes: u64,
    pub modified: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyFileInfo {
    pub path: String,
    pub company_name: Option<String>,
    pub schema_version: i64,
    pub app_schema_version: i64,
    pub needs_upgrade: bool,
    pub journal_entries: i64,
    pub last_entry_date: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored: CompanyFileInfo,
    pub safety_backup: BackupFile,
}

fn latest_app_schema() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

fn describe(path: &Path, kind: &str) -> Result<BackupFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("BACKUP_STAT: {}: {e}", path.display()))?;
    let modified: DateTime<Local> = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH).into();
    Ok(BackupFile {
        path: path.display().to_string(),
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        kind: kind.to_string(),
        size_bytes: meta.len(),
        modified: modified.format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
}

async fn open_read_only(path: &Path) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true).create_if_missing(false);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| format!("RESTORE_UNREADABLE: cannot open {}: {e}", path.display()))
}

async fn quick_check(pool: &SqlitePool) -> Result<(), String> {
    let result: String = sqlx::query_scalar("PRAGMA quick_check").fetch_one(pool).await.map_err(|e| e.to_string())?;
    if result == "ok" { Ok(()) } else { Err(format!("integrity check reported: {result}")) }
}

/// Writes a consistent snapshot of `pool`'s database to `destination` without blocking writers.
pub async fn snapshot(pool: &SqlitePool, live_path: &Path, destination: &Path) -> Result<u64, String> {
    if destination.canonicalize().ok().is_some_and(|d| live_path.canonicalize().ok().is_some_and(|l| l == d)) {
        return Err("BACKUP_SAME_FILE: choose a location other than the live company file".into());
    }
    let dir = destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("BACKUP_DIR: cannot create {}: {e}", dir.display()))?;
    let name = destination.file_name().ok_or_else(|| "BACKUP_BAD_PATH: choose a file name".to_string())?;
    let tmp = dir.join(format!(".{}.partial-{}", name.to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    let result = write_verified_snapshot(pool, &tmp, destination).await;
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

async fn write_verified_snapshot(pool: &SqlitePool, tmp: &Path, destination: &Path) -> Result<u64, String> {
    let tmp_name: String = tmp.to_string_lossy().into_owned();
    sqlx::query("VACUUM INTO ?1").bind(tmp_name).execute(pool).await.map_err(|e| format!("BACKUP_FAILED: {e}"))?;
    let check = open_read_only(tmp).await?;
    let verified = quick_check(&check).await;
    check.close().await;
    verified.map_err(|e| format!("BACKUP_VERIFY: the snapshot failed verification ({e})"))?;
    std::fs::rename(tmp, destination).map_err(|e| format!("BACKUP_WRITE: cannot write {}: {e}", destination.display()))?;
    std::fs::metadata(destination).map(|m| m.len()).map_err(|e| e.to_string())
}

/// Validates a candidate company file and describes it. Nothing is modified.
pub async fn inspect(path: &Path) -> Result<CompanyFileInfo, String> {
    let mut header = [0u8; 16];
    {
        use std::io::Read;
        let mut file = std::fs::File::open(path).map_err(|e| format!("RESTORE_UNREADABLE: {}: {e}", path.display()))?;
        file.read_exact(&mut header).map_err(|_| "RESTORE_NOT_SQLITE: this is not a LocalBooks company file".to_string())?;
    }
    if &header != SQLITE_HEADER {
        return Err("RESTORE_NOT_SQLITE: this is not a LocalBooks company file".into());
    }

    let pool = open_read_only(path).await?;
    let result = inspect_pool(&pool, path).await;
    pool.close().await;
    result
}

async fn table_exists(pool: &SqlitePool, table: &str) -> Result<bool, String> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1")
        .bind(table)
        .fetch_one(pool)
        .await
        .map(|n| n > 0)
        .map_err(|e| e.to_string())
}

async fn inspect_pool(pool: &SqlitePool, path: &Path) -> Result<CompanyFileInfo, String> {
    quick_check(pool).await.map_err(|e| format!("RESTORE_CORRUPT: the file is damaged ({e})"))?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(pool).await.map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err(format!("RESTORE_CORRUPT: the file is damaged ({integrity})"));
    }

    for table in ["_sqlx_migrations", "chart_of_accounts", "journal_entries", "journal_lines"] {
        if !table_exists(pool, table).await? {
            return Err("RESTORE_NOT_LOCALBOOKS: this SQLite file is not a LocalBooks company file".into());
        }
    }

    let applied: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    for (version, checksum, success) in &applied {
        if !success {
            return Err(format!("RESTORE_INCOMPLETE: schema step {version} never finished in this file"));
        }
        match MIGRATOR.iter().find(|m| m.version == *version) {
            None => {
                return Err(format!(
                    "RESTORE_NEWER: this file uses schema version {version}, newer than this LocalBooks build supports ({}); update LocalBooks first",
                    latest_app_schema()
                ))
            }
            Some(m) if m.checksum.as_ref() != checksum.as_slice() => {
                return Err(format!("RESTORE_TAMPERED: schema step {version} in this file does not match LocalBooks; it may have been edited"))
            }
            Some(_) => {}
        }
    }
    let schema_version = applied.iter().map(|a| a.0).max().unwrap_or(0);

    let dangling: Option<String> = sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(table) = dangling {
        return Err(format!("RESTORE_BROKEN_LINKS: records in '{table}' point to data that no longer exists"));
    }

    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
             SELECT l.journal_entry_id FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
             WHERE e.status = 'POSTED' GROUP BY l.journal_entry_id HAVING SUM(l.debit_cents) <> SUM(l.credit_cents))",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if unbalanced > 0 {
        return Err(format!("RESTORE_UNBALANCED: {unbalanced} posted journal entr{} do not balance", if unbalanced == 1 { "y" } else { "ies" }));
    }

    let (journal_entries, last_entry_date): (i64, Option<String>) =
        sqlx::query_as("SELECT COUNT(*), MAX(entry_date) FROM journal_entries WHERE status = 'POSTED'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let company_name = if table_exists(pool, "system_preferences").await? {
        sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'company_name'")
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
    } else {
        None
    };

    Ok(CompanyFileInfo {
        path: path.display().to_string(),
        company_name,
        schema_version,
        app_schema_version: latest_app_schema(),
        needs_upgrade: schema_version < latest_app_schema(),
        journal_entries,
        last_entry_date,
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    })
}

fn stamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Replaces the live company file with a verified, upgraded copy of `source`.
pub async fn restore(db: &Db, source: &Path) -> Result<RestoreResult, String> {
    let info = inspect(source).await?;
    let dir = db.backups_dir().to_path_buf();
    let live_path = db.path().await;

    // 1. Safety net: snapshot the books being replaced.
    let safety_path = dir.join(format!("pre-restore-{}.db", stamp()));
    snapshot(&db.pool().await, &live_path, &safety_path).await?;
    let safety_backup = describe(&safety_path, "PRE_RESTORE")?;

    // 2. Copy the backup off to the side and bring it to the current schema there.
    let staged = dir.join(format!(".restore-staging-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&staged);
    let source_pool = open_read_only(source).await?;
    let copied = sqlx::query("VACUUM INTO ?1").bind(staged.to_string_lossy().into_owned()).execute(&source_pool).await;
    source_pool.close().await;
    copied.map_err(|e| format!("RESTORE_COPY: {e}"))?;
    let upgraded = db::open(&staged).await;
    match upgraded {
        Ok(pool) => pool.close().await,
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("RESTORE_UPGRADE: the backup could not be upgraded to this version ({e})"));
        }
    }

    // 3. Swap it in under the database write lock.
    db.swap_in(&staged, &safety_path).await?;
    let restored = CompanyFileInfo { path: live_path.display().to_string(), needs_upgrade: false, schema_version: latest_app_schema(), ..info };
    Ok(RestoreResult { restored, safety_backup })
}

fn kind_of(file_name: &str) -> &'static str {
    if file_name.starts_with("auto-") {
        "AUTO"
    } else if file_name.starts_with("pre-restore-") {
        "PRE_RESTORE"
    } else {
        "MANUAL"
    }
}

/// Snapshots kept in the local backups folder, newest first.
pub fn list_backups(dir: &Path) -> Result<Vec<BackupFile>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<BackupFile> = std::fs::read_dir(&dir)
        .map_err(|e| format!("BACKUP_DIR: {e}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "db") && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            describe(&p, kind_of(&name)).ok()
        })
        .collect();
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.file_name.cmp(&a.file_name)));
    Ok(files)
}

/// Takes a manual snapshot into the backups folder.
pub async fn backup_now(db: &Db) -> Result<BackupFile, String> {
    let path = db.backups_dir().join(format!("manual-{}.db", stamp()));
    snapshot(&db.pool().await, &db.path().await, &path).await?;
    describe(&path, "MANUAL")
}

/// Daily automatic backup, honouring the `auto_backup_*` preferences. Returns the new file, if any.
pub async fn auto_backup(db: &Db) -> Result<Option<BackupFile>, String> {
    let pool = db.pool().await;
    if preferences::get(&pool, "auto_backup_enabled").await? != "true" {
        return Ok(None);
    }
    let keep: usize = preferences::get(&pool, "auto_backup_keep").await?.parse().unwrap_or(7);
    let existing: Vec<BackupFile> = list_backups(db.backups_dir())?.into_iter().filter(|b| b.kind == "AUTO").collect();
    let dir = db.backups_dir();
    let newest_age = existing
        .iter()
        .filter_map(|b| std::fs::metadata(&b.path).and_then(|m| m.modified()).ok())
        .filter_map(|t| SystemTime::now().duration_since(t).ok())
        .min();
    let created = if newest_age.is_some_and(|age| age < AUTO_BACKUP_INTERVAL) {
        None
    } else {
        let path = dir.join(format!("auto-{}.db", stamp()));
        snapshot(&pool, &db.path().await, &path).await?;
        Some(describe(&path, "AUTO")?)
    };

    let mut autos: Vec<BackupFile> = list_backups(db.backups_dir())?.into_iter().filter(|b| b.kind == "AUTO").collect();
    autos.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    for old in autos.iter().skip(keep) {
        let _ = std::fs::remove_file(&old.path);
    }
    Ok(created)
}

pub fn describe_export(path: &Path) -> Result<BackupFile, String> {
    describe(path, "EXPORT")
}

#[cfg(test)]
mod tests {
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
            pool.close().await;
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
                pool.close().await;
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
}
