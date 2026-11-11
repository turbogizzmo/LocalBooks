//! The key header in front of an encrypted company file (gap R1, phase 2 of
//! `docs/plans/encryption-at-rest.md`).
//!
//! Layout of the first [`HEADER_LEN`](super::vfs::HEADER_LEN) bytes:
//!
//! - bytes 0–15: [`SIGNATURE`](super::vfs::SIGNATURE), written once and never changed;
//! - two copies of the header at [`COPY_A`] and [`COPY_B`], each [`COPY_LEN`] bytes: a magic tag,
//!   a sequence number, the body's length, the body, and a SHA-256 of all of that. Writes go to the
//!   copy that is not the newest, and are synced, so a crash part-way through a write leaves the
//!   other copy whole;
//! - the rest is zero.
//!
//! **Files SQLite has open.** On macOS and Linux, closing any handle to a file drops every lock this
//! process holds on it, SQLite's included. So the path-based functions here are for files no
//! connection in this process has open (before opening, or a copy); a live database's header is
//! read and written with [`read_open`] and [`write_open`], which go through the connection's own
//! file handle ([`super::vfs::FCNTL_HEADER_IO`]).
//!
//! The body (JSON) names the SQLCipher page size and holds the key slots. A slot is opaque here:
//! the slot's kind, the user it belongs to and the sealed key, as [`crate::crypto`] writes them. They
//! are needed before the database can be decrypted, so they can't live only inside it.

use std::ffi::{c_int, c_void};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use libsqlite3_sys as ffi;
use sqlx::SqliteConnection;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::vfs::{HeaderIo, FCNTL_HEADER_IO, HEADER_LEN, SIGNATURE};

const MAGIC: &[u8; 8] = b"LBHDR001";
pub const COPY_A: u64 = 4096;
pub const COPY_LEN: u64 = 28 * 1024;
pub const COPY_B: u64 = COPY_A + COPY_LEN + 4096;
/// Magic, sequence number and body length.
const PREFIX_LEN: usize = 8 + 8 + 4;
const HASH_LEN: usize = 32;
/// The largest body a copy can hold.
pub const MAX_BODY: usize = COPY_LEN as usize - PREFIX_LEN - HASH_LEN;

/// A key slot as the header carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderSlot {
    /// `PASSWORD` or `RECOVERY`.
    pub kind: String,
    pub user_id: Option<i64>,
    /// The user's sign-in name, so the sign-in screen can find the slot before the users table
    /// can be read.
    pub username: Option<String>,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    /// Base64: 16-byte salt, 12-byte nonce, 48-byte sealed key.
    pub salt: String,
    pub nonce: String,
    pub wrapped_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// A random id for this encrypted file (shared by its copies and backups), which every slot is
    /// bound to, so a slot moved from another file doesn't open this one.
    #[serde(default)]
    pub file_id: String,
    /// SQLCipher's page size (`cipher_page_size`), which the WAL guard compares with a WAL's.
    pub page_size: u32,
    pub slots: Vec<HeaderSlot>,
}

/// Where the header bytes are read and written.
trait Store {
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), String>;
    /// Writes and syncs.
    fn write_at(&mut self, at: u64, buf: &[u8]) -> Result<(), String>;
}

/// A file no connection in this process has open.
impl Store for File {
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), String> {
        self.seek(SeekFrom::Start(at)).and_then(|_| self.read_exact(buf)).map_err(|e| format!("FILE_HEADER: cannot read the key header: {e}"))
    }

    fn write_at(&mut self, at: u64, buf: &[u8]) -> Result<(), String> {
        self.seek(SeekFrom::Start(at)).and_then(|_| self.write_all(buf)).and_then(|()| self.sync_all()).map_err(|e| format!("FILE_HEADER: cannot write the key header: {e}"))
    }
}

/// An open connection's main database, through its own file handle.
struct OpenDb(std::ptr::NonNull<ffi::sqlite3>);

impl OpenDb {
    fn io(&mut self, write: bool, at: u64, buf: *mut c_void, len: usize) -> Result<(), String> {
        let mut io = HeaderIo {
            write,
            offset: i64::try_from(at).map_err(|e| e.to_string())?,
            len: c_int::try_from(len).map_err(|e| e.to_string())?,
            buf,
        };
        // SAFETY: the handle is a live connection (held locked by the caller), `io` and `buf` live
        // for the call, and the VFS reads or writes exactly `len` bytes of `buf`.
        let rc = unsafe { ffi::sqlite3_file_control(self.0.as_ptr(), c"main".as_ptr(), FCNTL_HEADER_IO, std::ptr::addr_of_mut!(io).cast::<c_void>()) };
        match rc {
            ffi::SQLITE_OK => Ok(()),
            ffi::SQLITE_NOTFOUND => Err("FILE_NOT_ENCRYPTED: this company file is not encrypted".into()),
            rc => Err(format!("FILE_HEADER: the key header could not be {} ({rc})", if write { "written" } else { "read" })),
        }
    }
}

impl Store for OpenDb {
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), String> {
        let len = buf.len();
        self.io(false, at, buf.as_mut_ptr().cast::<c_void>(), len)
    }

    fn write_at(&mut self, at: u64, buf: &[u8]) -> Result<(), String> {
        // The VFS only reads from `buf` when writing.
        self.io(true, at, buf.as_ptr().cast_mut().cast::<c_void>(), buf.len())
    }
}

/// Whether `path` is an encrypted company file (starts with the signature). For files not open in
/// this process (see the module notes).
pub fn is_encrypted(path: &Path) -> bool {
    File::open(path).is_ok_and(|mut f| has_signature(&mut f))
}

fn has_signature(store: &mut impl Store) -> bool {
    let mut head = [0u8; 16];
    store.read_at(0, &mut head).is_ok() && &head == SIGNATURE
}

/// One copy: (sequence number, header), or None when it is missing, torn or damaged.
fn read_copy(store: &mut impl Store, at: u64) -> Option<(u64, Header)> {
    let mut bytes = vec![0u8; COPY_LEN as usize];
    store.read_at(at, &mut bytes).ok()?;
    if &bytes[..8] != MAGIC {
        return None;
    }
    let sequence = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
    let len = usize::try_from(u32::from_be_bytes(bytes[16..20].try_into().ok()?)).ok()?;
    if len > MAX_BODY {
        return None;
    }
    let end = PREFIX_LEN + len;
    let hash: [u8; HASH_LEN] = Sha256::digest(&bytes[..end]).into();
    if hash[..] != bytes[end..end + HASH_LEN] {
        return None;
    }
    let header = serde_json::from_slice(&bytes[PREFIX_LEN..end]).ok()?;
    Some((sequence, header))
}

fn read_both(store: &mut impl Store) -> [Option<(u64, Header)>; 2] {
    [read_copy(store, COPY_A), read_copy(store, COPY_B)]
}

const DAMAGED: &str = "FILE_HEADER_DAMAGED: the key header of this company file is damaged; restore a backup";
const NOT_ENCRYPTED: &str = "FILE_NOT_ENCRYPTED: this company file is not encrypted";

fn read_from(store: &mut impl Store) -> Result<Header, String> {
    if !has_signature(store) {
        return Err(NOT_ENCRYPTED.into());
    }
    read_both(store).into_iter().flatten().max_by_key(|(sequence, _)| *sequence).map(|(_, header)| header).ok_or_else(|| DAMAGED.into())
}

/// Writes over the older copy, so the newer one survives a crash part-way through.
fn write_to(store: &mut impl Store, header: &Header) -> Result<(), String> {
    if !has_signature(store) {
        return Err(NOT_ENCRYPTED.into());
    }
    let [a, b] = read_both(store);
    let (newest, target) = match (&a, &b) {
        (Some((sa, _)), Some((sb, _))) if sb > sa => (*sb, COPY_A),
        (Some((sa, _)), _) => (*sa, COPY_B),
        (None, Some((sb, _))) => (*sb, COPY_A),
        (None, None) => return Err(DAMAGED.into()),
    };
    store.write_at(target, &encode(newest + 1, header)?)
}

/// The newest whole copy of the header of an encrypted file not open in this process.
pub fn read(path: &Path) -> Result<Header, String> {
    let mut file = File::open(path).map_err(|e| format!("FILE_HEADER: cannot read {}: {e}", path.display()))?;
    read_from(&mut file)
}

/// Replaces the header of an encrypted file not open in this process.
// For files not open in this process (kept for tools and tests; the app writes through `write_open`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn write(path: &Path, header: &Header) -> Result<(), String> {
    let mut file = OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("FILE_HEADER: cannot open {}: {e}", path.display()))?;
    write_to(&mut file, header)
}

/// The header of the encrypted database `conn` has open, read through its own file handle.
pub async fn read_open(conn: &mut SqliteConnection) -> Result<Header, String> {
    let mut handle = conn.lock_handle().await.map_err(|e| e.to_string())?;
    read_from(&mut OpenDb(handle.as_raw_handle()))
}

/// Replaces the header of the encrypted database `conn` has open (a password or slot changed),
/// through its own file handle, so SQLite's locks stay in place.
pub async fn write_open(conn: &mut SqliteConnection, header: &Header) -> Result<(), String> {
    let mut handle = conn.lock_handle().await.map_err(|e| e.to_string())?;
    write_to(&mut OpenDb(handle.as_raw_handle()), header)
}

fn encode(sequence: u64, header: &Header) -> Result<Vec<u8>, String> {
    let body = serde_json::to_vec(header).map_err(|e| e.to_string())?;
    if body.len() > MAX_BODY {
        return Err(format!("FILE_HEADER_FULL: too many key slots for the file header ({} bytes)", body.len()));
    }
    let mut bytes = Vec::with_capacity(COPY_LEN as usize);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.extend_from_slice(&u32::try_from(body.len()).map_err(|e| e.to_string())?.to_be_bytes());
    bytes.extend_from_slice(&body);
    let hash: [u8; HASH_LEN] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    bytes.resize(COPY_LEN as usize, 0);
    Ok(bytes)
}

/// Starts a new encrypted file at `path` (which must not exist): the signature and the header in
/// both copies, and nothing else. Open it through the VFS with the page key to create the database.
pub fn create(path: &Path, header: &Header) -> Result<(), String> {
    let copy = encode(1, header)?;
    let mut bytes = vec![0u8; usize::try_from(HEADER_LEN).map_err(|e| e.to_string())?];
    bytes[..16].copy_from_slice(SIGNATURE);
    for at in [COPY_A, COPY_B] {
        let at = usize::try_from(at).map_err(|e| e.to_string())?;
        bytes[at..at + copy.len()].copy_from_slice(&copy);
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("FILE_HEADER: cannot create {}: {e}", path.display()))?;
    file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| format!("FILE_HEADER: cannot write {}: {e}", path.display()))?;
    // The new name must survive a crash too (Windows has no directory handle to sync).
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        File::open(dir).and_then(|d| d.sync_all()).map_err(|e| format!("FILE_HEADER: cannot sync {}: {e}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
