//! The page key of an encrypted company file and the header slots that unlock it (gap R1, phase 3a
//! of `docs/plans/encryption-at-rest.md`).
//!
//! SQLCipher encrypts every page with one random 256-bit **page key**. It is never stored as is:
//! the key header ([`super::header`]) holds one slot per user, sealing it under a key Argon2id
//! derives from that user's password, and one slot for the file's recovery key. Every user who may
//! sign in has a slot (not only those who see sensitive data, as with the field key in
//! [`crate::crypto`]), because nobody can read the users table until the page key is unlocked.
//!
//! Each slot is AES-256-GCM, bound to the file's id, the slot's kind and its user, so a slot copied
//! from another file or another user doesn't open. Argon2id comes from
//! [`crate::crypto::derive_backup_key`], which bounds the cost a damaged header can ask for and reads
//! a recovery key however it is typed.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use zeroize::Zeroizing;

use super::header::{Header, HeaderSlot};
use crate::crypto::{self, KdfCost};

const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
pub const PASSWORD: &str = "PASSWORD";
pub const RECOVERY: &str = "RECOVERY";

/// The unlocked page key. Wiped from memory when dropped; never printed.
pub struct PageKey(Zeroizing<[u8; KEY_LEN]>);

impl std::fmt::Debug for PageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageKey(..)")
    }
}

impl PageKey {
    /// A new random key, for a file being encrypted ([`super::whole_file::encrypt`]).
    pub fn generate() -> Result<Self, String> {
        Ok(PageKey(Zeroizing::new(crypto::random::<KEY_LEN>()?)))
    }

    /// A fixed key, for tests.
    #[cfg(test)]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        PageKey(Zeroizing::new(bytes))
    }

    pub fn bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

/// A new random id for a file being encrypted.
pub fn new_file_id() -> Result<String, String> {
    Ok(crypto::random::<16>()?.iter().map(|b| format!("{b:02x}")).collect())
}

fn aad(file_id: &str, kind: &str, user_id: Option<i64>) -> Vec<u8> {
    format!("localbooks page slot v1\0{file_id}\0{kind}\0{}", user_id.map(|u| u.to_string()).unwrap_or_default()).into_bytes()
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

async fn seal(key: &PageKey, file_id: &str, kind: &str, user_id: Option<i64>, username: Option<String>, secret: &str) -> Result<HeaderSlot, String> {
    let cost = KdfCost::for_new_keys();
    let salt: [u8; SALT_LEN] = crypto::random()?;
    let nonce: [u8; NONCE_LEN] = crypto::random()?;
    let slot_key = crypto::derive_backup_key(secret, salt, cost).await?;
    let wrapped = Aes256Gcm::new_from_slice(&slot_key[..])
        .map_err(|_| "CRYPTO_ENCRYPT: bad slot key".to_string())?
        .encrypt(&Nonce::from(nonce), Payload { msg: &key.0[..], aad: &aad(file_id, kind, user_id) })
        .map_err(|_| "CRYPTO_ENCRYPT: could not protect the page key".to_string())?;
    Ok(HeaderSlot {
        kind: kind.into(),
        user_id,
        username,
        memory_kib: cost.memory_kib,
        iterations: cost.iterations,
        parallelism: cost.parallelism,
        salt: b64().encode(salt),
        nonce: b64().encode(nonce),
        wrapped_key: b64().encode(wrapped),
    })
}

/// A slot that opens the page key with `user_id`'s password.
pub async fn password_slot(key: &PageKey, file_id: &str, user_id: i64, username: &str, password: &str) -> Result<HeaderSlot, String> {
    seal(key, file_id, PASSWORD, Some(user_id), Some(username.to_string()), password).await
}

/// The slot that opens the page key with the file's recovery key.
pub async fn recovery_slot(key: &PageKey, file_id: &str, recovery_key: &str) -> Result<HeaderSlot, String> {
    seal(key, file_id, RECOVERY, None, None, recovery_key).await
}

/// Opens one slot; None when the secret is wrong or the slot was altered or moved.
async fn open(slot: &HeaderSlot, file_id: &str, secret: &str) -> Result<Option<PageKey>, String> {
    let damaged = || "FILE_HEADER_DAMAGED: a key slot of this company file is damaged".to_string();
    let salt: [u8; SALT_LEN] = b64().decode(&slot.salt).ok().and_then(|v| v.try_into().ok()).ok_or_else(damaged)?;
    let nonce: [u8; NONCE_LEN] = b64().decode(&slot.nonce).ok().and_then(|v| v.try_into().ok()).ok_or_else(damaged)?;
    let wrapped = b64().decode(&slot.wrapped_key).map_err(|_| damaged())?;
    let cost = KdfCost { memory_kib: slot.memory_kib, iterations: slot.iterations, parallelism: slot.parallelism };
    let slot_key = crypto::derive_backup_key(secret, salt, cost).await?;
    let Ok(plain) = Aes256Gcm::new_from_slice(&slot_key[..])
        .map_err(|_| damaged())?
        .decrypt(&Nonce::from(nonce), Payload { msg: &wrapped, aad: &aad(file_id, &slot.kind, slot.user_id) })
    else {
        return Ok(None);
    };
    let plain = Zeroizing::new(plain);
    let key: [u8; KEY_LEN] = plain.as_slice().try_into().map_err(|_| damaged())?;
    Ok(Some(PageKey(Zeroizing::new(key))))
}

/// Whether `secret` opens `slot` of the file `file_id`.
pub async fn opens(slot: &HeaderSlot, file_id: &str, secret: &str) -> Result<bool, String> {
    Ok(open(slot, file_id, secret).await?.is_some())
}

/// Unlocks the page key with a user's password. The user name is compared without case, as the
/// users table does. An unknown name does the same Argon2id work as a wrong password, so the time
/// taken doesn't reveal which names exist. Returns the key and the id of the user whose slot opened.
pub async fn unlock_with_password(header: &Header, username: &str, password: &str) -> Result<Option<(PageKey, i64)>, String> {
    let wanted = username.trim().to_lowercase();
    let mut tried = false;
    // Newest first: a user can briefly have two slots while their password changes.
    for slot in header.slots.iter().rev().filter(|s| s.kind == PASSWORD && s.username.as_deref().is_some_and(|n| n.trim().to_lowercase() == wanted)) {
        tried = true;
        if let (Some(key), Some(user_id)) = (open(slot, &header.file_id, password).await?, slot.user_id) {
            return Ok(Some((key, user_id)));
        }
    }
    if !tried {
        let _ = crypto::derive_backup_key(password, [0; SALT_LEN], KdfCost::for_new_keys()).await;
    }
    Ok(None)
}

/// Unlocks the page key with the file's recovery key.
pub async fn unlock_with_recovery_key(header: &Header, recovery_key: &str) -> Result<Option<PageKey>, String> {
    for slot in header.slots.iter().filter(|s| s.kind == RECOVERY) {
        if let Some(key) = open(slot, &header.file_id, recovery_key).await? {
            return Ok(Some(key));
        }
    }
    Ok(None)
}

/// `header` with `user_id`'s slots replaced by `slot` (a new password, or the user's first slot).
pub fn with_user_slot(header: &Header, user_id: i64, slot: HeaderSlot) -> Header {
    let mut next = header.clone();
    next.slots.retain(|s| s.user_id != Some(user_id));
    next.slots.push(slot);
    next
}

/// `header` with `slot` added beside `user_id`'s current slots: while a password change is being
/// saved, either password opens the file, so a crash between the header and the users table can't
/// lock the user out. [`with_user_slot`] or [`without_user`] tidies up afterwards.
pub fn adding_user_slot(header: &Header, slot: HeaderSlot) -> Header {
    let mut next = header.clone();
    next.slots.push(slot);
    next
}

/// `header` without any slot for `user_id` (a user turned off can no longer open the file).
pub fn without_user(header: &Header, user_id: i64) -> Header {
    let mut next = header.clone();
    next.slots.retain(|s| s.user_id != Some(user_id));
    next
}

/// `header` with its recovery slot replaced.
pub fn with_recovery_slot(header: &Header, slot: HeaderSlot) -> Header {
    let mut next = header.clone();
    next.slots.retain(|s| s.kind != RECOVERY);
    next.slots.push(slot);
    next
}

#[cfg(test)]
mod tests;
