//! Page encryption for whole company files (gap R1, `docs/plans/encryption-at-rest.md`).
//!
//! - Phase 1: the SQLite library under sqlx is SQLCipher, built through `libsqlite3-sys`'s
//!   `bundled-sqlcipher` features (see `Cargo.toml`).
//! - Phase 2: an encrypted file is `[key header][SQLCipher database]`. [`header`] reads and writes
//!   the key header; [`vfs`] lets SQLite see the database behind it. [`open_options`] opens either
//!   kind of file.
//!
//! Nothing turns encryption on yet: every company file, backup and import is still plain SQLite,
//! read and written exactly as before.
//!
//! SQLCipher encrypts each database page (and each WAL and journal page) when a connection sets
//! `PRAGMA key` before anything else; sqlx runs a `key` pragma first for that reason. A file opened
//! without a key is ordinary SQLite.

use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

// Phase 3 opens company files through these; until then only the tests and the WAL guard do.
#[cfg_attr(not(test), allow(dead_code))]
pub mod header;
#[cfg_attr(not(test), allow(dead_code))]
pub mod vfs;

/// Connection options for `path` through the [`vfs`], with SQLCipher's `key` when the file is
/// encrypted. `page_key` is the raw 256-bit page key. A plain file opens as plain SQLite through
/// the same VFS (it passes through), so callers needn't know which kind they have.
#[cfg_attr(not(test), allow(dead_code))]
pub fn open_options(path: &Path, page_key: Option<&[u8; 32]>) -> Result<SqliteConnectOptions, String> {
    vfs::register()?;
    let options = SqliteConnectOptions::new().filename(path).vfs(vfs::VFS_NAME);
    Ok(match page_key {
        Some(key) => {
            // sqlx keeps its own copy for the pool's life; at least ours is wiped.
            let pragma = zeroize::Zeroizing::new(format!("\"x'{}'\"", key.iter().map(|b| format!("{b:02X}")).collect::<String>()));
            options.pragma("key", pragma.as_str().to_owned())
        }
        None => options,
    })
}

/// The SQLCipher version linked into this build ("4.5.7 community"), for the diagnostics bundle.
/// None if the library is plain SQLite.
pub async fn cipher_version(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar::<_, String>("PRAGMA cipher_version").fetch_optional(pool).await.ok().flatten()
}

/// The crypto library SQLCipher was built against: `commoncrypto` on macOS, `openssl` elsewhere.
/// A macOS build that reports `openssl` linked an OpenSSL other Macs may not have. SQLCipher only
/// answers on a keyed connection, so this asks a throwaway in-memory one.
pub async fn cipher_provider() -> Option<String> {
    use sqlx::{ConnectOptions, Connection};
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:").pragma("key", "'cipher provider check'");
    let mut conn = options.connect().await.ok()?;
    let provider = sqlx::query_scalar::<_, String>("PRAGMA cipher_provider").fetch_optional(&mut conn).await.ok().flatten();
    let _ = conn.close().await;
    provider
}

#[cfg(test)]
mod tests;
