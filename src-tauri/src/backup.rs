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

use std::path::{Path, PathBuf};
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
    /// `AUTO`, `MANUAL`, `PRE_RESTORE`, `PRE_UPGRADE`, `PRE_IMPORT`, `PRE_REBUILD`, `PRE_CONDENSE`,
    /// `ARCHIVE` (the read-only full history a condense leaves) or `EXPORT`
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
    let live_path = db.path().await?;

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
    } else if file_name.starts_with("pre-upgrade-") {
        "PRE_UPGRADE"
    } else if file_name.starts_with("pre-import-") {
        "PRE_IMPORT"
    } else if file_name.starts_with("pre-rebuild-") {
        "PRE_REBUILD"
    } else if file_name.starts_with("pre-condense-") {
        "PRE_CONDENSE"
    } else if file_name.starts_with("archive-") {
        "ARCHIVE"
    } else {
        "MANUAL"
    }
}

/// Saves a verified copy of the live books just before an import posts to them:
/// `pre-import-<batch>-<timestamp>.db`. These are never pruned automatically: "Start over" on
/// the import restores it.
pub async fn pre_import_snapshot(db: &Db, batch_id: i64) -> Result<PathBuf, String> {
    let path = db.backups_dir().join(format!("pre-import-{batch_id}-{}.db", stamp()));
    snapshot(&db.pool().await, &db.path().await?, &path).await?;
    Ok(path)
}

/// Saves a verified copy of the live books before File Dentist rebuilds them:
/// `pre-rebuild-<timestamp>.db`.
pub async fn pre_rebuild_snapshot(db: &Db) -> Result<BackupFile, String> {
    let path = db.backups_dir().join(format!("pre-rebuild-{}.db", stamp()));
    snapshot(&db.pool().await, &db.path().await?, &path).await?;
    describe(&path, "PRE_REBUILD")
}

/// Copies of a company file taken before a newer LocalBooks upgraded it; the newest few are kept.
const PRE_UPGRADE_KEEP: usize = 5;

/// Saves a verified copy of `live_path` (still at schema `version`) before its schema is upgraded:
/// `pre-upgrade-<file>-v<version>-<timestamp>.db` in `dir`. Older pre-upgrade copies beyond the
/// newest few are removed.
pub async fn pre_upgrade_snapshot(pool: &SqlitePool, live_path: &Path, dir: &Path, version: i64) -> Result<u64, String> {
    let stem = live_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "company".into());
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let destination = dir.join(format!("pre-upgrade-{stem}-v{version}-{stamp}.db"));
    let bytes = snapshot(pool, live_path, &destination).await?;
    let mut older: Vec<BackupFile> = list_backups(dir)?.into_iter().filter(|b| b.kind == "PRE_UPGRADE").collect();
    if older.len() > PRE_UPGRADE_KEEP {
        for stale in older.split_off(PRE_UPGRADE_KEEP) {
            let _ = std::fs::remove_file(&stale.path);
        }
    }
    Ok(bytes)
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
    snapshot(&db.pool().await, &db.path().await?, &path).await?;
    describe(&path, "MANUAL")
}

/// Daily automatic backup, honouring the `auto_backup_*` preferences. Returns the new file, if any.
pub async fn auto_backup(db: &Db) -> Result<Option<BackupFile>, String> {
    if !db.is_open().await {
        return Ok(None);
    }
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
        snapshot(&pool, &db.path().await?, &path).await?;
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

pub fn describe_kind(path: &Path, kind: &str) -> Result<BackupFile, String> {
    describe(path, kind)
}

#[cfg(test)]
mod tests;
