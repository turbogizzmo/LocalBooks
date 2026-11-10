use super::*;
use crate::file_cipher::header::{self, Header};
use crate::file_cipher::open_options;
use sqlx::sqlite::{SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

const KEY: [u8; 32] = [7; 32];
const MARKER: &str = "PLAINTEXT-CUSTOMER-NAME";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-file-vfs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

async fn pool(path: &std::path::Path, key: Option<&[u8; 32]>, connections: u32) -> Result<SqlitePool, sqlx::Error> {
    let options = open_options(path, key).unwrap().create_if_missing(true).journal_mode(SqliteJournalMode::Wal).pragma("secure_delete", "ON");
    SqlitePoolOptions::new().max_connections(connections).connect_with(options).await
}

async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM customers").fetch_one(pool).await
}

/// Moves everything from the WAL into the database file, so the file's bytes are the whole story.
async fn checkpoint(pool: &SqlitePool) {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await.unwrap();
}

fn new_header() -> Header {
    Header { page_size: 4096, slots: Vec::new() }
}

#[test]
fn offsets_skip_the_header_and_the_lock_bytes() {
    let boundary = LOCK_START - HEADER_LEN;
    assert_eq!(real_offset(0), HEADER_LEN);
    assert_eq!(real_offset(boundary - 4096), LOCK_START - 4096, "the last page before the gap");
    assert_eq!(real_offset(boundary), LOCK_START + LOCK_SPAN, "the first page after it");
    for db in [0, 4096, boundary - 65536, boundary, boundary + 4096, 5 * LOCK_START] {
        assert_eq!(database_len(real_offset(db)), db, "the size inverts the offset at {db}");
        let real = real_offset(db);
        assert!(!(LOCK_START..LOCK_START + LOCK_SPAN).contains(&real), "no page starts in the lock bytes");
    }
    assert_eq!(database_len(0), 0);
    assert_eq!(database_len(HEADER_LEN), 0);
    assert_eq!(database_len(LOCK_START + 10), boundary, "a file ending inside the gap");
    assert!(one_side(boundary - 4096, 4096) && one_side(boundary, 65536) && !one_side(boundary - 100, 200));
    assert_eq!(HEADER_LEN % 65536, 0, "page-aligned for every page size");
    assert_eq!(LOCK_SPAN % 65536, 0);
}

#[tokio::test]
async fn an_encrypted_file_keeps_its_header_and_hides_its_data() {
    let dir = scratch("encrypted");
    let path = dir.join("books.db");
    header::create(&path, &new_header()).unwrap();
    let books = pool(&path, Some(&KEY), 4).await.unwrap();
    sqlx::query("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT").execute(&books).await.unwrap();
    for i in 0..2000 {
        sqlx::query("INSERT INTO customers (name) VALUES (?1)").bind(format!("{MARKER} {i}")).execute(&books).await.unwrap();
    }
    // Readers and the writer share the WAL through the VFS.
    let (a, b) = tokio::join!(count(&books), count(&books));
    assert_eq!((a.unwrap(), b.unwrap()), (2000, 2000));
    let wal = std::fs::read(dir.join("books.db-wal")).unwrap_or_default();
    assert!(!contains(&wal, MARKER.as_bytes()), "the WAL is encrypted too");
    // Its header stays readable, so the WAL guard can still compare page sizes.
    assert!(matches!(wal.get(..4), Some([0x37, 0x7f, 0x06, 0x82 | 0x83])), "a WAL header SQLite writes in clear");
    assert_eq!(wal.get(8..12), Some(&4096u32.to_be_bytes()[..]), "with the header's page size");
    assert!(crate::wal_guard::check(&path).is_ok());
    checkpoint(&books).await;
    books.close().await;

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(SIGNATURE));
    assert!(!contains(&bytes, MARKER.as_bytes()), "no plaintext anywhere in the file");
    assert!(!contains(&bytes, b"SQLite format 3"), "not even SQLite's own header");
    assert!(bytes.len() > usize::try_from(HEADER_LEN).unwrap());

    // The header can change while the database stays readable.
    header::write(&path, &Header { page_size: 4096, slots: vec![] }).unwrap();
    let books = pool(&path, Some(&KEY), 1).await.unwrap();
    // While it is open, the header is read and written through the connection's own handle.
    let mut conn = books.acquire().await.unwrap();
    assert_eq!(header::read_open(&mut conn).await.unwrap(), Header { page_size: 4096, slots: vec![] });
    let renamed = Header { page_size: 4096, slots: vec![] };
    header::write_open(&mut conn, &renamed).await.unwrap();
    assert_eq!(header::read_open(&mut conn).await.unwrap(), renamed);
    drop(conn);
    assert_eq!(count(&books).await.unwrap(), 2000);
    let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&books).await.unwrap();
    assert_eq!(check, "ok");
    books.close().await;

    // Without the key, or without the VFS, it is not a database.
    let no_key = match pool(&path, None, 1).await {
        Ok(p) => count(&p).await.map(|_| ()),
        Err(e) => Err(e),
    };
    assert!(no_key.unwrap_err().to_string().contains("not a database"));
    let plain_vfs = sqlx::sqlite::SqliteConnectOptions::new().filename(&path).pragma("key", "\"x'0707070707070707070707070707070707070707070707070707070707070707'\"");
    let direct = match SqlitePoolOptions::new().max_connections(1).connect_with(plain_vfs).await {
        Ok(p) => count(&p).await.map(|_| ()),
        Err(e) => Err(e),
    };
    assert!(direct.unwrap_err().to_string().contains("not a database"), "the header hides the database from plain SQLite");
    assert_eq!(header::read(&path).unwrap(), Header { page_size: 4096, slots: vec![] }, "SQLite never wrote into the header");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn plain_files_pass_through_unchanged() {
    let dir = scratch("plain");
    let path = dir.join("plain.db");
    let books = pool(&path, None, 2).await.unwrap();
    sqlx::query("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT").execute(&books).await.unwrap();
    sqlx::query("INSERT INTO customers (name) VALUES (?1)").bind(MARKER).execute(&books).await.unwrap();
    let mut conn = books.acquire().await.unwrap();
    assert!(header::read_open(&mut conn).await.unwrap_err().starts_with("FILE_NOT_ENCRYPTED"), "a plain file has no header to read");
    drop(conn);
    checkpoint(&books).await;
    books.close().await;
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"), "a plain database at byte 0, as before");
    assert!(contains(&bytes, MARKER.as_bytes()));
    assert!(!header::is_encrypted(&path));

    // And the default VFS reads what this one wrote.
    let direct = SqlitePoolOptions::new().max_connections(1).connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await.unwrap();
    assert_eq!(count(&direct).await.unwrap(), 1);
    direct.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
