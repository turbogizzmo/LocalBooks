use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteLockingMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, SqlitePool};

use crate::storage::{self, FileLock, StorageInfo, DB_FILE_NAME};

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
    /// Opened directly (a double-clicked `.localbooks` file or File › Open) rather than from the
    /// configured location. Lasts for this session; `config.json` is not changed.
    opened_from_file: bool,
    /// Who is signed in to this file (when it has users). A new or restored file starts signed out.
    session: SessionSlot,
    /// A remembered sign-in is tried once per opening, and never after someone signs out or the
    /// idle limit signs them out.
    remembered_tried: std::sync::atomic::AtomicBool,
}

/// When the signed-in user last did something, and how long they may be idle. Wall-clock time, so
/// time asleep counts.
#[derive(Debug)]
struct Idle {
    last: std::time::SystemTime,
    limit: Option<Duration>,
}

impl Idle {
    fn is_over(&self) -> bool {
        self.limit.is_some_and(|limit| self.last.elapsed().unwrap_or_default() >= limit)
    }
}

/// The signed-in user of the open company file, shared by the commands of one process.
pub type SessionSlot = std::sync::Arc<tokio::sync::Mutex<Option<crate::users::Session>>>;

/// The live company file. Commands borrow a pool clone per call; a restore or relocation swaps the
/// pool under the write lock, so no command ever sees a half-replaced database.
///
/// After File › Close Company no company file is open (`live` is `None`). Commands then get
/// `closed`, an empty in-memory database that refuses writes, so a stray request fails ("no such
/// table") instead of reading or writing some other company's books.
pub struct Db {
    live: tokio::sync::RwLock<Option<Live>>,
    closed: SqlitePool,
    config: DbConfig,
    startup_warning: std::sync::Mutex<Option<String>>,
    /// Who may do what right now, read without waiting by the check in front of every command.
    gate: std::sync::RwLock<crate::permissions::Gate>,
    idle: std::sync::Mutex<Idle>,
    /// One refresh at a time, so a slower one can never write an older picture over a newer one.
    refreshing: tokio::sync::Mutex<()>,
    /// The open file is an archive left by condensing (see `condense`): it opens read-only.
    archive: std::sync::atomic::AtomicBool,
}

/// Returned by operations that need an open company file while none is.
pub const NO_COMPANY_OPEN: &str = "NO_COMPANY_OPEN: no company file is open. Open or create one from the File menu.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    /// What to call the open company in the window title and header (see [`company_display_name`]).
    pub company_name: Option<String>,
    pub path: String,
    pub file_name: String,
    pub opened_from_file: bool,
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

/// `backups`: where to save a copy of an existing file before upgrading its schema (see `open_for`).
async fn open_live(path: &Path, backups: Option<&Path>) -> Result<Live, String> {
    // A mapped network drive (Z:\...) is opened through its UNC path so it is recognized as shared
    // and keeps working if the drive letter changes.
    let path = &storage::resolve_network_path(path);
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data directory {}: {e}", dir.display()))?;
    let storage = storage::classify(dir);
    let lock = if storage.is_shared() { Some(FileLock::acquire(path)?) } else { None };
    remove_stale_replaced(path);
    let pool = open_for(path, &storage, backups).await?;
    Ok(Live { path: path.to_path_buf(), pool, storage, lock, opened_from_file: false, session: SessionSlot::default(), remembered_tried: Default::default() })
}

impl Db {
    fn with(live: Option<Live>, config: DbConfig) -> Db {
        let closed = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(SqliteConnectOptions::new().filename(":memory:").pragma("query_only", "ON"));
        Db {
            live: tokio::sync::RwLock::new(live),
            closed,
            config,
            startup_warning: std::sync::Mutex::new(None),
            gate: std::sync::RwLock::new(crate::permissions::Gate::Unchecked),
            idle: std::sync::Mutex::new(Idle { last: std::time::SystemTime::now(), limit: None }),
            refreshing: tokio::sync::Mutex::new(()),
            archive: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Opens the company file at `path` (creating and migrating it if new).
    pub async fn open_at(path: PathBuf, config: DbConfig) -> Result<Db, String> {
        let live = open_live(&path, Some(&config.backups_dir)).await?;
        Ok(Db::with(Some(live), config))
    }

    /// No company file open: LocalBooks starts on the No Company Open screen.
    pub fn closed(config: DbConfig) -> Db {
        Db::with(None, config)
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

    /// Startup when the operating system launched LocalBooks to open `file`: that file is opened in
    /// place. If it cannot be (missing, not a company file, damaged), the usual company file opens
    /// instead and the reason is shown.
    ///
    /// Without a file, a company that was closed when LocalBooks last quit stays closed.
    pub async fn startup_with_file(config: DbConfig, file: Option<PathBuf>) -> Result<Db, String> {
        let Some(file) = file else {
            if storage::read_config(&config.config_dir).start_with_no_company {
                return Ok(Db::closed(config));
            }
            return Db::startup(config).await;
        };
        match open_company_file(&file, &config.backups_dir).await {
            Ok(live) => Ok(Db::with(Some(live), config)),
            Err(err) => {
                let db = Db::startup(config).await?;
                let mut warning = db.startup_warning.lock().expect("warning mutex");
                let reason = format!("Could not open {}: {err}.", file.display());
                *warning = Some(match warning.take() {
                    Some(previous) => format!("{reason} {previous}"),
                    None => format!("{reason} Showing your usual company file instead."),
                });
                drop(warning);
                Ok(db)
            }
        }
    }

    /// Switches the live company file to `path` in place, for this session. The file must pass the
    /// same checks as a restore; it is fully opened before the current file is closed, so a failure
    /// leaves the current books in use.
    pub async fn mount(&self, path: &Path) -> Result<StorageStatus, String> {
        if let Ok(current) = self.path().await {
            let same = path.canonicalize().ok().zip(current.canonicalize().ok()).is_some_and(|(a, b)| a == b);
            if same {
                return self.require_status().await;
            }
        }
        let next = open_company_file(path, &self.config.backups_dir).await?;
        self.replace_live(next).await
    }

    /// Makes `next` the live company file, closing the current one (if any) and its lock.
    async fn replace_live(&self, next: Live) -> Result<StorageStatus, String> {
        {
            let mut live = self.live.write().await;
            if let Some(current) = live.take() {
                close_pool(&current.pool).await;
            }
            *live = Some(next);
        }
        self.refresh_gate().await;
        *self.startup_warning.lock().expect("warning mutex") = None;
        self.require_status().await
    }

    /// File › New Company: creates an empty company file at `path` (schema applied, default chart
    /// of accounts, or the chart and settings from the setup assistant), names it `company_name`,
    /// and makes it the live company file for this session. An existing file at `path` is never
    /// replaced.
    pub async fn create(&self, path: &Path, company_name: &str, setup: Option<&crate::company_setup::CompanySetup>) -> Result<StorageStatus, String> {
        let name = company_name.trim();
        if name.is_empty() {
            return Err("NEW_COMPANY_NAME: enter a name for the new company".into());
        }
        let path = storage::resolve_network_path(path);
        if path.exists() {
            return Err(format!("NEW_COMPANY_EXISTS: {} already exists; choose another name or folder (LocalBooks never replaces a company file)", path.display()));
        }
        let mut next = open_live(&path, None).await?;
        let prepared = async {
            crate::preferences::set(&next.pool, "company_name", name).await?;
            match setup {
                Some(setup) => crate::company_setup::apply(&next.pool, setup).await,
                None => Ok(()),
            }
        }
        .await;
        if let Err(err) = prepared {
            close_pool(&next.pool).await;
            drop(next);
            remove_with_sidecars(&path);
            return Err(err);
        }
        next.opened_from_file = true;
        self.replace_live(next).await
    }

    /// File › Close Company: closes the company file (checkpointing it) and releases its lock, so
    /// it can be opened elsewhere. Whether a company was open.
    pub async fn close(&self) -> bool {
        let closed = self.live.write().await.take();
        self.refresh_gate().await;
        *self.startup_warning.lock().expect("warning mutex") = None;
        match closed {
            Some(live) => {
                close_pool(&live.pool).await;
                let path = live.path.clone();
                drop(live); // releases the shared-location lock, if any
                checkpoint_closed(&path).await;
                true
            }
            None => false,
        }
    }

    pub async fn is_open(&self) -> bool {
        self.live.read().await.is_some()
    }

    /// The folder holding `config.json`.
    pub fn config_dir(&self) -> &Path {
        &self.config.config_dir
    }

    /// Records a problem to show in the app's banner (used when a file handed over by the operating
    /// system after launch cannot be opened).
    pub fn set_warning(&self, warning: String) {
        *self.startup_warning.lock().expect("warning mutex") = Some(warning);
    }

    pub async fn pool(&self) -> SqlitePool {
        match self.live.read().await.as_ref() {
            Some(live) => live.pool.clone(),
            None => self.closed.clone(),
        }
    }

    /// Recomputes who may do what from the open file and its sign-in. Runs whenever either changes;
    /// also records the signed-in user as the audit trail's actor.
    pub async fn refresh_gate(&self) {
        use crate::permissions::{Gate, SignedIn};
        let _one_at_a_time = self.refreshing.lock().await;
        let mut limit = None;
        let mut archive = false;
        let (gate, actor) = match self.live.read().await.as_ref() {
            None => (Gate::NoCompany, None),
            Some(live) => {
                archive = crate::condense::pool_is_archive(&live.pool).await;
                // An unreadable users table asks for a sign-in rather than opening the books.
                if crate::users::users_enabled(&live.pool).await.unwrap_or(true) {
                    let minutes = crate::users::auto_lock_minutes(&live.pool).await;
                    limit = (minutes > 0).then(|| Duration::from_secs(60 * u64::from(minutes)));
                    let session = live.session.lock().await;
                    let user = session.as_ref().map(|s| &s.user);
                    (
                        Gate::Users(user.map(|u| SignedIn { role: u.role, must_change_password: u.must_change_password })),
                        user.map(|u| u.username.clone()),
                    )
                } else {
                    (Gate::NoUsers, None)
                }
            }
        };
        crate::audit_chain::set_signed_in(actor);
        self.archive.store(archive, std::sync::atomic::Ordering::SeqCst);
        *self.idle.lock().expect("idle lock") = Idle { last: std::time::SystemTime::now(), limit };
        *self.gate.write().expect("gate lock") = gate;
    }

    /// The open file's pool for a reader outside the commands (the MCP server): none (the empty,
    /// closed database) while the file has users and nobody is signed in.
    pub async fn readable_pool(&self) -> SqlitePool {
        match self.check(crate::permissions::Need::Read, false) {
            Ok(()) => self.pool().await,
            Err(_) => self.closed.clone(),
        }
    }

    /// Whether the signed-in user (if any) may run a command needing `need`.
    /// Past the idle limit, every command but the [`Need::Open`](crate::permissions::Need::Open)
    /// ones is refused with [`IDLE_SIGNED_OUT`](crate::permissions::IDLE_SIGNED_OUT). `activity`:
    /// the command shows someone at work (a change, or a window reporting typing and clicks). Reads
    /// do not count, since windows reload what another window changed without anyone touching them.
    pub fn check(&self, need: crate::permissions::Need, activity: bool) -> Result<(), String> {
        use crate::permissions::{Gate, Need, IDLE_SIGNED_OUT};
        let gate = self.gate.read().expect("gate lock");
        if need != Need::Open && matches!(*gate, Gate::Users(Some(_))) {
            let mut idle = self.idle.lock().expect("idle lock");
            if idle.is_over() {
                return Err(IDLE_SIGNED_OUT.into());
            }
            if activity {
                idle.last = std::time::SystemTime::now();
            }
        }
        crate::permissions::check(&gate, need)?;
        if matches!(need, Need::Write | Need::Admin) && self.archive.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(crate::condense::ARCHIVE_READ_ONLY.into());
        }
        Ok(())
    }

    /// Whether someone is signed in and has been idle past the limit.
    pub fn idle_expired(&self) -> bool {
        matches!(*self.gate.read().expect("gate lock"), crate::permissions::Gate::Users(Some(_))) && self.idle.lock().expect("idle lock").is_over()
    }

    /// Tests: pretend the signed-in user has been idle for `by`.
    #[cfg(test)]
    pub fn backdate_activity(&self, by: Duration) {
        let mut idle = self.idle.lock().expect("idle lock");
        idle.last = std::time::SystemTime::now() - by;
    }

    /// Whether a remembered sign-in may be tried now (the first time since the file was opened);
    /// later calls, and calls after [`Db::forbid_remembered_sign_in`], answer false.
    pub async fn take_remembered_attempt(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.live.read().await.as_ref().is_some_and(|live| !live.remembered_tried.swap(true, Ordering::SeqCst))
    }

    /// After a sign-out or idle lock: only a typed password signs in again while the file stays open.
    pub async fn forbid_remembered_sign_in(&self) {
        use std::sync::atomic::Ordering;
        if let Some(live) = self.live.read().await.as_ref() {
            live.remembered_tried.store(true, Ordering::SeqCst);
        }
    }

    /// The open company file's pool and sign-in, read together so both belong to the same file.
    pub async fn pool_and_session(&self) -> Result<(SqlitePool, SessionSlot), String> {
        let live = self.live.read().await;
        let live = live.as_ref().ok_or_else(|| NO_COMPANY_OPEN.to_string())?;
        Ok((live.pool.clone(), live.session.clone()))
    }

    /// The open company file, or [`NO_COMPANY_OPEN`].
    pub async fn path(&self) -> Result<PathBuf, String> {
        self.live.read().await.as_ref().map(|live| live.path.clone()).ok_or_else(|| NO_COMPANY_OPEN.to_string())
    }

    pub fn backups_dir(&self) -> &Path {
        &self.config.backups_dir
    }

    /// The open company file, or `None` when no company is open.
    pub async fn status(&self) -> Option<StorageStatus> {
        let live = self.live.read().await;
        let live = live.as_ref()?;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&live.pool).await.unwrap_or_else(|_| "unknown".into());
        let is_default = live.path == self.config.default_path;
        Some(StorageStatus {
            company_name: company_display_name(&live.pool, &live.path, is_default).await,
            path: live.path.display().to_string(),
            file_name: live.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            opened_from_file: live.opened_from_file,
            directory: live.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
            storage: live.storage.clone(),
            is_default,
            journal_mode,
            startup_warning: self.startup_warning.lock().expect("warning mutex").clone(),
        })
    }

    async fn require_status(&self) -> Result<StorageStatus, String> {
        self.status().await.ok_or_else(|| NO_COMPANY_OPEN.to_string())
    }

    /// A problem to show on the No Company Open screen (or in the app's banner), if any.
    pub fn warning(&self) -> Option<String> {
        self.startup_warning.lock().expect("warning mutex").clone()
    }

    /// Replaces the live database file with `staged` (a migrated, closed SQLite file) and reopens it.
    /// If the replacement cannot be opened, `fallback` (a snapshot of the previous file) is put back.
    pub async fn swap_in(&self, staged: &Path, fallback: &Path) -> Result<(), String> {
        let swapped = self.swap_in_file(staged, fallback, None).await;
        self.refresh_gate().await;
        swapped
    }

    /// [`Db::swap_in`], only if the live books still have `content_digest` (see
    /// `condense::content_digest`), checked under the write lock so nothing changes them in between.
    pub async fn swap_in_unchanged(&self, staged: &Path, fallback: &Path, content_digest: &str) -> Result<(), String> {
        let swapped = self.swap_in_file(staged, fallback, Some(content_digest)).await;
        self.refresh_gate().await;
        swapped
    }

    async fn swap_in_file(&self, staged: &Path, fallback: &Path, unchanged: Option<&str>) -> Result<(), String> {
        let mut guard = self.live.write().await;
        let live = guard.as_mut().ok_or_else(|| NO_COMPANY_OPEN.to_string())?;
        if let Some(expected) = unchanged {
            if crate::condense::content_digest(&live.pool).await? != expected {
                return Err("CONDENSE_BOOKS_CHANGED: the books changed while the condensed file was being made, so they were left as they were; run Condense Data again".into());
            }
        }
        // Windows cannot move or remove a file that any handle still has open, so every connection
        // is closed (and confirmed closed) before the files are touched.
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&live.pool).await;
        close_pool(&live.pool).await;
        let path = live.path.clone();
        let storage = live.storage.clone();
        let aside = match put_in_place(staged, &path).await {
            Ok(aside) => aside,
            Err(e) => {
                // Nothing was moved: reopen the books as they were.
                live.pool = open_for(&path, &storage, None).await.map_err(|open| format!("RESTORE_FAILED: {e}; reopening your books failed: {open}"))?;
                return Err(format!("RESTORE_SWAP: {e}; your current books were left in place"));
            }
        };
        match open_for(&path, &storage, None).await {
            Ok(pool) => {
                live.pool = pool;
                // The restored file has its own users; whoever was signed in signs in again.
                live.session = SessionSlot::default();
                remove_replaced(&aside).await;
                // The replacement's audit trail may be behind this computer's record of the old one.
                // The swap has happened either way, so a failure here must not report it as failed.
                if let Err(err) = crate::audit_chain::reanchor(&live.pool).await {
                    eprintln!("LocalBooks: could not record the restored file's audit trail: {err}");
                }
                Ok(())
            }
            Err(open_err) => {
                // Put the previous books back: the file moved aside, or else the snapshot.
                remove_with_sidecars(&path);
                if while_in_use(|| std::fs::rename(&aside, &path)).await.is_err() {
                    while_in_use(|| std::fs::copy(fallback, &path).map(|_| ())).await.map_err(|e| format!("RESTORE_FAILED: {open_err}; putting the previous books back also failed: {e}"))?;
                }
                remove_sidecars(&path).await?;
                live.pool = open_for(&path, &storage, None).await.map_err(|e| format!("RESTORE_FAILED: {open_err}; reopening the previous books failed: {e}"))?;
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
        let current = self.path().await?;
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

        let next = open_live(&target, Some(&self.config.backups_dir)).await?;
        let status = self.replace_live(next).await?;
        let directory = (target != self.config.default_path).then(|| new_dir.to_path_buf());
        storage::update_config(&self.config.config_dir, |config| config.database_directory = directory)?;
        Ok(status)
    }

    /// Closes the pool (checkpointing the journal) and releases any shared-location lock.
    pub async fn shutdown(&self) {
        let mut live = self.live.write().await;
        if let Some(live) = live.as_mut() {
            close_pool(&live.pool).await;
            live.lock = None;
        }
    }
}

/// Removes SQLite's journal files next to a closed company file, so a replacement file never meets
/// another file's log.
async fn remove_sidecars(path: &Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            while_in_use(|| match std::fs::remove_file(&sidecar) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            })
            .await
            .map_err(|e| format!("cannot remove {}: {e}", sidecar.display()))?;
        }
    }
    Ok(())
}

/// Runs a file operation, retrying for up to two seconds while Windows reports the file in use
/// (a just-closed connection's handle can take a moment to be released). Other errors return at once.
/// Closes `pool` and waits until every one of its connections has closed.
///
/// `Pool::close` in sqlx 0.8 can return with a connection still open: after a query, a connection
/// goes back to the pool in a spawned task that pings it first, and if `close` starts during that
/// ping, its last wait ends when the connection reaches the idle queue, which it does not look at
/// again. That connection (and the file, its `-wal` and `-shm`) then stays open for as long as any
/// clone of the pool lives. macOS and Linux don't mind; Windows refuses to move or remove the file.
/// Closing again closes it.
pub(crate) async fn close_pool(pool: &SqlitePool) {
    for _ in 0..400 {
        pool.close().await;
        if pool.size() == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Folds a closed company file's write-ahead log into it and switches it to a rollback journal,
/// which makes SQLite remove its `-wal` and `-shm` files. SQLite changes the journal mode only for
/// a connection that has the file to itself, so this also proves nothing else has it open. The
/// next [`open_for`] switches a local file back to WAL.
async fn leave_wal(path: &Path) -> Result<(), String> {
    let options = SqliteConnectOptions::new().filename(path).create_if_missing(false).busy_timeout(Duration::from_secs(5));
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await.map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mode: Result<String, _> = sqlx::query_scalar("PRAGMA journal_mode = DELETE").fetch_one(&mut conn).await;
    let _ = conn.close().await;
    match mode {
        Ok(mode) if mode.eq_ignore_ascii_case("delete") => clear_sidecars(path).await,
        Ok(mode) => Err(format!("{} is still open elsewhere (its journal stayed in {mode} mode)", path.display())),
        Err(e) => Err(format!("{} is still open elsewhere ({e})", path.display())),
    }
}

/// Removes the journal files of a closed company file in rollback mode, before the file is moved.
/// On Windows a virus scanner can hold a just-closed `-wal` or `-shm` for longer than it takes to
/// retry. Those two are harmless to leave as long as the log is empty: SQLite reads no pages from an
/// empty log, and the first connection to open a file rebuilds its `-shm`. A log that still has
/// content, or a rollback `-journal`, would be applied to the replacement, so those must go.
async fn clear_sidecars(path: &Path) -> Result<(), String> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if !sidecar.exists() {
            continue;
        }
        let removed = while_in_use(|| match std::fs::remove_file(&sidecar) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        })
        .await;
        let Err(e) = removed else { continue };
        let empty = || std::fs::metadata(&sidecar).map(|m| m.len() == 0).unwrap_or(true);
        let harmless = match suffix {
            "-shm" => true,
            "-wal" => empty() || (std::fs::OpenOptions::new().write(true).open(&sidecar).and_then(|f| f.set_len(0)).is_ok() && empty()),
            _ => false,
        };
        if !harmless {
            return Err(format!("cannot remove {}: {e}", sidecar.display()));
        }
        eprintln!("LocalBooks: left {} in place ({e}); it holds nothing", sidecar.display());
    }
    Ok(())
}

/// Marks a company file that was replaced and is waiting to be removed: `<file>.replaced-<pid>-<n>`.
const REPLACED_INFIX: &str = ".replaced-";

/// Where the file being replaced is moved while its replacement goes in.
fn replaced_path(path: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    PathBuf::from(format!("{}{REPLACED_INFIX}{}-{nanos}", path.display(), std::process::id()))
}

/// Puts the closed file `staged` in place of the closed company file `path`. The company file is
/// renamed aside first and the replacement renamed in, rather than one renamed over the other, so a
/// failure at any step can put the original back unchanged. Returns where the original now is; the
/// caller removes it once the replacement opens. On an error the company file is unchanged.
async fn put_in_place(staged: &Path, path: &Path) -> Result<PathBuf, String> {
    leave_wal(staged).await.map_err(|e| format!("the replacement file is not ready: {e}"))?;
    leave_wal(path).await.map_err(|e| format!("cannot close the company file: {e}"))?;
    let aside = replaced_path(path);
    while_in_use(|| std::fs::rename(path, &aside)).await.map_err(|e| format!("cannot move {} aside: {e}", path.display()))?;
    if let Err(e) = while_in_use(|| std::fs::rename(staged, path)).await {
        return match while_in_use(|| std::fs::rename(&aside, path)).await {
            Ok(()) => Err(format!("cannot put the replacement in place: {e}")),
            Err(back) => Err(format!("cannot put the replacement in place ({e}), nor move the company file back from {} ({back})", aside.display())),
        };
    }
    Ok(aside)
}

/// Removes a company file that was replaced. If something (a virus scanner, say) still has it open,
/// it is left for [`remove_stale_replaced`] to remove the next time the company file opens.
async fn remove_replaced(aside: &Path) {
    let _ = while_in_use(|| match std::fs::remove_file(aside) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    })
    .await;
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", aside.display()));
    }
}

/// Removes replaced copies of `path` that an earlier swap could not remove (see [`put_in_place`]).
fn remove_stale_replaced(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return };
    let prefix = format!("{}{REPLACED_INFIX}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

async fn while_in_use(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
    const IN_USE: [i32; 3] = [5, 32, 33];
    for waited in 0..200 {
        match op() {
            Err(e) if cfg!(windows) && e.raw_os_error().is_some_and(|code| IN_USE.contains(&code)) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            other => {
                if waited > 0 {
                    eprintln!("LocalBooks: a file was in use for about {} ms", waited * 50);
                }
                return other;
            }
        }
    }
    op()
}

/// Removes a company file that was never used, with SQLite's journal files.
fn remove_with_sidecars(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

/// The company name new files start with, until the user names theirs in Preferences.
pub const PLACEHOLDER_COMPANY_NAME: &str = "LocalBooks Ledger";

/// The name that tells the user which company is open: the Company name preference once it has been
/// set; otherwise the file's own name ("Acme Ltd.localbooks" → "Acme Ltd", or the folder's name for
/// a `localbooks.db` kept elsewhere). None for the usual company file that was never named.
pub async fn company_display_name(pool: &SqlitePool, path: &Path, is_default: bool) -> Option<String> {
    let named: Option<String> = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'company_name'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    if let Some(name) = named.map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && n != PLACEHOLDER_COMPANY_NAME) {
        return Some(name);
    }
    if is_default {
        return None;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().trim().to_string()).filter(|s| !s.is_empty());
    let generic = Path::new(DB_FILE_NAME).file_stem().map(|s| s.to_string_lossy().into_owned());
    match stem {
        Some(stem) if Some(&stem) != generic.as_ref() => Some(stem),
        _ => path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()),
    }
}

/// Opens an existing company file chosen by the user, after the same checks a restore applies.
async fn open_company_file(path: &Path, backups: &Path) -> Result<Live, String> {
    if !path.is_file() {
        return Err(format!("OPEN_NOT_FOUND: {} does not exist", path.display()));
    }
    crate::backup::inspect(path).await?;
    let mut live = open_live(path, Some(backups)).await?;
    live.opened_from_file = true;
    Ok(live)
}

/// Rewrites the company file without free space, and empties the write-ahead log, so values that
/// were just encrypted leave no plain copy behind in the file or its journal (after users are turned
/// on). Runs outside any transaction.
pub async fn compact(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("VACUUM").execute(pool).await.map_err(|e| format!("DB_COMPACT: {e}"))?;
    // Rollback-journal files (shared locations) have no log to empty; the pragma is a no-op there.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await.map_err(|e| format!("DB_COMPACT: {e}"))?;
    Ok(())
}

/// Embedded schema migrations from `src-tauri/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens (creating if needed) the company file at `path` on local disk and migrates it.
///
/// WAL + `synchronous = FULL` gives full durability: a committed entry survives power loss.
pub async fn open(path: &Path) -> Result<SqlitePool, String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    open_for(path, &storage::classify(dir), None).await
}

/// Opens with settings suited to where the file lives. Shared locations (network or synced folders)
/// cannot use WAL, so they use a rollback journal with exclusive locking on a single connection.
///
/// When an existing company file needs schema upgrades (a newer LocalBooks opening it for the first
/// time) and `backups` is given, a verified copy is saved there first, so an upgrade can always be
/// undone by restoring it.
async fn open_for(path: &Path, storage: &StorageInfo, backups: Option<&Path>) -> Result<SqlitePool, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data directory {}: {e}", dir.display()))?;
    }

    let base = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Full)
        // Overwrite deleted and replaced content, so a value written in plain and then encrypted
        // (an import's tax IDs) does not linger in free space.
        .pragma("secure_delete", "ON")
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

    if let Err(e) = prepare(&pool, path, backups).await {
        // Closed before returning, so a caller can move or remove the file straight away.
        close_pool(&pool).await;
        return Err(e);
    }
    // Seal anything written outside a write transaction (the migration records) into the audit chain.
    // Best effort: the books must still open (and be verifiable) if sealing fails; the next write
    // transaction seals them instead.
    let sealed = match crate::ledger::begin_write(&pool).await {
        Ok(tx) => crate::ledger::commit_write(tx).await,
        Err(e) => Err(e),
    };
    if let Err(e) = sealed {
        eprintln!("LocalBooks: could not seal the audit trail of {}: {e}", path.display());
    }

    Ok(pool)
}

/// Checks and upgrades a newly opened company file (see [`open_for`]).
async fn prepare(pool: &SqlitePool, path: &Path, backups: Option<&Path>) -> Result<(), String> {
    refuse_newer_file(pool, path).await?;
    if let Some(dir) = backups {
        if let Some(version) = pending_upgrade_from(pool).await? {
            crate::backup::pre_upgrade_snapshot(pool, path, dir, version).await.map_err(|e| {
                format!("{e} (LocalBooks did not upgrade {} because it could not save a copy first)", path.display())
            })?;
        }
    }
    migrate(pool).await.map_err(|e| format!("cannot migrate company file {}: {e}", path.display()))
}

/// Folds a closed company file's write-ahead log back into the file and removes it. The pool's
/// connections close together, so none of them sees itself as the last one and the log can be
/// left behind (seen on Linux and Windows); one connection opened and closed on its own does it.
/// Best effort: the data is safe either way, the next open replays the log.
async fn checkpoint_closed(path: &Path) {
    if !PathBuf::from(format!("{}-wal", path.display())).exists() {
        return;
    }
    let options = SqliteConnectOptions::new().filename(path).busy_timeout(Duration::from_secs(5));
    if let Ok(mut conn) = sqlx::SqliteConnection::connect_with(&options).await {
        // Reading the file attaches the log; as the only connection, closing then removes it.
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&mut conn).await;
        let _ = conn.close().await;
    }
}

/// The schema version of an existing company file that has migrations still to apply, or None for a
/// new (empty) file or one that is up to date.
async fn pending_upgrade_from(pool: &SqlitePool) -> Result<Option<i64>, String> {
    let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    Ok(applied_schema(pool).await?.filter(|v| *v < latest))
}

/// The highest schema version the company file has applied, or `None` for a new file.
async fn applied_schema(pool: &SqlitePool) -> Result<Option<i64>, String> {
    let has_table: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("cannot read company file: {e}"))?;
    if !has_table {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("cannot read company file: {e}"))
}

/// Refuses a company file that a newer LocalBooks has already upgraded, in words a person can act
/// on (the migrator's own error names a "missing" migration). The file is left untouched.
async fn refuse_newer_file(pool: &SqlitePool, path: &Path) -> Result<(), String> {
    let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    match applied_schema(pool).await? {
        Some(applied) if applied > latest => Err(format!(
            "DB_NEWER_FILE: {} was updated by a newer version of LocalBooks (company file version {applied}; this copy of LocalBooks understands up to {latest}). Install the latest LocalBooks to open it. The file has not been changed.",
            path.display()
        )),
        _ => Ok(()),
    }
}

/// Applies pending migrations with foreign-key enforcement off, as SQLite's table-rebuild procedure
/// requires (sqlx wraps each SQLite migration in a transaction, where the pragma cannot change).
/// The whole database is then checked for dangling references before enforcement is restored.
pub async fn migrate(pool: &SqlitePool) -> Result<(), String> {
    migrate_with(pool, &MIGRATOR).await
}

/// [`migrate`] with a given set of migrations (tests use it to build files from earlier releases).
pub(crate) async fn migrate_with(pool: &SqlitePool, migrator: &sqlx::migrate::Migrator) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await.map_err(|e| e.to_string())?;
    // `run_direct` takes the connection itself rather than an `Acquire` impl, which keeps this
    // future provably `Send` when a restore runs it inside a Tauri command.
    let migrated = migrator.run_direct(&mut *conn).await.map_err(|e| e.to_string());
    let violations = sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string());
    if let Err(e) = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await {
        // Never hand a connection without enforcement back to the pool.
        let _ = conn.detach().close().await;
        migrated?;
        return Err(e.to_string());
    }
    migrated?;
    if violations?.is_some() {
        return Err("foreign key check failed after migration".into());
    }
    record_migrations(&mut conn).await
}

#[cfg(test)]
mod migrate_tests;

/// Adds a MIGRATION event to the ledger audit trail for every applied schema migration not yet
/// recorded there, so the trail shows which schema each part of the history was written under.
async fn record_migrations(conn: &mut sqlx::SqliteConnection) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
         SELECT 'MIGRATION', m.version, json_object(
                    'version', m.version, 'description', m.description, 'checksum', lower(hex(m.checksum)),
                    'installedOn', m.installed_on, 'executionTimeNs', m.execution_time)
         FROM _sqlx_migrations m
         WHERE m.success = 1
           AND NOT EXISTS (SELECT 1 FROM ledger_audit_log a WHERE a.action_type = 'MIGRATION' AND a.target_entity_id = m.version)
         ORDER BY m.version",
    )
    .execute(conn)
    .await
    .map(|_| ())
    .map_err(|e| format!("cannot record migrations in the audit trail: {e}"))
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
pub(crate) async fn open_read_only_for_test(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .expect("open read-only")
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
mod tests;
