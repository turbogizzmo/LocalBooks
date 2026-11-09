//! Save every attachment at once: the files attached to transactions, gathered into one zip file,
//! a folder per transaction ("2026-10-01 Invoice 1001 (entry 42)") so they are easy to find and
//! never overwrite each other. Reading only; nothing in the books changes.

use std::collections::HashSet;
#[cfg(test)]
use std::io::Cursor;
use std::io::{Seek, Write};
use std::path::Path;

use sqlx::SqlitePool;

use crate::ledger::db_err;

/// What a source type is called in a folder name.
fn source_label(source_type: &str) -> &'static str {
    match source_type {
        "INVOICE" => "Invoice",
        "SALES_RECEIPT" => "Sales receipt",
        "RECEIVE_PAYMENT" => "Payment",
        "CREDIT_MEMO" => "Credit memo",
        "BILL" => "Bill",
        "BILL_PAYMENT" => "Bill payment",
        "EXPENSE" => "Expense",
        "CHECK" => "Check",
        "DEPOSIT" => "Deposit",
        "TRANSFER" => "Transfer",
        "VENDOR_CREDIT" => "Vendor credit",
        "JOURNAL" => "Journal entry",
        _ => "Transaction",
    }
}

/// A file or folder name without characters that file systems refuse, trimmed to a sane length.
fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let short: String = trimmed.chars().take(120).collect();
    if short.is_empty() { "attachment".into() } else { short }
}

/// "report.pdf" → "report (2).pdf" for the second file of the same name in a folder.
fn numbered(name: &str, n: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// How many attachments there are to save (one transaction's, or all): a cheap check before
/// asking where to save them.
pub async fn count(pool: &SqlitePool, entry: Option<i64>) -> Result<usize, String> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE ?1 IS NULL OR journal_entry_id = ?1").bind(entry).fetch_one(pool).await.map_err(db_err)?;
    Ok(usize::try_from(n).unwrap_or(0))
}

/// Writes the attachments into a zip on `out`, reading one file at a time so a company with many
/// large attachments never holds them all in memory. Returns how many files it holds; refuses
/// when there are none.
pub async fn write_zip<W: Write + Seek>(pool: &SqlitePool, entry: Option<i64>, out: W) -> Result<(usize, W), String> {
    let rows: Vec<(i64, i64, String, String, i64, String)> = sqlx::query_as(
        "SELECT a.id, e.id, e.entry_date, e.source_type, e.entry_number, a.file_name
         FROM attachments a JOIN journal_entries e ON e.id = a.journal_entry_id
         WHERE ?1 IS NULL OR a.journal_entry_id = ?1
         ORDER BY e.entry_date, e.entry_number, a.id",
    )
    .bind(entry)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if rows.is_empty() {
        return Err("ATTACH_NONE: there are no attachments to save".into());
    }
    let mut zip = zip::ZipWriter::new(out);
    // Most attachments (PDFs, photos) are already compressed; store them as they are.
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(false);
    let mut used = HashSet::new();
    for (attachment_id, entry_id, date, source_type, entry_number, file_name) in &rows {
        let folder = safe_name(&format!("{date} {} {entry_number} (entry {entry_id})", source_label(source_type)));
        let base = safe_name(file_name);
        let mut path = format!("{folder}/{base}");
        let mut n = 2;
        while !used.insert(path.to_lowercase()) {
            path = format!("{folder}/{}", numbered(&base, n));
            n += 1;
        }
        let content: Vec<u8> = sqlx::query_scalar("SELECT content FROM attachments WHERE id = ?1").bind(attachment_id).fetch_one(pool).await.map_err(db_err)?;
        zip.start_file(path.as_str(), options).map_err(|e| format!("ATTACH_ZIP: {e}"))?;
        zip.write_all(&content).map_err(|e| format!("ATTACH_ZIP: {e}"))?;
    }
    let out = zip.finish().map_err(|e| format!("ATTACH_ZIP: {e}"))?;
    Ok((rows.len(), out))
}

/// Saves the zip at `path`: written to a hidden file beside it, then renamed into place, so an
/// interrupted save never leaves half a zip under the chosen name.
pub async fn save_zip(pool: &SqlitePool, entry: Option<i64>, path: &Path) -> Result<usize, String> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| "ATTACH_BAD_PATH: choose a file name".to_string())?;
    let tmp = dir.join(format!(".{}.partial-{}", name.to_string_lossy(), std::process::id()));
    let file = std::fs::File::create(&tmp).map_err(|e| format!("ATTACH_WRITE: could not write {}: {e}", path.display()))?;
    let result = async {
        let (count, file) = write_zip(pool, entry, file).await?;
        file.sync_all().and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| format!("ATTACH_WRITE: could not write {}: {e}", path.display()))?;
        Ok(count)
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The zip in memory, for tests.
#[cfg(test)]
pub async fn zip_attachments(pool: &SqlitePool, entry: Option<i64>) -> Result<(usize, Vec<u8>), String> {
    let (count, out) = write_zip(pool, entry, Cursor::new(Vec::new())).await?;
    Ok((count, out.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn names_are_safe_and_numbered() {
        assert_eq!(safe_name("a/b:c?.pdf"), "a_b_c_.pdf");
        assert_eq!(safe_name("  ..  "), "attachment");
        assert_eq!(numbered("receipt.pdf", 2), "receipt (2).pdf");
        assert_eq!(numbered("README", 3), "README (3)");
        assert_eq!(numbered(".hidden", 2), ".hidden (2)");
    }

    #[tokio::test]
    async fn every_attachment_goes_into_one_zip_a_folder_per_transaction() {
        let pool = crate::db::test_pool().await;
        let cash: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
        let sales: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(&pool).await.unwrap();
        let mut entries = Vec::new();
        for day in ["2026-10-01", "2026-10-02"] {
            let posted = crate::ledger::post_journal_entry(
                &pool,
                crate::ledger::JournalEntry {
                    id: None,
                    date: chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap(),
                    description: "Supplies".into(),
                    lines: vec![crate::ledger::JournalLine::debit(cash, 500), crate::ledger::JournalLine::credit(sales, 500)],
                    currency: None,
                },
            )
            .await
            .unwrap();
            entries.push(posted);
        }
        assert!(zip_attachments(&pool, None).await.unwrap_err().starts_with("ATTACH_NONE"));
        let attach = |entry: i64, name: &'static str, body: &'static [u8]| {
            let pool = pool.clone();
            async move {
                sqlx::query("INSERT INTO attachments (journal_entry_id, file_name, media_type, size_bytes, content) VALUES (?1, ?2, 'application/pdf', ?3, ?4)")
                    .bind(entry)
                    .bind(name)
                    .bind(body.len() as i64)
                    .bind(body)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        };
        attach(entries[0], "receipt.pdf", b"first").await;
        attach(entries[0], "receipt.pdf", b"second").await;
        attach(entries[1], "photo.jpg", b"third").await;

        let (count, bytes) = zip_attachments(&pool, None).await.unwrap();
        assert_eq!(count, 3);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = archive.file_names().map(|n| n.unwrap().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 3);
        assert!(names[0].starts_with("2026-10-01 Journal entry ") && names[0].ends_with("/receipt (2).pdf"), "{names:?}");
        assert!(names[1].ends_with("/receipt.pdf"));
        assert!(names[2].starts_with("2026-10-02 ") && names[2].ends_with("/photo.jpg"));
        let mut body = String::new();
        archive.by_name(&names[0]).unwrap().read_to_string(&mut body).unwrap();
        assert_eq!(body, "second");

        // One transaction's attachments only.
        let (count, _) = zip_attachments(&pool, Some(entries[1])).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!((super::count(&pool, None).await.unwrap(), super::count(&pool, Some(entries[1])).await.unwrap()), (3, 1));

        // Saved to disk through a hidden file renamed into place.
        let dir = std::env::temp_dir().join(format!("lb-attach-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Attachments.zip");
        assert_eq!(save_zip(&pool, None, &target).await.unwrap(), 3);
        assert_eq!(zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap().len(), 3);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no partial file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
