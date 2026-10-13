use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteLockingMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, SqlitePool};

use crate::storage::{self, AppConfig, FileLock, StorageInfo, DB_FILE_NAME};

/// Fixed locations the app needs besides the company file itself.
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Holds `config.json`.
    pub config_dir: PathBuf,
    /// The company file used when no override is configured.
    pub default_path: PathBuf,
    /// Automatic and pre-restore snapshots always stay on this local folder, never on a share.
    pub backups_dir: PathBuf,
}

struct Live {
    path: PathBuf,
    pool: SqlitePool,
    storage: StorageInfo,
    lock: Option<FileLock>,
}

/// The live company file. Commands borrow a pool clone per call; a restore or relocation swaps the
/// pool under the write lock, so no command ever sees a half-replaced database.
pub struct Db {
    live: tokio::sync::RwLock<Live>,
    config: DbConfig,
    startup_warning: std::sync::Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub path: String,
    pub directory: String,
    pub storage: StorageInfo,
    pub is_default: bool,
    pub journal_mode: String,
    /// Set when the configured location could not be opened at startup and the default was used.
    pub startup_warning: Option<String>,
}

/// How to treat a folder that has no company file yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocateMode {
    /// Use the company file that already exists in the folder.
    OpenExisting,
    /// Copy the current books into the folder and switch to the copy.
    MoveCurrent,
    /// Create a new, empty company file in the folder.
    StartNew,
}

impl RelocateMode {
    pub fn parse(mode: Option<&str>, target_exists: bool) -> Result<Self, String> {
        match (mode, target_exists) {
            (None | Some("OPEN"), true) => Ok(Self::OpenExisting),
            (None | Some("NEW"), false) => Ok(Self::StartNew),
            (Some("MOVE"), false) => Ok(Self::MoveCurrent),
            (Some("MOVE" | "NEW"), true) => Err("STORAGE_EXISTS: that folder already has a company file; open it instead".into()),
            (Some("OPEN"), false) => Err("STORAGE_NO_FILE: there is no company file in that folder to open".into()),
            (Some(other), _) => Err(format!("STORAGE_BAD_MODE: unknown mode '{other}'")),
        }
    }
}

async fn open_live(path: &Path) -> Result<Live, String> {
    // A mapped network drive (Z:\...) is opened through its UNC path so it is recognized as shared
    // and keeps working if the drive letter changes.
    let path = &storage::resolve_network_path(path);
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data directory {}: {e}", dir.display()))?;
    let storage = storage::classify(dir);
    let lock = if storage.is_shared() { Some(FileLock::acquire(path)?) } else { None };
    let pool = open_for(path, &storage).await?;
    Ok(Live { path: path.to_path_buf(), pool, storage, lock })
}

impl Db {
    /// Opens the company file at `path` (creating and migrating it if new).
    pub async fn open_at(path: PathBuf, config: DbConfig) -> Result<Db, String> {
        let live = open_live(&path).await?;
        Ok(Db { live: tokio::sync::RwLock::new(live), config, startup_warning: std::sync::Mutex::new(None) })
    }

    /// Startup: honour `config.json` when its folder and company file are available; otherwise open
    /// the default local company file and remember why, so the UI can say so.
    pub async fn startup(config: DbConfig) -> Result<Db, String> {
        let warning = match storage::read_config(&config.config_dir).database_directory {
            None => None,
            Some(dir) => {
                let dir = storage::resolve_network_path(&dir);
                let target = dir.join(DB_FILE_NAME);
                if !dir.is_dir() {
                    Some(format!("The company file location {} is not available (is the drive connected?). Showing the local company file instead.", dir.display()))
                } else if !target.is_file() {
                    Some(format!("No company file was found in {}. Showing the local company file instead.", dir.display()))
                } else {
                    match Db::open_at(target, config.clone()).await {
                        Ok(db) => return Ok(db),
                        Err(err) => Some(format!("Could not open the company file in {}: {err}. Showing the local company file instead.", dir.display())),
                    }
                }
            }
        };
        let db = Db::open_at(config.default_path.clone(), config).await?;
        *db.startup_warning.lock().expect("warning mutex") = warning;
        Ok(db)
    }

    pub async fn pool(&self) -> SqlitePool {
        self.live.read().await.pool.clone()
    }

    pub async fn path(&self) -> PathBuf {
        self.live.read().await.path.clone()
    }

    pub fn backups_dir(&self) -> &Path {
        &self.config.backups_dir
    }

    pub async fn status(&self) -> StorageStatus {
        let live = self.live.read().await;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&live.pool).await.unwrap_or_else(|_| "unknown".into());
        StorageStatus {
            path: live.path.display().to_string(),
            directory: live.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            storage: live.storage.clone(),
            is_default: live.path == self.config.default_path,
            journal_mode,
            startup_warning: self.startup_warning.lock().expect("warning mutex").clone(),
        }
    }

    /// Replaces the live database file with `staged` (a migrated, closed SQLite file) and reopens it.
    /// If the replacement cannot be opened, `fallback` (a snapshot of the previous file) is put back.
    pub async fn swap_in(&self, staged: &Path, fallback: &Path) -> Result<(), String> {
        let mut live = self.live.write().await;
        live.pool.close().await;
        let path = live.path.clone();
        let storage = live.storage.clone();
        let remove_sidecars = |path: &Path| {
            for suffix in ["-wal", "-shm", "-journal"] {
                let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
                if sidecar.exists() {
                    std::fs::remove_file(&sidecar).map_err(|e| format!("cannot remove {}: {e}", sidecar.display()))?;
                }
            }
            Ok::<(), String>(())
        };
        let swapped = remove_sidecars(&path)
            .and_then(|()| std::fs::rename(staged, &path).map_err(|e| format!("cannot replace the company file: {e}")));
        match open_for(&path, &storage).await {
            Ok(pool) => {
                live.pool = pool;
                swapped.map_err(|e| format!("RESTORE_SWAP: {e}; your current books were left in place"))
            }
            Err(open_err) => {
                std::fs::copy(fallback, &path).map_err(|e| format!("RESTORE_FAILED: {open_err}; putting the previous books back also failed: {e}"))?;
                remove_sidecars(&path)?;
                live.pool = open_for(&path, &storage).await.map_err(|e| format!("RESTORE_FAILED: {open_err}; reopening the previous books failed: {e}"))?;
                Err(format!("RESTORE_FAILED: {open_err}; your previous books were put back"))
            }
        }
    }

    /// Switches the live company file to `new_dir/localbooks.db` and records it in `config.json`.
    /// An existing file there must pass the same checks as a restore; the new file is fully opened
    /// before the old one is closed, so a failure leaves the current books in use.
    pub async fn relocate(&self, new_dir: &Path, mode: RelocateMode) -> Result<StorageStatus, String> {
        let resolved = storage::resolve_network_path(new_dir);
        let new_dir = resolved.as_path();
        storage::check_writable_dir(new_dir)?;
        let target = new_dir.join(DB_FILE_NAME);
        let current = self.path().await;
        let same = |a: &Path, b: &Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y);
        if same(&target, &current) || (target == current) {
            return Err("STORAGE_UNCHANGED: LocalBooks is already using the company file in that folder".into());
        }

        match mode {
            RelocateMode::OpenExisting => {
                crate::backup::inspect(&target).await.map_err(|e| format!("{e} (the company file there was not opened)"))?;
            }
            RelocateMode::MoveCurrent => {
                crate::backup::snapshot(&self.pool().await, &current, &target).await?;
                crate::backup::inspect(&target).await?;
            }
            RelocateMode::StartNew => {}
        }

        let next = open_live(&target).await?;
        {
            let mut live = self.live.write().await;
            live.pool.close().await;
            *live = next;
        }
        let config = if target == self.config.default_path {
            AppConfig::default()
        } else {
            AppConfig { database_directory: Some(new_dir.to_path_buf()) }
        };
        storage::write_config(&self.config.config_dir, &config)?;
        *self.startup_warning.lock().expect("warning mutex") = None;
        Ok(self.status().await)
    }

    /// Closes the pool (checkpointing the journal) and releases any shared-location lock.
    pub async fn shutdown(&self) {
        let mut live = self.live.write().await;
        live.pool.close().await;
        live.lock = None;
    }
}

/// Embedded schema migrations from `src-tauri/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens (creating if needed) the company file at `path` on local disk and migrates it.
///
/// WAL + `synchronous = FULL` gives full durability: a committed entry survives power loss.
pub async fn open(path: &Path) -> Result<SqlitePool, String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    open_for(path, &storage::classify(dir)).await
}

/// Opens with settings suited to where the file lives. Shared locations (network or synced folders)
/// cannot use WAL, so they use a rollback journal with exclusive locking on a single connection.
async fn open_for(path: &Path, storage: &StorageInfo) -> Result<SqlitePool, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data directory {}: {e}", dir.display()))?;
    }

    let base = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5));
    let (options, connections) = if storage.is_shared() {
        (base.journal_mode(SqliteJournalMode::Delete).locking_mode(SqliteLockingMode::Exclusive), 1)
    } else {
        (base.journal_mode(SqliteJournalMode::Wal), 4)
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(connections)
        .connect_with(options)
        .await
        .map_err(|e| format!("cannot open company file {}: {e}", path.display()))?;

    migrate(&pool).await.map_err(|e| format!("cannot migrate company file {}: {e}", path.display()))?;

    Ok(pool)
}

/// Applies pending migrations with foreign-key enforcement off, as SQLite's table-rebuild procedure
/// requires (sqlx wraps each SQLite migration in a transaction, where the pragma cannot change).
/// The whole database is then checked for dangling references before enforcement is restored.
pub async fn migrate(pool: &SqlitePool) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await.map_err(|e| e.to_string())?;
    // `run_direct` takes the connection itself rather than an `Acquire` impl, which keeps this
    // future provably `Send` when a restore runs it inside a Tauri command.
    let migrated = MIGRATOR.run_direct(&mut *conn).await.map_err(|e| e.to_string());
    let violations = sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string());
    let restored = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await.map_err(|e| e.to_string());
    if restored.is_err() {
        // Never hand a connection without enforcement back to the pool.
        let _ = conn.detach().close().await;
    }
    migrated?;
    restored?;
    if violations?.is_some() {
        return Err("foreign key check failed after migration".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn test_pool() -> SqlitePool {
    use std::str::FromStr;

    // One connection: every connection to `sqlite::memory:` would otherwise be its own database.
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("valid sqlite url")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open in-memory database");
    migrate(&pool).await.expect("apply migrations");
    pool
}

#[cfg(test)]
pub(crate) fn test_config(root: &Path) -> DbConfig {
    DbConfig {
        config_dir: root.join("config"),
        default_path: root.join("default").join(DB_FILE_NAME),
        backups_dir: root.join("backups"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn relocate_moves_opens_or_starts_new_and_persists_the_choice() {
        let tmp = TempDir::new("relocate");
        let cfg = test_config(&tmp.0);
        let db = Db::startup(cfg.clone()).await.unwrap();
        post_one(&db).await;
        assert!(db.status().await.is_default);
        assert_eq!(db.status().await.journal_mode, "wal");

        // Move the current books to a new folder: same data, new location, old file untouched.
        let moved_dir = tmp.0.join("shared");
        std::fs::create_dir_all(&moved_dir).unwrap();
        let status = db.relocate(&moved_dir, RelocateMode::parse(Some("MOVE"), false).unwrap()).await.unwrap();
        assert!(!status.is_default);
        assert_eq!(entries(&db).await, 1);
        post_one(&db).await;
        assert!(cfg.default_path.exists());
        assert_eq!(storage::read_config(&cfg.config_dir).database_directory.as_deref(), Some(moved_dir.as_path()));

        // A restart honours config.json.
        db.shutdown().await;
        drop(db);
        let db = Db::startup(cfg.clone()).await.unwrap();
        assert_eq!(db.path().await, moved_dir.join(DB_FILE_NAME));
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
        assert!(db.status().await.is_default);
        assert_eq!(storage::read_config(&cfg.config_dir), AppConfig::default());
    }

    #[tokio::test]
    async fn startup_falls_back_when_the_configured_folder_is_missing() {
        let tmp = TempDir::new("fallback");
        let cfg = test_config(&tmp.0);
        storage::write_config(&cfg.config_dir, &AppConfig { database_directory: Some(tmp.0.join("unplugged-nas")) }).unwrap();
        let db = Db::startup(cfg.clone()).await.unwrap();
        let status = db.status().await;
        assert!(status.is_default);
        assert!(status.startup_warning.unwrap().contains("is not available"));
        assert!(!tmp.0.join("unplugged-nas").exists(), "never creates an empty company file on a missing drive");
    }
}
