//! Where the company file lives: classification of a location (local disk, network share, synced
//! folder), the `config.json` pointer that overrides the default location, and the single-user lock
//! used on shared locations.
//!
//! SQLite's WAL mode needs shared memory on one machine, and network/sync file locking is
//! unreliable, so shared locations are opened in rollback-journal mode with exclusive locking, and
//! a heartbeat lock file keeps a second computer from opening the same books at the same time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

pub const DB_FILE_NAME: &str = "localbooks.db";
/// Extension of a company file the operating system opens with LocalBooks (double-click, Open With).
pub const COMPANY_FILE_EXTENSION: &str = "localbooks";

/// Whether `path` names a `.localbooks` company file (extension compared case-insensitively).
pub fn is_company_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(COMPANY_FILE_EXTENSION))
}

/// The company file the app was launched to open, if any. Windows passes the file as a plain path
/// argument; Linux desktops may pass a `file://` URL. The first argument (the executable) is skipped.
pub fn company_file_from_args<I: IntoIterator<Item = std::ffi::OsString>>(args: I) -> Option<PathBuf> {
    args.into_iter().skip(1).find_map(|arg| {
        let text = arg.to_string_lossy();
        let path = if text.starts_with("file://") {
            tauri::Url::parse(&text).ok()?.to_file_path().ok()?
        } else {
            PathBuf::from(arg)
        };
        is_company_file(&path).then_some(path)
    })
}
/// A lock whose heartbeat is older than this is considered abandoned (crash, power loss).
const STALE_AFTER: Duration = Duration::from_secs(180);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageKind {
    Local,
    Network,
    Synced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub kind: StorageKind,
    /// Human description, e.g. "Local disk (apfs)", "Network drive (smbfs)", "Synced folder (Dropbox)".
    pub detail: String,
}

impl StorageInfo {
    pub fn is_shared(&self) -> bool {
        self.kind != StorageKind::Local
    }
}

const NETWORK_FS: &[&str] = &["smbfs", "nfs", "afpfs", "webdav", "cifs", "smb2", "smb3", "ncpfs", "9p", "sshfs", "fuse.sshfs", "davfs"];

/// Pure classification from a path and its filesystem type (testable without real mounts).
pub fn classify_parts(path: &str, fs_type: Option<&str>) -> StorageInfo {
    if let Some(fs) = fs_type.filter(|fs| NETWORK_FS.contains(&fs.to_ascii_lowercase().as_str())) {
        return StorageInfo { kind: StorageKind::Network, detail: format!("Network drive ({fs})") };
    }
    // UNC shares (\\server\share, //server/share). Windows verbatim paths (\\?\C:\...) are local
    // unless they are verbatim UNC paths (\\?\UNC\server\share).
    let verbatim = path.strip_prefix(r"\\?\");
    let unc = match verbatim {
        Some(rest) => rest.get(..4).is_some_and(|p| p.eq_ignore_ascii_case(r"UNC\")),
        None => path.starts_with(r"\\") || path.starts_with("//"),
    };
    if unc {
        return StorageInfo { kind: StorageKind::Network, detail: "Network share (UNC path)".into() };
    }

    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if let Some(idx) = lower.find("/library/cloudstorage/") {
        let rest = &normalized[idx + "/library/cloudstorage/".len()..];
        let provider = rest.split('/').next().unwrap_or("cloud storage");
        let provider = provider.split('-').next().unwrap_or(provider);
        return StorageInfo { kind: StorageKind::Synced, detail: format!("Synced folder ({provider})") };
    }
    if lower.contains("/library/mobile documents/com~apple~clouddocs") {
        return StorageInfo { kind: StorageKind::Synced, detail: "Synced folder (iCloud Drive)".into() };
    }
    for (marker, provider) in [("dropbox", "Dropbox"), ("onedrive", "OneDrive"), ("google drive", "Google Drive"), ("googledrive", "Google Drive"), ("icloud drive", "iCloud Drive"), ("box sync", "Box"), ("box", "Box")] {
        if lower.split('/').any(|component| component == marker || component.starts_with(&format!("{marker} ")) || component.starts_with(&format!("{marker}-"))) {
            return StorageInfo { kind: StorageKind::Synced, detail: format!("Synced folder ({provider})") };
        }
    }
    let fs = fs_type.unwrap_or("unknown filesystem");
    StorageInfo { kind: StorageKind::Local, detail: format!("Local disk ({fs})") }
}

/// Classifies the directory that holds (or will hold) the company file.
pub fn classify(dir: &Path) -> StorageInfo {
    classify_parts(&dir.to_string_lossy(), filesystem_type(dir).as_deref())
}

#[cfg(target_os = "macos")]
fn filesystem_type(dir: &Path) -> Option<String> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated string and `stats` is a properly sized out-param.
    if unsafe { libc::statfs(c_path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    // SAFETY: the kernel fills `f_fstypename` with a NUL-terminated name.
    let name = unsafe { CStr::from_ptr(stats.f_fstypename.as_ptr()) };
    Some(name.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
fn filesystem_type(dir: &Path) -> Option<String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated string and `stats` is a properly sized out-param.
    if unsafe { libc::statfs(c_path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let name = match stats.f_type as u64 {
        0x6969 => "nfs",
        0x517B => "smbfs",
        0xFF53_4D42 => "cifs",
        0xFE53_4D42 => "smb2",
        0x6573_5546 => "fuse",
        _ => "local",
    };
    Some(name.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn filesystem_type(_dir: &Path) -> Option<String> {
    None
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Splits a drive-letter path (`Z:\\Accounting`) into its drive (`Z:`) and remainder (`\\Accounting`).
pub fn split_drive_letter(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes.len() == 2 || matches!(bytes[2], b'\\' | b'/')) {
        Some((&path[..2], &path[2..]))
    } else {
        None
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Joins a network share (`\\\\nas\\books`) with the rest of a mapped-drive path (`\\2026\\q1`).
pub fn join_unc(remote: &str, rest: &str) -> String {
    let remote = remote.trim_end_matches(['\\', '/']);
    let rest = rest.replace('/', "\\");
    if rest.is_empty() || rest == "\\" {
        remote.to_string()
    } else if rest.starts_with('\\') {
        format!("{remote}{rest}")
    } else {
        format!("{remote}\\{rest}")
    }
}

/// Resolves a path on a mapped network drive (`Z:\\Accounting`) to its UNC form
/// (`\\\\nas\\share\\Accounting`), so the company file keeps working even if the drive letter is
/// unmapped or mapped differently, and so the location is recognized as a network share. Paths on
/// local drives, and every path on other platforms, are returned unchanged.
pub fn resolve_network_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str() {
            if let Some((drive, rest)) = split_drive_letter(text) {
                if let Some(remote) = mapped_drive_remote(drive) {
                    return PathBuf::from(join_unc(&remote, rest));
                }
            }
        }
    }
    path.to_path_buf()
}

/// The UNC share behind a mapped drive letter (`Z:` → `\\\\nas\\share`), via `WNetGetConnectionW`.
/// `None` for local drives, disconnected mappings, or any API failure.
#[cfg(windows)]
pub fn mapped_drive_remote(drive: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;

    let local: Vec<u16> = drive.encode_utf16().chain(std::iter::once(0)).collect();
    let mut len: u32 = 260;
    let mut buf: Vec<u16> = vec![0; len as usize];
    // SAFETY: `local` is NUL-terminated; `buf` has `len` writable u16 slots and `len` is updated in place.
    let mut rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
    if rc == ERROR_MORE_DATA {
        buf = vec![0; len as usize];
        // SAFETY: as above, with the buffer resized to the length the API asked for.
        rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
    }
    if rc != NO_ERROR {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let remote = String::from_utf16_lossy(&buf[..end]);
    (!remote.is_empty()).then_some(remote)
}

/// Confirms `dir` exists, is a directory, and accepts new files.
pub fn check_writable_dir(dir: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(dir).map_err(|_| format!("STORAGE_MISSING: {} is not available", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("STORAGE_NOT_FOLDER: {} is not a folder", dir.display()));
    }
    let probe = dir.join(format!(".localbooks-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| format!("STORAGE_READ_ONLY: cannot write to {}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// config.json
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Folder holding `localbooks.db`; `None` means the default app-data folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_directory: Option<PathBuf>,
    /// Company files opened on this computer, most recent first (see [`add_recent`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_companies: Vec<RecentCompany>,
    /// The company was closed (File › Close Company) when LocalBooks last quit, so the next launch
    /// starts on the No Company Open screen, as classic desktop accounting software does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub start_with_no_company: bool,
}

pub fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.json")
}

/// Reads `config.json`; a missing or unreadable file means defaults.
pub fn read_config(config_dir: &Path) -> AppConfig {
    std::fs::read(config_path(config_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn write_config(config_dir: &Path, config: &AppConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("STORAGE_CONFIG: cannot create {}: {e}", config_dir.display()))?;
    let body = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    crate::export_engine::write_atomically(&config_path(config_dir), &body).map_err(|e| e.replace("EXPORT_WRITE", "STORAGE_CONFIG"))
}

/// Reads `config.json`, applies `change`, and writes it back, keeping every other setting.
pub fn update_config(config_dir: &Path, change: impl FnOnce(&mut AppConfig)) -> Result<(), String> {
    let mut config = read_config(config_dir);
    change(&mut config);
    write_config(config_dir, &config)
}

// ------------------------------------------------------------------------------------------------
// Recent company files (File › Open Previous Company, the No Company Open screen)
// ------------------------------------------------------------------------------------------------

/// How many company files the recent list keeps.
pub const MAX_RECENT_COMPANIES: usize = 10;

/// A company file opened on this computer, as remembered in `config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCompany {
    pub path: PathBuf,
    /// The company's name when it was last open; `None` for a file that was never named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_name: Option<String>,
    /// Local time it was last opened, `YYYY-MM-DD HH:MM`.
    pub last_opened: String,
    /// Size in bytes when it was last opened.
    #[serde(default)]
    pub file_size: u64,
}

impl RecentCompany {
    /// An entry for `path` opened now, with its current size on disk.
    pub fn opened_now(path: &Path, company_name: Option<String>) -> RecentCompany {
        RecentCompany {
            path: path.to_path_buf(),
            company_name,
            last_opened: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            file_size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        }
    }
}

/// Whether two paths name the same file (spelled the same, or the same file once resolved).
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y)
}

/// Puts `entry` at the top of the list, replacing any older entry for the same file, and keeps at
/// most [`MAX_RECENT_COMPANIES`].
pub fn add_recent(list: &mut Vec<RecentCompany>, entry: RecentCompany) {
    list.retain(|r| !same_file(&r.path, &entry.path));
    list.insert(0, entry);
    list.truncate(MAX_RECENT_COMPANIES);
}

/// Takes `path` off the list (the file itself is never touched). Whether it was there.
pub fn remove_recent(list: &mut Vec<RecentCompany>, path: &Path) -> bool {
    let before = list.len();
    list.retain(|r| !same_file(&r.path, path));
    list.len() != before
}

/// A recent company file as the No Company Open screen and the File menu show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCompanyView {
    pub path: String,
    /// The company's name, or the file's name for one that was never named.
    pub company_name: String,
    pub file_name: String,
    /// The folder that holds it.
    pub location: String,
    pub last_opened: String,
    /// When the file was last changed on disk, `YYYY-MM-DD HH:MM`; `None` when it is missing.
    pub last_modified: Option<String>,
    /// Current size in bytes (the remembered size when the file is missing).
    pub file_size: u64,
    /// The file is no longer there (moved, deleted, or on a drive that is not connected). Missing
    /// entries stay listed, so a reconnected drive brings them back; Edit List removes them.
    pub missing: bool,
}

/// The recent list with each file's current state on disk.
pub fn describe_recent(list: &[RecentCompany]) -> Vec<RecentCompanyView> {
    list.iter()
        .map(|r| {
            let meta = std::fs::metadata(&r.path).ok().filter(|m| m.is_file());
            let file_name = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            RecentCompanyView {
                path: r.path.display().to_string(),
                company_name: r.company_name.clone().unwrap_or_else(|| file_name.clone()),
                location: r.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
                file_name,
                last_opened: r.last_opened.clone(),
                last_modified: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string()),
                file_size: meta.as_ref().map_or(r.file_size, |m| m.len()),
                missing: meta.is_none(),
            }
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------
// Find a company file
// ------------------------------------------------------------------------------------------------

/// A company file found on disk by [`find_company_files`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundCompanyFile {
    pub path: String,
    pub file_name: String,
    pub location: String,
    pub last_modified: Option<String>,
    pub file_size: u64,
}

/// Folders never searched: app bundles, caches, build output, and the like.
const SKIPPED_FOLDERS: &[&str] = &["Library", "node_modules", "target", "Applications", "System", "private", "proc", "dev"];

/// Looks for company files (`*.localbooks`, and `localbooks.db` kept in a folder) under each of
/// `roots`, at most `max_depth` folders deep, skipping hidden and system folders. Stops after
/// `limit` files or at `deadline`, so a huge or slow (network) folder cannot hang the search.
/// Newest first.
pub fn find_company_files(roots: &[PathBuf], max_depth: usize, limit: usize, deadline: std::time::Instant) -> Vec<FoundCompanyFile> {
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pending: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    let mut modified = std::collections::HashMap::new();
    while let Some((dir, depth)) = pending.pop() {
        if found.len() >= limit || std::time::Instant::now() >= deadline {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if depth < max_depth && !name.starts_with('.') && !SKIPPED_FOLDERS.contains(&name.as_str()) && !name.ends_with(".app") {
                    pending.push((path, depth + 1));
                }
            } else if kind.is_file() && (is_company_file(&path) || name == DB_FILE_NAME) {
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                if !seen.insert(key) {
                    continue;
                }
                let meta = entry.metadata().ok();
                let time = meta.as_ref().and_then(|m| m.modified().ok());
                found.push(FoundCompanyFile {
                    path: path.display().to_string(),
                    file_name: name,
                    location: dir.display().to_string(),
                    last_modified: time.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string()),
                    file_size: meta.map_or(0, |m| m.len()),
                });
                modified.insert(path.display().to_string(), time);
                if found.len() >= limit {
                    break;
                }
            }
        }
    }
    found.sort_by(|a, b| modified[&b.path].cmp(&modified[&a.path]).then_with(|| a.path.cmp(&b.path)));
    found
}

// ------------------------------------------------------------------------------------------------
// Single-user lock for shared locations
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockRecord {
    host: String,
    pid: u32,
    acquired_at: String,
}

pub fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a writable buffer of the declared length.
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            return String::from_utf8_lossy(&buf[..end]).into_owned();
        }
    }
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "this computer".into())
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs error checking only; no signal is delivered.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    true
}

pub fn lock_path(db_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lock", db_path.display()))
}

/// Held while this process has a shared-location company file open. Dropping it releases the lock.
pub struct FileLock {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl FileLock {
    pub fn acquire(db_path: &Path) -> Result<FileLock, String> {
        let path = lock_path(db_path);
        let me = LockRecord { host: host_name(), pid: std::process::id(), acquired_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string() };

        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(holder) = serde_json::from_slice::<LockRecord>(&bytes) {
                let heartbeat_age = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .unwrap_or(Duration::MAX);
                let mine = holder.host == me.host && holder.pid == me.pid;
                let dead_here = holder.host == me.host && !process_alive(holder.pid);
                if !mine && !dead_here && heartbeat_age < STALE_AFTER {
                    return Err(format!(
                        "STORAGE_IN_USE: this company file is open on {} (since {}). Close LocalBooks there first; shared company files can only be open on one computer at a time.",
                        holder.host, holder.acquired_at
                    ));
                }
            }
        }

        let body = serde_json::to_vec(&me).map_err(|e| e.to_string())?;
        std::fs::write(&path, &body).map_err(|e| format!("STORAGE_LOCK: cannot create {}: {e}", path.display()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let (beat_path, beat_stop) = (path.clone(), stop.clone());
        std::thread::spawn(move || {
            while !beat_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                let due = std::fs::metadata(&beat_path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .is_some_and(|age| age >= HEARTBEAT_EVERY);
                if due && !beat_stop.load(Ordering::Relaxed) {
                    let _ = std::fs::write(&beat_path, &body);
                }
            }
        });
        Ok(FileLock { path, stop })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Only remove the lock if it is still ours.
        if let Ok(bytes) = std::fs::read(&self.path) {
            if let Ok(holder) = serde_json::from_slice::<LockRecord>(&bytes) {
                if holder.host == host_name() && holder.pid == std::process::id() {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
