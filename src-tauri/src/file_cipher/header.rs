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
//! The body (JSON) names the SQLCipher page size and holds the key slots. A slot is opaque here:
//! the slot's kind, the user it belongs to and the sealed key, as [`crate::crypto`] writes them. They
//! are needed before the database can be decrypted, so they can't live only inside it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::vfs::{HEADER_LEN, SIGNATURE};

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
    /// SQLCipher's page size (`cipher_page_size`), which the WAL guard compares with a WAL's.
    pub page_size: u32,
    pub slots: Vec<HeaderSlot>,
}

/// Whether `path` is an encrypted company file (starts with the signature).
pub fn is_encrypted(path: &Path) -> bool {
    let mut head = [0u8; 16];
    File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == SIGNATURE
}

/// One copy: (sequence number, header), or None when it is missing, torn or damaged.
fn read_copy(file: &mut File, at: u64) -> Option<(u64, Header)> {
    let mut bytes = vec![0u8; COPY_LEN as usize];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut bytes).ok()?;
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

fn read_both(file: &mut File) -> [Option<(u64, Header)>; 2] {
    [read_copy(file, COPY_A), read_copy(file, COPY_B)]
}

/// The newest whole copy of the header of an encrypted file.
pub fn read(path: &Path) -> Result<Header, String> {
    if !is_encrypted(path) {
        return Err(format!("FILE_NOT_ENCRYPTED: {} is not an encrypted company file", path.display()));
    }
    let mut file = File::open(path).map_err(|e| format!("FILE_HEADER: cannot read {}: {e}", path.display()))?;
    read_both(&mut file)
        .into_iter()
        .flatten()
        .max_by_key(|(sequence, _)| *sequence)
        .map(|(_, header)| header)
        .ok_or_else(|| format!("FILE_HEADER_DAMAGED: the key header of {} is damaged; restore a backup", path.display()))
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
    file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| format!("FILE_HEADER: cannot write {}: {e}", path.display()))
}

/// Replaces the header of an encrypted file (a password or slot changed): writes over the older
/// copy and syncs, so the newer one survives a crash in between.
pub fn write(path: &Path, header: &Header) -> Result<(), String> {
    if !is_encrypted(path) {
        return Err(format!("FILE_NOT_ENCRYPTED: {} is not an encrypted company file", path.display()));
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("FILE_HEADER: cannot open {}: {e}", path.display()))?;
    let [a, b] = read_both(&mut file);
    let (newest, target) = match (&a, &b) {
        (Some((sa, _)), Some((sb, _))) if sb > sa => (*sb, COPY_A),
        (Some((sa, _)), _) => (*sa, COPY_B),
        (None, Some((sb, _))) => (*sb, COPY_A),
        (None, None) => return Err(format!("FILE_HEADER_DAMAGED: the key header of {} is damaged; restore a backup", path.display())),
    };
    let copy = encode(newest + 1, header)?;
    file.seek(SeekFrom::Start(target))
        .and_then(|_| file.write_all(&copy))
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("FILE_HEADER: cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests;
