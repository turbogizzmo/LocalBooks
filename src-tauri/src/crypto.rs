//! Encryption of sensitive fields, such as vendor tax IDs, with keys kept in the company file (see
//! migration 0042).
//!
//! One random 256-bit data key per company file encrypts those fields with AES-256-GCM. Each value is
//! stored as `lb1:<base64 of nonce, ciphertext and tag>` and authenticated together with its table,
//! column, row and the file's key id, so it cannot be moved to another row or into another file.
//!
//! The data key is never stored. Key slots hold it encrypted with a key derived by Argon2id from a
//! user's password (one slot per user) or from the file's recovery key (one slot per file), which is
//! shown to the owner once and replaces a vendor's password-reset service. Unlocking any slot gives
//! the same data key, so changing a password re-encrypts one slot and no field.
//!
//! The functions take a connection so callers can combine them with their own writes in one database
//! transaction.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use sqlx::SqliteConnection;
use zeroize::Zeroizing;

use crate::ledger::db_err;

const FIELD_PREFIX: &str = "lb1:";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const SALT_LEN: usize = 16;
const RECOVERY_BYTES: usize = 20;
/// Crockford base32: no I, L, O or U, so a recovery key read aloud or typed from paper is unambiguous.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 1024;

/// Argon2id cost of a key slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

/// 64 MiB and 3 passes: about 0.16 s per unlock on an Apple-silicon Mac, above OWASP's Argon2id minimum
/// (19 MiB, 2 passes).
const PRODUCTION_COST: Cost = Cost { memory_kib: 64 * 1024, iterations: 3, parallelism: 1 };

/// New slots use this cost; tests use Argon2's minimum so they stay fast. Unlocking always uses the
/// cost stored with the slot.
#[cfg(not(test))]
const COST: Cost = PRODUCTION_COST;
#[cfg(test)]
const COST: Cost = Cost { memory_kib: 8, iterations: 1, parallelism: 1 };

/// The company file's data key, unlocked. Wiped from memory when dropped.
pub struct DataKey {
    key: Zeroizing<[u8; KEY_LEN]>,
    key_id: String,
}

// Debug output never shows the key.
impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataKey").field("key_id", &self.key_id).finish_non_exhaustive()
    }
}

impl DataKey {
    #[cfg(test)]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.key[..]).expect("a 32-byte key")
    }

    /// Encrypts `plain` for `table.column` of row `row_id`.
    pub fn encrypt_field(&self, table: &str, column: &str, row_id: i64, plain: &str) -> Result<String, String> {
        let nonce: [u8; NONCE_LEN] = random()?;
        let aad = field_aad(&self.key_id, table, column, row_id);
        let sealed = self
            .cipher()
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain.as_bytes(), aad: &aad })
            .map_err(|_| "CRYPTO_ENCRYPT: could not encrypt the value".to_string())?;
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&sealed);
        Ok(format!("{FIELD_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(raw)))
    }

    /// Decrypts a value [`encrypt_field`](Self::encrypt_field) stored for `table.column` of `row_id`.
    pub fn decrypt_field(&self, table: &str, column: &str, row_id: i64, stored: &str) -> Result<String, String> {
        let unreadable = || format!("FIELD_UNREADABLE: {table}.{column} of row {row_id} does not decrypt with this company file's key");
        let encoded = stored.strip_prefix(FIELD_PREFIX).ok_or_else(|| format!("FIELD_NOT_ENCRYPTED: {table}.{column} of row {row_id} is not encrypted"))?;
        let raw = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| unreadable())?;
        if raw.len() < NONCE_LEN + TAG_LEN {
            return Err(unreadable());
        }
        let (nonce, sealed) = raw.split_at(NONCE_LEN);
        let aad = field_aad(&self.key_id, table, column, row_id);
        let plain = Zeroizing::new(
            self.cipher().decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: &aad }).map_err(|_| unreadable())?,
        );
        String::from_utf8(plain.to_vec()).map_err(|_| unreadable())
    }
}

/// Whether a stored value is an encrypted field (rather than plain text written before encryption).
pub fn is_encrypted(stored: &str) -> bool {
    stored.starts_with(FIELD_PREFIX)
}

/// A recovery key, in its printable form (`XXXX-XXXX-…`, eight groups). Wiped from memory when dropped.
pub struct RecoveryKey(Zeroizing<String>);

impl RecoveryKey {
    fn generate() -> Result<(RecoveryKey, Zeroizing<[u8; RECOVERY_BYTES]>), String> {
        let bytes = Zeroizing::new(random::<RECOVERY_BYTES>()?);
        let text = encode_base32(&bytes[..]);
        let groups: Vec<&str> = (0..text.len()).step_by(4).map(|i| &text[i..i + 4]).collect();
        Ok((RecoveryKey(Zeroizing::new(groups.join("-"))), bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(..)")
    }
}

/// Reads a recovery key as typed: case, spaces and dashes do not matter, and O, I and L are read as
/// the digits they look like.
fn parse_recovery_key(text: &str) -> Result<Zeroizing<[u8; RECOVERY_BYTES]>, String> {
    let bad = || "KEY_RECOVERY_FORMAT: a recovery key has 32 letters and digits, in eight groups of four".to_string();
    let mut bits: u64 = 0;
    let (mut count, mut out, mut filled) = (0u32, Zeroizing::new([0u8; RECOVERY_BYTES]), 0usize);
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let value = CROCKFORD.iter().position(|&a| a as char == c).ok_or_else(bad)? as u64;
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            if filled == RECOVERY_BYTES {
                return Err(bad());
            }
            out[filled] = (bits >> count) as u8;
            filled += 1;
        }
    }
    if filled != RECOVERY_BYTES || count != 0 {
        return Err(bad());
    }
    Ok(out)
}

fn encode_base32(bytes: &[u8]) -> String {
    let (mut bits, mut count, mut out) = (0u64, 0u32, String::new());
    for &b in bytes {
        bits = (bits << 8) | b as u64;
        count += 8;
        while count >= 5 {
            count -= 5;
            out.push(CROCKFORD[((bits >> count) & 31) as usize] as char);
        }
    }
    if count > 0 {
        out.push(CROCKFORD[((bits << (5 - count)) & 31) as usize] as char);
    }
    out
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("CRYPTO_RANDOM: the system's random number generator failed ({e})"))?;
    Ok(bytes)
}

fn field_aad(key_id: &str, table: &str, column: &str, row_id: i64) -> Vec<u8> {
    format!("localbooks field v1\0{key_id}\0{table}.{column}\0{row_id}").into_bytes()
}

fn slot_aad(key_id: &str, kind: &str, user_id: Option<i64>) -> Vec<u8> {
    format!("localbooks key slot v1\0{key_id}\0{kind}\0{}", user_id.map(|u| u.to_string()).unwrap_or_default()).into_bytes()
}

/// Derives a slot's key from a password or recovery key. Argon2id is slow by design, so it runs off
/// the async workers. The cost comes from the company file, so it is bounded: a tampered file must
/// not be able to ask for gigabytes of memory or minutes of work.
async fn derive(secret: Zeroizing<Vec<u8>>, salt: [u8; SALT_LEN], cost: Cost) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    if !(8..=1024 * 1024).contains(&cost.memory_kib) || !(1..=16).contains(&cost.iterations) || !(1..=8).contains(&cost.parallelism) {
        return Err("CRYPTO_KDF: unusable key slot cost".into());
    }
    tokio::task::spawn_blocking(move || {
        let params = Params::new(cost.memory_kib, cost.iterations, cost.parallelism, Some(KEY_LEN))
            .map_err(|e| format!("CRYPTO_KDF: unusable key slot cost ({e})"))?;
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(&secret, &salt, &mut out[..])
            .map_err(|e| format!("CRYPTO_KDF: {e}"))?;
        Ok(out)
    })
    .await
    .map_err(|e| format!("CRYPTO_KDF: {e}"))?
}

async fn key_id(conn: &mut SqliteConnection) -> Result<String, String> {
    sqlx::query_scalar("SELECT key_id FROM data_key WHERE id = 1")
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "DATA_KEY_MISSING: sensitive-data protection is not turned on for this company file".to_string())
}

/// Whether this company file has a data key (sensitive-data protection is on).
pub async fn is_protected(conn: &mut SqliteConnection) -> Result<bool, String> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_key").fetch_one(&mut *conn).await.map_err(db_err)?;
    Ok(n > 0)
}

/// Turns on sensitive-data protection: creates the data key and its recovery slot. The recovery key
/// is returned once, to show the owner; it is not kept anywhere.
pub async fn create(conn: &mut SqliteConnection) -> Result<(DataKey, RecoveryKey), String> {
    if is_protected(&mut *conn).await? {
        return Err("DATA_KEY_EXISTS: sensitive-data protection is already on for this company file".into());
    }
    let id: [u8; 16] = random()?;
    let key_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO data_key (id, key_id) VALUES (1, ?1)").bind(&key_id).execute(&mut *conn).await.map_err(db_err)?;
    let key = DataKey { key: Zeroizing::new(random()?), key_id };
    let recovery = replace_recovery_key(&mut *conn, &key).await?;
    Ok((key, recovery))
}

/// The password rules: at least [`MIN_PASSWORD_CHARS`] characters, nothing else.
pub fn check_password_rules(password: &str) -> Result<(), String> {
    let chars = password.chars().count();
    if chars < MIN_PASSWORD_CHARS {
        return Err(format!("PASSWORD_TOO_SHORT: use at least {MIN_PASSWORD_CHARS} characters"));
    }
    if chars > MAX_PASSWORD_CHARS {
        return Err(format!("PASSWORD_TOO_LONG: use at most {MAX_PASSWORD_CHARS} characters"));
    }
    Ok(())
}

/// A verifier for signing in with `password`: `argon2id$<memory KiB>$<iterations>$<parallelism>$<salt>$<hash>`.
pub async fn password_verifier(password: &str) -> Result<String, String> {
    let salt: [u8; SALT_LEN] = random()?;
    let hash = derive(Zeroizing::new(password.as_bytes().to_vec()), salt, COST).await?;
    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD;
    Ok(format!("argon2id${}${}${}${}${}", COST.memory_kib, COST.iterations, COST.parallelism, b64.encode(salt), b64.encode(&hash[..])))
}

/// Whether `password` matches a [`password_verifier`].
pub async fn verify_password(password: &str, verifier: &str) -> Result<bool, String> {
    let damaged = || "CRYPTO_VERIFIER: damaged password verifier".to_string();
    let parts: Vec<&str> = verifier.split('$').collect();
    let ["argon2id", m, t, p, salt, hash] = parts[..] else { return Err(damaged()) };
    let number = |s: &str| s.parse::<u32>().map_err(|_| damaged());
    let cost = Cost { memory_kib: number(m)?, iterations: number(t)?, parallelism: number(p)? };
    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD;
    let salt: [u8; SALT_LEN] = b64.decode(salt).map_err(|_| damaged())?.try_into().map_err(|_| damaged())?;
    let expected = b64.decode(hash).map_err(|_| damaged())?;
    let actual = derive(Zeroizing::new(password.as_bytes().to_vec()), salt, cost).await?;
    // Compared in constant time.
    Ok(expected.len() == KEY_LEN && expected.iter().zip(actual.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0)
}

/// Gives `user_id` a slot opened by `password`, replacing any slot they had.
pub async fn set_password(conn: &mut SqliteConnection, key: &DataKey, user_id: i64, password: &str) -> Result<(), String> {
    check_password_rules(password)?;
    sqlx::query("DELETE FROM key_slots WHERE user_id = ?1").bind(user_id).execute(&mut *conn).await.map_err(db_err)?;
    add_slot(conn, key, "PASSWORD", Some(user_id), Zeroizing::new(password.as_bytes().to_vec()), COST).await
}

/// Issues a new recovery key; the previous one stops working.
pub async fn replace_recovery_key(conn: &mut SqliteConnection, key: &DataKey) -> Result<RecoveryKey, String> {
    let (recovery, bytes) = RecoveryKey::generate()?;
    sqlx::query("DELETE FROM key_slots WHERE kind = 'RECOVERY'").execute(&mut *conn).await.map_err(db_err)?;
    add_slot(conn, key, "RECOVERY", None, Zeroizing::new(bytes.to_vec()), COST).await?;
    Ok(recovery)
}

async fn add_slot(conn: &mut SqliteConnection, key: &DataKey, kind: &str, user_id: Option<i64>, secret: Zeroizing<Vec<u8>>, cost: Cost) -> Result<(), String> {
    let salt: [u8; SALT_LEN] = random()?;
    let nonce: [u8; NONCE_LEN] = random()?;
    let slot_key = derive(secret, salt, cost).await?;
    let aad = slot_aad(&key.key_id, kind, user_id);
    let wrapped = Aes256Gcm::new_from_slice(&slot_key[..])
        .expect("a 32-byte key")
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: &key.key[..], aad: &aad })
        .map_err(|_| "CRYPTO_ENCRYPT: could not protect the data key".to_string())?;
    sqlx::query(
        "INSERT INTO key_slots (kind, user_id, memory_kib, iterations, parallelism, salt, nonce, wrapped_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(kind)
    .bind(user_id)
    .bind(cost.memory_kib as i64)
    .bind(cost.iterations as i64)
    .bind(cost.parallelism as i64)
    .bind(&salt[..])
    .bind(&nonce[..])
    .bind(wrapped)
    .execute(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Slot {
    memory_kib: i64,
    iterations: i64,
    parallelism: i64,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    wrapped_key: Vec<u8>,
}

/// Opens one slot; `None` when the secret is wrong (or the slot was altered or moved).
async fn open_slot(key_id: String, kind: &str, user_id: Option<i64>, slot: Slot, secret: Zeroizing<Vec<u8>>) -> Result<Option<DataKey>, String> {
    let as_u32 = |v: i64| u32::try_from(v).map_err(|_| "CRYPTO_KDF: unusable key slot cost".to_string());
    let cost = Cost { memory_kib: as_u32(slot.memory_kib)?, iterations: as_u32(slot.iterations)?, parallelism: as_u32(slot.parallelism)? };
    let salt: [u8; SALT_LEN] = slot.salt.as_slice().try_into().map_err(|_| "CRYPTO_SLOT: damaged key slot".to_string())?;
    let slot_key = derive(secret, salt, cost).await?;
    let aad = slot_aad(&key_id, kind, user_id);
    let Ok(plain) = Aes256Gcm::new_from_slice(&slot_key[..])
        .expect("a 32-byte key")
        .decrypt(Nonce::from_slice(&slot.nonce), Payload { msg: &slot.wrapped_key, aad: &aad })
    else {
        return Ok(None);
    };
    let plain = Zeroizing::new(plain);
    let key: [u8; KEY_LEN] = plain.as_slice().try_into().map_err(|_| "CRYPTO_SLOT: damaged key slot".to_string())?;
    Ok(Some(DataKey { key: Zeroizing::new(key), key_id }))
}

/// Unlocks the data key with `user_id`'s password.
pub async fn unlock_with_password(conn: &mut SqliteConnection, user_id: i64, password: &str) -> Result<DataKey, String> {
    let key_id = key_id(&mut *conn).await?;
    let slot: Slot = sqlx::query_as("SELECT memory_kib, iterations, parallelism, salt, nonce, wrapped_key FROM key_slots WHERE user_id = ?1")
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "KEY_NO_SLOT: this user has no access to sensitive data".to_string())?;
    open_slot(key_id, "PASSWORD", Some(user_id), slot, Zeroizing::new(password.as_bytes().to_vec()))
        .await?
        .ok_or_else(|| "KEY_WRONG_PASSWORD: the password is not correct".to_string())
}

/// Unlocks the data key with the file's recovery key, as typed by the owner.
pub async fn unlock_with_recovery_key(conn: &mut SqliteConnection, recovery_key: &str) -> Result<DataKey, String> {
    let bytes = parse_recovery_key(recovery_key)?;
    let key_id = key_id(&mut *conn).await?;
    let slot: Slot = sqlx::query_as("SELECT memory_kib, iterations, parallelism, salt, nonce, wrapped_key FROM key_slots WHERE kind = 'RECOVERY'")
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "KEY_NO_RECOVERY: this company file has no recovery key".to_string())?;
    open_slot(key_id, "RECOVERY", None, slot, Zeroizing::new(bytes.to_vec()))
        .await?
        .ok_or_else(|| "KEY_WRONG_RECOVERY: that is not this company file's recovery key".to_string())
}

#[cfg(test)]
mod tests;
