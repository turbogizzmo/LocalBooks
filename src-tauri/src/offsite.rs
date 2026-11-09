//! Offsite backup: an encrypted copy of the books in a folder the user picks, such as a folder a
//! cloud service keeps in sync. The copy is encrypted here, before it is written, so whatever
//! carries the folder elsewhere sees only scrambled bytes. LocalBooks itself sends nothing over the
//! network; it only writes the file.
//!
//! - **Key.** At setup the user gives a backup password: their sign-in password, the company's
//!   recovery key, or another password. Argon2id turns it and a random salt into the backup key
//!   ([`crate::crypto::derive_backup_key`]). The key is kept in this computer's credential store
//!   ([`crate::secrets`]) so scheduled copies need no prompt; the password itself is never stored.
//!   Restoring on any computer needs only the file and the password.
//! - **File.** `<company>-<time>.lbbackup`: a header (format tag, salt, Argon2id cost and a random
//!   nonce prefix), then the verified snapshot ([`crate::backup::snapshot`]) in 1 MiB chunks, each
//!   sealed with AES-256-GCM. A chunk's nonce is the prefix and its number, and its associated data
//!   is the header, its number and whether it is the last, so chunks cannot be reordered, dropped
//!   or cut off without the file failing to open.
//! - **Schedule.** While the company is open, a copy is made when the newest is more than 20 hours
//!   old; the oldest copies past the number to keep are deleted from the folder.
//! - **Restore.** A copy is decrypted into the backups folder and handed to
//!   [`crate::backup::restore`], which checks it like any backup. **Test restore** decrypts and
//!   checks a copy without changing the books, and records when it last passed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use chrono::Local;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use zeroize::Zeroizing;

use crate::backup::{self, CompanyFileInfo, RestoreResult};
use crate::crypto::{self, KdfCost};
use crate::db::Db;
use crate::ledger::db_err;
use crate::secrets::SecretStore;

const MAGIC: &[u8; 8] = b"LBOFFS01";
const SALT_LEN: usize = 16;
const PREFIX_LEN: usize = 8;
/// Magic, salt, three u32 cost values and the nonce prefix.
const HEADER_LEN: usize = 8 + SALT_LEN + 12 + PREFIX_LEN;
const CHUNK: usize = 1024 * 1024;
const TAG_LEN: usize = 16;
pub const EXTENSION: &str = "lbbackup";
/// A copy is due when the newest is older than this.
pub const INTERVAL: Duration = Duration::from_secs(20 * 60 * 60);

pub const KEYCHAIN_SERVICE: &str = "LocalBooks offsite backup";
pub const SECRETS_FILE: &str = "offsite-backup-keys.json";

pub fn secret_store(config_dir: &Path) -> SecretStore {
    SecretStore::for_this_computer(config_dir, KEYCHAIN_SERVICE, SECRETS_FILE, "the offsite backup key")
}

/// The settings kept in the company file. The key is not one of them.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OffsiteSettings {
    pub folder: String,
    pub keep: i64,
    #[serde(skip)]
    pub salt: String,
    #[serde(skip)]
    pub kdf_memory_kib: i64,
    #[serde(skip)]
    pub kdf_iterations: i64,
    #[serde(skip)]
    pub kdf_parallelism: i64,
    /// Identifies the key in the credential store, and checks it is the one these settings made.
    #[serde(skip)]
    pub key_check: String,
    pub last_backup_at: Option<String>,
    pub last_backup_file: Option<String>,
    pub last_error: Option<String>,
    pub last_test_restore_at: Option<String>,
}

/// What the Offsite screen shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsiteStatus {
    pub settings: Option<OffsiteSettings>,
    /// This computer holds the key (otherwise the password must be given again).
    pub has_key: bool,
    pub copies: Vec<OffsiteCopy>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OffsiteCopy {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
}

fn now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn key_check(key: &[u8; 32]) -> String {
    let digest = Sha256::new().chain_update(b"localbooks offsite key check v1\0").chain_update(key).finalize();
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

pub async fn settings(pool: &SqlitePool) -> Result<Option<OffsiteSettings>, String> {
    sqlx::query_as("SELECT folder, keep, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, key_check, last_backup_at, last_backup_file, last_error, last_test_restore_at FROM offsite_backup WHERE id = 1")
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// The key for these settings from the credential store, if this computer has it.
async fn stored_key(store: &SecretStore, s: &OffsiteSettings) -> Result<Option<Zeroizing<[u8; 32]>>, String> {
    let Some(text) = store.get(&s.key_check).await? else { return Ok(None) };
    let text = Zeroizing::new(text);
    let bytes = Zeroizing::new(base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).map_err(|_| "OFFSITE_KEY: the stored backup key is damaged".to_string())?);
    let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| "OFFSITE_KEY: the stored backup key is damaged".to_string())?;
    let key = Zeroizing::new(key);
    Ok((key_check(&key) == s.key_check).then_some(key))
}

/// The Offsite screen: settings, whether this computer has the key, and the copies in the folder.
pub async fn status(pool: &SqlitePool, store: &SecretStore) -> Result<OffsiteStatus, String> {
    let settings = settings(pool).await?;
    let (has_key, copies) = match &settings {
        Some(s) => (stored_key(store, s).await?.is_some(), list_copies(Path::new(&s.folder))),
        None => (false, Vec::new()),
    };
    Ok(OffsiteStatus { settings, has_key, copies })
}

/// The copies in `folder`, newest first. A folder that cannot be read lists none.
pub fn list_copies(folder: &Path) -> Vec<OffsiteCopy> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut out: Vec<OffsiteCopy> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(OffsiteCopy { path: e.path().display().to_string(), file_name: e.file_name().to_string_lossy().into_owned(), size_bytes: meta.len() })
        })
        .collect();
    // Names end in a sortable time stamp.
    out.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    out
}

/// Turns offsite backup on (or changes its folder, password or count): derives the key from
/// `password` and keeps it in this computer's credential store.
pub async fn set_up(pool: &SqlitePool, store: &SecretStore, folder: &Path, password: &str, keep: i64) -> Result<OffsiteSettings, String> {
    crypto::check_password_rules(password).map_err(|e| e.replacen("PASSWORD", "OFFSITE_PASSWORD", 1))?;
    if !folder.is_dir() {
        return Err(format!("OFFSITE_FOLDER: {} is not a folder", folder.display()));
    }
    if !(1..=100).contains(&keep) {
        return Err("OFFSITE_KEEP: keep between 1 and 100 copies".into());
    }
    let salt: [u8; SALT_LEN] = crypto::random()?;
    let cost = KdfCost::for_new_keys();
    let key = crypto::derive_backup_key(password, salt, cost).await?;
    let check = key_check(&key);
    store.put(&check, &base64::engine::general_purpose::STANDARD.encode(key.as_slice())).await?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(salt);
    sqlx::query(
        "INSERT INTO offsite_backup (id, folder, keep, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, key_check) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (id) DO UPDATE SET folder = excluded.folder, keep = excluded.keep, salt = excluded.salt, kdf_memory_kib = excluded.kdf_memory_kib,
           kdf_iterations = excluded.kdf_iterations, kdf_parallelism = excluded.kdf_parallelism, key_check = excluded.key_check, last_error = NULL",
    )
    .bind(folder.display().to_string())
    .bind(keep)
    .bind(b64)
    .bind(i64::from(cost.memory_kib))
    .bind(i64::from(cost.iterations))
    .bind(i64::from(cost.parallelism))
    .bind(&check)
    .execute(pool)
    .await
    .map_err(db_err)?;
    settings(pool).await?.ok_or_else(|| "OFFSITE_SETTINGS: not saved".into())
}

/// Turns offsite backup off. Copies already in the folder stay there.
pub async fn turn_off(pool: &SqlitePool, store: &SecretStore) -> Result<(), String> {
    if let Some(s) = settings(pool).await? {
        store.delete(&s.key_check).await?;
    }
    sqlx::query("DELETE FROM offsite_backup WHERE id = 1").execute(pool).await.map_err(db_err)?;
    Ok(())
}

fn header(salt: &[u8; SALT_LEN], cost: KdfCost, prefix: &[u8; PREFIX_LEN]) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..8].copy_from_slice(MAGIC);
    h[8..24].copy_from_slice(salt);
    h[24..28].copy_from_slice(&cost.memory_kib.to_le_bytes());
    h[28..32].copy_from_slice(&cost.iterations.to_le_bytes());
    h[32..36].copy_from_slice(&cost.parallelism.to_le_bytes());
    h[36..].copy_from_slice(prefix);
    h
}

fn chunk_aad(header: &[u8; HEADER_LEN], index: u32, last: bool) -> Vec<u8> {
    let mut aad = header.to_vec();
    aad.extend_from_slice(&index.to_be_bytes());
    aad.push(u8::from(last));
    aad
}

fn nonce(prefix: &[u8; PREFIX_LEN], index: u32) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..PREFIX_LEN].copy_from_slice(prefix);
    n[PREFIX_LEN..].copy_from_slice(&index.to_be_bytes());
    n
}

/// Reads up to `buf.len()` bytes, fewer only at the end of the file.
fn fill(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match reader.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Encrypts `plain` into `out` with `key` (derived from `salt` and `cost`).
pub fn encrypt_file(plain: &Path, out: &Path, key: &[u8; 32], salt: &[u8; SALT_LEN], cost: KdfCost) -> Result<(), String> {
    let io = |e: std::io::Error| format!("OFFSITE_WRITE: {e}");
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "OFFSITE_KEY: bad key length".to_string())?;
    let prefix: [u8; PREFIX_LEN] = crypto::random()?;
    let head = header(salt, cost, &prefix);
    let mut reader = std::io::BufReader::new(std::fs::File::open(plain).map_err(io)?);
    let mut writer = std::io::BufWriter::new(std::fs::File::create(out).map_err(io)?);
    writer.write_all(&head).map_err(io)?;
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    // Read one chunk ahead, so the last chunk is known when it is sealed.
    let mut len = fill(&mut reader, &mut buf).map_err(io)?;
    let mut next = Zeroizing::new(vec![0u8; CHUNK]);
    let mut index: u32 = 0;
    loop {
        let next_len = if len == CHUNK { fill(&mut reader, &mut next).map_err(io)? } else { 0 };
        let last = next_len == 0;
        let sealed = cipher
            .encrypt(&Nonce::from(nonce(&prefix, index)), Payload { msg: &buf[..len], aad: &chunk_aad(&head, index, last) })
            .map_err(|_| "OFFSITE_ENCRYPT: encryption failed".to_string())?;
        writer.write_all(&sealed).map_err(io)?;
        if last {
            break;
        }
        std::mem::swap(&mut buf, &mut next);
        len = next_len;
        index = index.checked_add(1).ok_or_else(|| "OFFSITE_TOO_LARGE: the company file is too large".to_string())?;
    }
    writer.into_inner().map_err(|e| io(e.into_error()))?.sync_all().map_err(io)?;
    Ok(())
}

/// A copy's header as read: its bytes (part of every chunk's associated data), the salt and cost
/// its key was derived with, and the nonce prefix.
type Header = ([u8; HEADER_LEN], [u8; SALT_LEN], KdfCost, [u8; PREFIX_LEN]);

fn read_header(file: &mut impl Read) -> Result<Header, String> {
    let mut head = [0u8; HEADER_LEN];
    let not_ours = || "OFFSITE_NOT_A_BACKUP: this is not a LocalBooks offsite backup".to_string();
    file.read_exact(&mut head).map_err(|_| not_ours())?;
    if &head[..8] != MAGIC {
        return Err(not_ours());
    }
    let u = |r: std::ops::Range<usize>| u32::from_le_bytes(head[r].try_into().unwrap_or([0; 4]));
    let cost = KdfCost { memory_kib: u(24..28), iterations: u(28..32), parallelism: u(32..36) };
    // The salt and nonce prefix are the file's own bytes.
    let salt: [u8; SALT_LEN] = head[8..24].try_into().map_err(|_| not_ours())?;
    let prefix: [u8; PREFIX_LEN] = head[36..].try_into().map_err(|_| not_ours())?;
    Ok((head, salt, cost, prefix))
}

/// Decrypts the copy at `path` into `out`, with the key `password` gives. On any failure `out` is
/// removed, so no partly decrypted data is left behind.
pub async fn decrypt_file(path: &Path, password: &str, out: &Path) -> Result<(), String> {
    let result = decrypt_into(path, password, out).await;
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

async fn decrypt_into(path: &Path, password: &str, out: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| format!("OFFSITE_READ: {e}");
    let mut reader = std::io::BufReader::new(std::fs::File::open(path).map_err(io)?);
    let (head, salt, cost, prefix) = read_header(&mut reader)?;
    // A damaged header can carry an unusable cost; report it as a file that isn't a backup.
    let key = crypto::derive_backup_key(password, salt, cost).await.map_err(|_| "OFFSITE_NOT_A_BACKUP: the file's header is damaged".to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice()).map_err(|_| "OFFSITE_KEY: bad key length".to_string())?;
    let wrong = || "OFFSITE_DECRYPT: the password is wrong, or the file is damaged or incomplete".to_string();
    let mut writer = std::io::BufWriter::new(std::fs::File::create(out).map_err(io)?);
    let mut buf = vec![0u8; CHUNK + TAG_LEN];
    let mut next = vec![0u8; CHUNK + TAG_LEN];
    let mut len = fill(&mut reader, &mut buf).map_err(io)?;
    let mut index: u32 = 0;
    loop {
        let next_len = if len == buf.len() { fill(&mut reader, &mut next).map_err(io)? } else { 0 };
        let last = next_len == 0;
        let plain = Zeroizing::new(cipher.decrypt(&Nonce::from(nonce(&prefix, index)), Payload { msg: &buf[..len], aad: &chunk_aad(&head, index, last) }).map_err(|_| wrong())?);
        writer.write_all(&plain).map_err(io)?;
        if last {
            break;
        }
        std::mem::swap(&mut buf, &mut next);
        len = next_len;
        index = index.checked_add(1).ok_or_else(wrong)?;
    }
    writer.flush().map_err(io)?;
    Ok(())
}

/// A file name part from the company name: letters, digits and dashes.
fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-').chars().take(40).collect::<String>();
    if s.is_empty() { "company".into() } else { s }
}

/// Makes an encrypted copy in the folder now, and deletes the oldest past the number to keep.
pub async fn back_up_now(db: &Db, store: &SecretStore) -> Result<OffsiteCopy, String> {
    let pool = db.pool().await;
    let s = settings(&pool).await?.ok_or_else(|| "OFFSITE_OFF: offsite backup is not set up".to_string())?;
    let result = write_copy(db, &pool, store, &s).await;
    let error = result.as_ref().err().cloned();
    sqlx::query("UPDATE offsite_backup SET last_error = ?1, last_backup_at = CASE WHEN ?1 IS NULL THEN ?2 ELSE last_backup_at END, last_backup_file = CASE WHEN ?1 IS NULL THEN ?3 ELSE last_backup_file END WHERE id = 1")
        .bind(&error)
        .bind(now())
        .bind(result.as_ref().ok().map(|c| c.file_name.clone()))
        .execute(&pool)
        .await
        .map_err(db_err)?;
    result
}

async fn write_copy(db: &Db, pool: &SqlitePool, store: &SecretStore, s: &OffsiteSettings) -> Result<OffsiteCopy, String> {
    let key = stored_key(store, s).await?.ok_or_else(|| "OFFSITE_KEY_MISSING: this computer does not have the backup key; enter the backup password again under Offsite".to_string())?;
    let folder = PathBuf::from(&s.folder);
    if !folder.is_dir() {
        return Err(format!("OFFSITE_FOLDER: the folder {} is not there; is the drive or synced folder available?", folder.display()));
    }
    let salt: [u8; SALT_LEN] = base64::engine::general_purpose::STANDARD.decode(&s.salt).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| "OFFSITE_SETTINGS: damaged salt".to_string())?;
    let cost = KdfCost { memory_kib: u32::try_from(s.kdf_memory_kib).unwrap_or(0), iterations: u32::try_from(s.kdf_iterations).unwrap_or(0), parallelism: u32::try_from(s.kdf_parallelism).unwrap_or(0) };
    let company: String = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'company_name'").fetch_optional(pool).await.map_err(db_err)?.unwrap_or_default();
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("{}-{stamp}.{EXTENSION}", safe_name(&company));
    let plain = db.backups_dir().join(format!(".offsite-{}.db", std::process::id()));
    let partial = folder.join(format!(".{file_name}.partial"));
    let done = async {
        backup::snapshot(pool, &db.path().await?, &plain).await?;
        encrypt_file(&plain, &partial, &key, &salt, cost)?;
        std::fs::rename(&partial, folder.join(&file_name)).map_err(|e| format!("OFFSITE_WRITE: {e}"))
    }
    .await;
    let _ = std::fs::remove_file(&plain);
    if done.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    done?;
    let copies = list_copies(&folder);
    for old in copies.iter().skip(usize::try_from(s.keep).unwrap_or(1)) {
        let _ = std::fs::remove_file(&old.path);
    }
    copies.into_iter().find(|c| c.file_name == file_name).ok_or_else(|| "OFFSITE_WRITE: the copy is not in the folder".into())
}

/// The scheduled copy: made when offsite backup is on and the newest copy is older than
/// [`INTERVAL`]. `None` when nothing was due.
pub async fn run_if_due(db: &Db, store: &SecretStore) -> Result<Option<OffsiteCopy>, String> {
    if !db.is_open().await {
        return Ok(None);
    }
    let pool = db.pool().await;
    let Some(s) = settings(&pool).await? else { return Ok(None) };
    let fresh = list_copies(Path::new(&s.folder))
        .first()
        .and_then(|c| std::fs::metadata(&c.path).and_then(|m| m.modified()).ok())
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < INTERVAL);
    if fresh {
        return Ok(None);
    }
    back_up_now(db, store).await.map(Some)
}

/// Decrypts a copy into a temporary file in the backups folder, runs `then` on it, and deletes it.
async fn with_decrypted<T, F, Fut>(db: &Db, path: &Path, password: &str, then: F) -> Result<T, String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let tmp = db.backups_dir().join(format!(".offsite-restore-{}.db", std::process::id()));
    let result = match decrypt_file(path, password, &tmp).await {
        Ok(()) => then(tmp.clone()).await,
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Opens a copy and checks it as a restore would, without changing the books. Records the time
/// when it passes.
pub async fn test_restore(db: &Db, path: &Path, password: &str) -> Result<CompanyFileInfo, String> {
    let info = with_decrypted(db, path, password, |tmp| async move { backup::inspect(&tmp).await }).await?;
    if db.is_open().await {
        sqlx::query("UPDATE offsite_backup SET last_test_restore_at = ?1 WHERE id = 1").bind(now()).execute(&db.pool().await).await.map_err(db_err)?;
    }
    Ok(info)
}

/// Replaces the open books with a copy, through the checked restore of any backup.
pub async fn restore(db: &Db, path: &Path, password: &str) -> Result<RestoreResult, String> {
    with_decrypted(db, path, password, |tmp| async move { backup::restore(db, &tmp).await }).await
}

#[cfg(test)]
mod tests;
