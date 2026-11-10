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

pub mod header;
pub mod keys;
pub mod user_slots;
pub mod vfs;

/// Connection options for `path` through the [`vfs`], with SQLCipher's `key` when the file is
/// encrypted. `page_key` is the raw 256-bit page key. A plain file opens as plain SQLite through
/// the same VFS (it passes through), so callers needn't know which kind they have.
pub fn open_options(path: &Path, page_key: Option<&keys::PageKey>) -> Result<SqliteConnectOptions, String> {
    vfs::register()?;
    let options = SqliteConnectOptions::new().filename(path).vfs(vfs::VFS_NAME);
    Ok(match page_key {
        Some(key) => {
            // sqlx keeps its own copy for the pool's life; at least ours is wiped.
            let pragma = zeroize::Zeroizing::new(format!("\"x'{}'\"", key.bytes().iter().map(|b| format!("{b:02X}")).collect::<String>()));
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

/// Copies the plain company file at `source` into a new encrypted file at `dest` (which must not
/// exist): writes `header`, then exports every table, index and trigger into it under `key`
/// (SQLCipher's `sqlcipher_export`). The source is only read. On failure `dest` is removed.
///
/// Test-only until turning encryption on ships (phase 3b): this is the one way to make an encrypted
/// company file, so nothing outside the tests can make one yet. (A backup of an encrypted file is
/// encrypted too, but that needs one to exist first.)
#[cfg(test)]
pub async fn encrypt_into(source: &Path, dest: &Path, key: &keys::PageKey, header: &header::Header) -> Result<(), String> {
    use sqlx::{ConnectOptions, Connection};
    header::create(dest, header)?;
    let result = async {
        // Through the VFS, so the attached file is opened behind its header.
        let mut conn = open_options(source, None)?.connect().await.map_err(|e| format!("FILE_ENCRYPT: cannot open {}: {e}", source.display()))?;
        let hex: zeroize::Zeroizing<String> = zeroize::Zeroizing::new(key.bytes().iter().map(|b| format!("{b:02X}")).collect());
        let exported = async {
            sqlx::query(&format!("ATTACH DATABASE ?1 AS encrypted KEY \"x'{}'\"", hex.as_str()))
                .bind(dest.to_string_lossy().into_owned())
                .execute(&mut conn)
                .await?;
            sqlx::query("SELECT sqlcipher_export('encrypted')").execute(&mut conn).await?;
            sqlx::query("DETACH DATABASE encrypted").execute(&mut conn).await.map(|_| ())
        }
        .await
        .map_err(|e| format!("FILE_ENCRYPT: could not encrypt the company file: {e}"));
        let _ = conn.close().await;
        exported
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
        let _ = std::fs::remove_file(crate::wal_guard::wal_path(dest));
    }
    result
}

#[cfg(test)]
mod tests;
