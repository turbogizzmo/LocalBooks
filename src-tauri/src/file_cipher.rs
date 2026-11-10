//! Page encryption for whole company files (gap R1): the SQLite library under sqlx is SQLCipher,
//! built through `libsqlite3-sys`'s `bundled-sqlcipher` features (see `Cargo.toml`). This is phase 1
//! of `docs/plans/encryption-at-rest.md`: the library is in place, and nothing sets a key yet, so
//! every company file, backup and import is still plain SQLite, read and written exactly as before.
//!
//! SQLCipher encrypts each database page (and each WAL and journal page) when a connection sets
//! `PRAGMA key` before anything else; sqlx runs a `key` pragma first for that reason. A file opened
//! without a key is ordinary SQLite.

use sqlx::SqlitePool;

/// The SQLCipher version linked into this build ("4.5.7 community"), for the diagnostics bundle.
/// None if the library is plain SQLite.
pub async fn cipher_version(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar::<_, String>("PRAGMA cipher_version").fetch_optional(pool).await.ok().flatten()
}

#[cfg(test)]
mod tests;
