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
mod tests {
    use super::*;

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
        let cfg = AppConfig { database_directory: Some(PathBuf::from("/Volumes/Accounting/Books")) };
        write_config(&dir, &cfg).unwrap();
        assert_eq!(read_config(&dir), cfg);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
