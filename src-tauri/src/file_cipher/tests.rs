use super::*;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

const KEY: &str = "\"x'2DD29CA851E7B56E4697B0E1F08507293D761A05CE4D1B628663F411A8086D99'\"";
const MARKER: &[u8] = b"PLAINTEXT-CUSTOMER-NAME";

fn options(path: &std::path::Path, key: Option<&str>) -> SqliteConnectOptions {
    let options = SqliteConnectOptions::new().filename(path).create_if_missing(true).journal_mode(SqliteJournalMode::Wal).pragma("secure_delete", "ON");
    match key {
        Some(key) => options.pragma("key", key.to_string()),
        None => options,
    }
}

/// Writes one row holding [`MARKER`] and closes the file (which checkpoints the WAL into it).
async fn write(path: &std::path::Path, key: Option<&str>) {
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options(path, key)).await.unwrap();
    sqlx::query("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO customers (name) VALUES (?1)").bind(String::from_utf8(MARKER.to_vec()).unwrap()).execute(&pool).await.unwrap();
    pool.close().await;
}

/// A fresh folder for one test, removed first if a crashed run left it behind.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-file-cipher-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test]
async fn the_sqlite_library_is_sqlcipher() {
    let pool = crate::db::test_pool().await;
    let version = cipher_version(&pool).await.expect("SQLCipher is linked");
    assert!(version.starts_with('4'), "SQLCipher 4: {version}");
}

#[tokio::test]
async fn a_file_without_a_key_is_plain_sqlite_as_before() {
    let dir = scratch("plain");
    let path = dir.join("plain.db");
    write(&path, None).await;
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    assert!(contains(&bytes, MARKER), "plain files are unchanged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_keyed_file_shows_nothing_readable_and_opens_only_with_its_key() {
    let dir = scratch("keyed");
    let path = dir.join("encrypted.db");
    write(&path, Some(KEY)).await;
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3"), "the header is encrypted too");
    assert!(!contains(&bytes, MARKER), "no plaintext in the file");

    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options(&path, Some(KEY))).await.unwrap();
    let name: String = sqlx::query_scalar("SELECT name FROM customers").fetch_one(&pool).await.unwrap();
    assert_eq!(name.as_bytes(), MARKER);
    pool.close().await;

    let wrong = "\"x'0000000000000000000000000000000000000000000000000000000000000000'\"";
    for key in [None, Some(wrong)] {
        let opened = SqlitePoolOptions::new().max_connections(1).connect_with(options(&path, key)).await;
        let read = match opened {
            Ok(pool) => sqlx::query("SELECT name FROM customers").fetch_one(&pool).await.map(|_| ()),
            Err(e) => Err(e),
        };
        assert!(read.unwrap_err().to_string().contains("not a database"), "key {key:?} is refused");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// SQLCipher 4.5.7 is built on SQLite 3.45.3, which predates digit separators in SQL numbers
/// (`12_500`, SQLite 3.46). Rust allows them, so a query string written like Rust code fails only
/// when it runs. No string literal that reads as SQL may hold one, tested or not, and neither may a
/// migration outside its comments.
#[test]
fn sql_never_uses_digit_separators() {
    let number = regex::Regex::new(r"(?:^|[^\w.])\d+(?:_\d+)+(?:$|[^\w])").unwrap();
    let sql = regex::Regex::new(r"(?i)\b(select|insert|update|delete|values|where|create|set)\b").unwrap();
    let literal = regex::Regex::new(r#""((?:[^"\\]|\\.)*)""#).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("file_cipher/tests.rs") {
                for (i, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    if literal.captures_iter(line).any(|c| number.is_match(&c[1]) && sql.is_match(&c[1])) {
                        found.push(format!("{}:{}", path.strip_prefix(root).unwrap().display(), i + 1));
                    }
                }
            }
        }
    }
    for entry in std::fs::read_dir(root.join("migrations")).unwrap() {
        let path = entry.unwrap().path();
        for (i, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
            let code = line.split("--").next().unwrap_or_default();
            if number.is_match(code) {
                found.push(format!("{}:{}", path.strip_prefix(root).unwrap().display(), i + 1));
            }
        }
    }
    assert!(found.is_empty(), "write these numbers without underscores (SQLite 3.45 rejects 12_500):\n{}", found.join("\n"));
}
