//! Help › Save Diagnostics (gap R12): a bundle the user can send to whoever helps them, saved
//! where they choose. Nothing is sent anywhere, and nothing from the books goes in it.
//!
//! LocalBooks keeps a small diagnostics log in its log folder ([`log`]): when it starts and stops,
//! which company file it opened (its name, not its contents), and any crash with where in the code
//! it happened. The bundle is a zip of:
//!
//! - `versions.txt`: LocalBooks' version, the operating system and processor, SQLite's version;
//! - `database.txt`: the company file's size, which schema migrations it has, its integrity check,
//!   and how many rows each table holds (counts only, no values);
//! - `log.txt` (and `log.old.txt`): the diagnostics log.
//!
//! What a crash report says is the code's own panic message and location; LocalBooks' messages
//! name documents by number, not by customer or amount, but the log is plain text the user can read
//! before sending it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sqlx::SqlitePool;

use crate::ledger::db_err;

/// The log (`diagnostics.log`) rolls over to `diagnostics.old.log` past this size; the bundle
/// names them `log.txt` and `log.old.txt`.
const LOG_LIMIT: u64 = 512 * 1024;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static WRITING: Mutex<()> = Mutex::new(());

fn log_path(dir: &Path) -> PathBuf {
    dir.join("diagnostics.log")
}

/// Sets the folder the log goes to, and records crashes there (keeping the default report too).
pub fn install(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    if LOG_DIR.set(dir).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info.location().map_or_else(|| "unknown place".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        log(&format!("CRASH at {place}: {message}"));
        previous(info);
    }));
}

/// Appends a line to the diagnostics log (if one is installed), with the time.
pub fn log(line: &str) {
    let Some(dir) = LOG_DIR.get() else { return };
    write_line(dir, line, LOG_LIMIT);
}

fn write_line(dir: &Path, line: &str, limit: u64) {
    let _guard = WRITING.lock();
    let path = log_path(dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > limit) {
        // The older log goes first: renaming over an existing file fails on Windows.
        let old = dir.join("diagnostics.old.log");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&path, &old);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), line.replace('\n', " "));
    }
}

/// What goes in the bundle: (name in the zip, text).
pub async fn bundle(pool: Option<&SqlitePool>, company_file: Option<&Path>, log_dir: Option<&Path>, app_version: &str) -> Result<Vec<(String, String)>, String> {
    let mut files = Vec::new();
    let mut versions = format!(
        "LocalBooks {app_version}\nOperating system: {} ({})\nProcessor: {}\n",
        std::env::consts::OS,
        std::env::consts::FAMILY,
        std::env::consts::ARCH
    );
    if let Some(pool) = pool {
        let sqlite: String = sqlx::query_scalar("SELECT sqlite_version()").fetch_one(pool).await.map_err(db_err)?;
        versions.push_str(&format!("SQLite: {sqlite}\n"));
    }
    files.push(("versions.txt".to_string(), versions));

    match pool {
        Some(pool) => {
            let mut db = String::new();
            if let Some(path) = company_file {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                db.push_str(&format!("Company file: {name}, {size} bytes\n"));
            }
            let check: Vec<String> = sqlx::query_scalar("PRAGMA quick_check").fetch_all(pool).await.map_err(db_err)?;
            db.push_str(&format!("Integrity (quick check): {}\n", check.join("; ")));
            let (pages, page_size): (i64, i64) = sqlx::query_as("SELECT (SELECT page_count FROM pragma_page_count), (SELECT page_size FROM pragma_page_size)")
                .fetch_one(pool)
                .await
                .map_err(db_err)?;
            db.push_str(&format!("Pages: {pages} of {page_size} bytes\n\nSchema migrations:\n"));
            let migrations: Vec<(i64, String, bool)> = sqlx::query_as("SELECT version, description, success FROM _sqlx_migrations ORDER BY version")
                .fetch_all(pool)
                .await
                .map_err(db_err)?;
            for (version, description, success) in migrations {
                db.push_str(&format!("  {version:04} {description}{}\n", if success { "" } else { " (FAILED)" }));
            }
            db.push_str("\nRows per table (counts only):\n");
            let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .fetch_all(pool)
                .await
                .map_err(db_err)?;
            for table in tables {
                let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\""))).fetch_one(pool).await.map_err(db_err)?;
                db.push_str(&format!("  {table}: {count}\n"));
            }
            files.push(("database.txt".to_string(), db));
        }
        None => files.push(("database.txt".to_string(), "No company file is open.\n".to_string())),
    }

    if let Some(dir) = log_dir {
        for (name, file) in [("log.txt", "diagnostics.log"), ("log.old.txt", "diagnostics.old.log")] {
            if let Ok(text) = std::fs::read_to_string(dir.join(file)) {
                files.push((name.to_string(), text));
            }
        }
    }
    Ok(files)
}

/// Writes the bundle as a zip file at `path`.
pub fn write_zip(files: &[(String, String)], path: &Path) -> Result<(), String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in files {
            zip.start_file(name.as_str(), options).map_err(|e| format!("DIAG_ZIP: {e}"))?;
            zip.write_all(text.as_bytes()).map_err(|e| format!("DIAG_ZIP: {e}"))?;
        }
        zip.finish().map_err(|e| format!("DIAG_ZIP: {e}"))?;
    }
    crate::export_engine::write_atomically(path, &bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn the_log_appends_lines_and_rolls_over() {
        let dir = std::env::temp_dir().join(format!("lb-diag-log-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_line(&dir, "LocalBooks started", 64);
        write_line(&dir, "opened Books.localbooks\nsecond line", 64);
        let text = std::fs::read_to_string(log_path(&dir)).unwrap();
        assert_eq!(text.lines().count(), 2, "one line each: {text}");
        assert!(text.contains("opened Books.localbooks second line"));
        write_line(&dir, "past the limit", 64);
        assert!(dir.join("diagnostics.old.log").exists(), "the full log rolls over");
        // Again, with an older log already there (renaming over it fails on Windows).
        for n in 0..4 {
            write_line(&dir, &format!("filling the log again, line {n}"), 64);
        }
        assert!(std::fs::metadata(log_path(&dir)).unwrap().len() <= 200, "the log keeps rolling over");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_bundle_has_versions_schema_and_counts_but_no_business_data() {
        let pool = test_pool().await;
        let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Secret Customer Ltd", "issueDate": "2026-10-01", "dueDate": "2026-10-31",
            "lines": [{ "description": "Confidential work", "quantityE4": 10_000, "rateCents": 123_456, "incomeAccountId": income, "isTaxable": false }] }))
        .unwrap();
        crate::invoices::create_and_post_invoice(&pool, input).await.unwrap();

        let files = bundle(Some(&pool), None, None, "9.9.9").await.unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["versions.txt", "database.txt"]);
        let all: String = files.iter().map(|(_, t)| t.as_str()).collect();
        assert!(all.contains("LocalBooks 9.9.9") && all.contains("SQLite: 3."));
        assert!(all.contains("Integrity (quick check): ok"));
        assert!(all.contains("  invoices: 1"), "counts per table");
        assert!(all.contains("\n  0001 "), "the schema migrations");
        for business in ["Secret Customer", "Confidential", "1234.56", "123456"] {
            assert!(!all.contains(business), "no business data: found {business}");
        }

        let dir = std::env::temp_dir().join(format!("lb-diag-zip-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("diagnostics.zip");
        write_zip(&files, &path).unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        assert!(archive.by_name("versions.txt").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(bundle(None, None, None, "1").await.unwrap()[1].1, "No company file is open.\n");
    }
}
