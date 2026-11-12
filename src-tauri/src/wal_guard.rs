//! Refuses a SQLite file whose write-ahead log was written for a different page size.
//!
//! SQLite before 3.54 can hand back uninitialized memory when it opens a database together with a
//! `-wal` file whose page size doesn't match the database's (RUSTSEC-2026-0459). LocalBooks bundles
//! an older SQLite through sqlx (gap R24 in `docs/gaps.md`), and it opens files other people send:
//! company files, backups, and GnuCash or QuickBooks for Mac files to import. A real log always has
//! its database's page size (SQLite can't change the page size of a database in WAL mode), so a
//! mismatch only comes from a leftover or crafted file, and refusing it costs nothing.
//!
//! **Two checks, before and at use.** [`check`] reads both headers by path just before a file is
//! opened, to refuse it with a clear message. But a log can change between that check and the
//! moment SQLite reads it (another program, or a connection still closing that checkpoints it), so
//! every company file also opens through LocalBooks' VFS ([`crate::file_cipher::vfs`]), which
//! repeats the comparison **when SQLite opens the log**, reading it through the very handle SQLite
//! will use, and refuses a mismatched one there. The first check is for the message; the second is
//! what closes the gap.
//!
//! Formats, from the SQLite file format documentation (sqlite.org/fileformat2.html): the database
//! header starts with "SQLite format 3\0" and holds the page size as a big-endian 16-bit number at
//! offset 16 (1 meaning 65536); the WAL header starts with magic 0x377f0682 or 0x377f0683 and holds
//! the page size as a big-endian 32-bit number at offset 8.

use std::io::Read;
use std::path::{Path, PathBuf};

const DB_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// The first `n` bytes of `path`, or None when it can't be read or is shorter.
fn head<const N: usize>(path: &Path) -> Option<[u8; N]> {
    let mut buf = [0u8; N];
    std::fs::File::open(path).ok()?.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// A page size SQLite accepts: a power of two from 512 to 65536.
pub(crate) fn valid_page_size(size: u32) -> Option<u32> {
    ((512..=65_536).contains(&size) && size.is_power_of_two()).then_some(size)
}

/// The page size in a database header's first 18 bytes, or None when they aren't a SQLite header or
/// hold a size SQLite would reject anyway.
pub(crate) fn db_header_page_size(header: &[u8; 18]) -> Option<u32> {
    if &header[..16] != DB_MAGIC {
        return None;
    }
    valid_page_size(match u16::from_be_bytes([header[16], header[17]]) {
        1 => 65_536,
        n => u32::from(n),
    })
}

/// The page size in a log header's first 12 bytes, or None when they aren't a valid log header
/// (SQLite ignores those).
pub(crate) fn wal_header_page_size(header: &[u8; 12]) -> Option<u32> {
    let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    if magic != 0x377f_0682 && magic != 0x377f_0683 {
        return None;
    }
    valid_page_size(u32::from_be_bytes([header[8], header[9], header[10], header[11]]))
}

/// The database's page size, or None when `path` isn't (yet) a SQLite database or its header
/// holds a size SQLite would reject anyway. An encrypted company file (gap R1) starts with
/// LocalBooks' key header instead, and SQLCipher encrypts the database's own header, so its page
/// size comes from the key header.
fn db_page_size(path: &Path) -> Option<u32> {
    if crate::file_cipher::header::is_encrypted(path) {
        return crate::file_cipher::header::read(path).ok().and_then(|h| valid_page_size(h.page_size));
    }
    db_header_page_size(&head(path)?)
}

/// The log's page size, or None when there is no log or it isn't a valid one (SQLite ignores those).
fn wal_page_size(path: &Path) -> Option<u32> {
    wal_header_page_size(&head(path)?)
}

/// `db`'s log file: its name with `-wal` added, built without going through a string.
pub(crate) fn wal_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

/// Checks `db` before SQLite opens it: an error when its `-wal` file has a different page size.
pub fn check(db: &Path) -> Result<(), String> {
    let wal = wal_path(db);
    let (Some(db_size), Some(wal_size)) = (db_page_size(db), wal_page_size(&wal)) else { return Ok(()) };
    if db_size == wal_size {
        return Ok(());
    }
    let name = db.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| db.display().to_string());
    Err(format!(
        "FILE_WAL_MISMATCH: {name} has a log file next to it ({name}-wal) that doesn't belong to it, so Tillhouse won't open it. \
         Move {name}-wal out of that folder, then open {name} again"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        // The process, a counter and the time: unique even for tests started in the same instant.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lb-wal-guard-{}-{n}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn db_header(page: u16) -> Vec<u8> {
        let mut h = DB_MAGIC.to_vec();
        h.extend_from_slice(&page.to_be_bytes());
        h.resize(100, 0);
        h
    }

    fn wal_header(page: u32) -> Vec<u8> {
        let mut h = 0x377f_0682u32.to_be_bytes().to_vec();
        h.extend_from_slice(&3_007_000u32.to_be_bytes());
        h.extend_from_slice(&page.to_be_bytes());
        h.resize(32, 0);
        h
    }

    #[test]
    fn a_log_with_another_page_size_is_refused() {
        let dir = dir();
        let db = dir.join("Books.localbooks");
        std::fs::write(&db, db_header(4096)).unwrap();
        assert_eq!(check(&db), Ok(()), "no log");
        std::fs::write(wal_path(&db), wal_header(4096)).unwrap();
        assert_eq!(check(&db), Ok(()), "its own log");
        std::fs::write(wal_path(&db), wal_header(1024)).unwrap();
        let err = check(&db).unwrap_err();
        assert!(err.starts_with("FILE_WAL_MISMATCH") && err.contains("Books.localbooks-wal"), "{err}");
        // 1 in the database header means 65536.
        std::fs::write(&db, db_header(1)).unwrap();
        std::fs::write(wal_path(&db), wal_header(65_536)).unwrap();
        assert_eq!(check(&db), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_sqlite_would_ignore_or_create_are_let_through() {
        let dir = dir();
        let db = dir.join("New.localbooks");
        assert_eq!(check(&db), Ok(()), "a file not created yet");
        std::fs::write(&db, b"").unwrap();
        std::fs::write(wal_path(&db), wal_header(1024)).unwrap();
        assert_eq!(check(&db), Ok(()), "an empty database");
        std::fs::write(&db, db_header(4096)).unwrap();
        std::fs::write(wal_path(&db), b"").unwrap();
        assert_eq!(check(&db), Ok(()), "an empty log");
        std::fs::write(wal_path(&db), vec![0u8; 32]).unwrap();
        assert_eq!(check(&db), Ok(()), "not a log at all: SQLite ignores it");
        // Page sizes SQLite would reject in either header are not this check's business.
        for bad in [0u32, 256, 3000, 131_072] {
            std::fs::write(wal_path(&db), wal_header(bad)).unwrap();
            assert_eq!(check(&db), Ok(()), "log page size {bad}");
        }
        std::fs::write(&db, db_header(3000)).unwrap();
        std::fs::write(wal_path(&db), wal_header(1024)).unwrap();
        assert_eq!(check(&db), Ok(()), "database page size 3000");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real WAL-mode database and its log agree, so neither check stands in the way of SQLite's
    /// own files (every `db::open` in the test suite goes through the VFS's check too).
    #[tokio::test]
    async fn a_real_company_file_and_its_log_pass() {
        let dir = dir();
        let path = dir.join("Real.localbooks");
        let pool = crate::db::open(&path).await.unwrap();
        sqlx::query("CREATE TABLE IF NOT EXISTS wal_guard_probe (x INTEGER) STRICT").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO wal_guard_probe VALUES (1)").execute(&pool).await.unwrap();
        assert!(wal_path(&path).exists(), "the open file has its log");
        assert_eq!(check(&path), Ok(()));
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A WAL-mode database made on one connection, closed completely: nothing else in this process
    /// can touch its log afterwards. (A pool's connections close on their own threads, and one that
    /// finishes late checkpoints and removes the log, which once made the crafted-log test below
    /// pass a file whose crafted log had already been deleted.)
    async fn closed_wal_database(path: &Path) {
        use sqlx::{ConnectOptions, Connection};
        let mut conn = crate::file_cipher::open_options(path, None)
            .unwrap()
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE probe (x INTEGER) STRICT").execute(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO probe VALUES (1)").execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();
        assert_eq!(db_page_size(path), Some(4096));
    }

    /// A company file with a crafted log beside it: refused before SQLite reads it.
    #[tokio::test]
    async fn a_company_file_with_a_crafted_log_is_refused() {
        let dir = dir();
        let path = dir.join("Real.localbooks");
        closed_wal_database(&path).await;
        std::fs::write(wal_path(&path), wal_header(1024)).unwrap();
        let err = crate::db::open(&path).await.map(|_| ()).unwrap_err();
        assert!(err.starts_with("FILE_WAL_MISMATCH"), "{err}");
        assert_eq!(std::fs::read(wal_path(&path)).unwrap(), wal_header(1024), "and the log is left as it was");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gap the second check closes: the log is fine when [`check`] looks, then is swapped for a
    /// crafted one before SQLite opens it. The VFS reads the log through SQLite's own handle as SQLite
    /// opens it, so the swap is caught there, and `db::open` explains it like the first check would.
    #[tokio::test]
    async fn a_log_swapped_after_the_check_is_refused_when_sqlite_opens_it() {
        use sqlx::{ConnectOptions, Connection};
        let dir = dir();
        let path = dir.join("Swapped.localbooks");
        closed_wal_database(&path).await;
        std::fs::write(wal_path(&path), wal_header(4096)).unwrap();
        assert_eq!(check(&path), Ok(()), "the log is fine when it is checked");
        std::fs::write(wal_path(&path), wal_header(1024)).unwrap(); // ...and swapped right after

        // Straight through the VFS, with no check first: SQLite itself can't open it.
        let opened = async {
            let mut conn = crate::file_cipher::open_options(&path, None)?.connect().await.map_err(|e| e.to_string())?;
            let read: Result<i64, _> = sqlx::query_scalar("SELECT COUNT(*) FROM probe").fetch_one(&mut conn).await;
            let _ = conn.close().await;
            read.map_err(|e| e.to_string())
        }
        .await;
        assert!(opened.is_err(), "the crafted log is refused at use: {opened:?}");

        // And through the app's own open, the reason is spelled out.
        let err = crate::db::open(&path).await.map(|_| ()).unwrap_err();
        assert!(err.starts_with("FILE_WAL_MISMATCH"), "{err}");

        // Its own log again: it opens and reads.
        std::fs::remove_file(wal_path(&path)).unwrap();
        let pool = crate::db::open(&path).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM probe").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 1);
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_encrypted_file_is_checked_by_its_key_header() {
        let dir = dir().join("encrypted");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("books.db");
        let header = crate::file_cipher::header::Header { file_id: String::new(), page_size: 4096, slots: Vec::new() };
        crate::file_cipher::header::create(&db, &header).unwrap();
        std::fs::write(wal_path(&db), wal_header(4096)).unwrap();
        assert!(check(&db).is_ok(), "a log with the header's page size");
        std::fs::write(wal_path(&db), wal_header(1024)).unwrap();
        assert!(check(&db).unwrap_err().starts_with("FILE_WAL_MISMATCH"), "a log with another page size");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
