//! The document inbox (migration 0102): receipts and bills waiting to be entered.
//!
//! Files land here when dropped on the inbox or picked for it. Each is kept in the company file
//! until it is entered as a bill, expense or check: posting attaches it to the transaction and
//! takes it out of the inbox, in one write. Nothing is sent anywhere. For a PDF with a text layer,
//! LocalBooks reads hints from the text for the form it opens: the total (the amount on a line
//! such as "Total", "Amount due" or "Balance due", else the largest amount), the first date, and
//! the name at the top of the first page. A scan has no text and gets no hints (there is no OCR).

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{db_err, rollback, MAX_LINE_CENTS};
use crate::workflows::{media_type, AttachmentView, MAX_ATTACHMENT_BYTES};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InboxDocument {
    pub id: i64,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub note: Option<String>,
    pub amount_cents: Option<i64>,
    pub document_date: Option<String>,
    pub name_hint: Option<String>,
    pub added_at: String,
}

/// What a PDF's text suggests for the form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub amount_cents: Option<i64>,
    pub date: Option<NaiveDate>,
    pub name: Option<String>,
}

/// Words on a line that name the amount to pay, strongest first.
const TOTAL_WORDS: &[&str] = &["AMOUNT DUE", "BALANCE DUE", "TOTAL DUE", "GRAND TOTAL", "TOTAL"];

/// Amounts written on a line ("$1,234.56", "1234.56", "USD 12.00"), in cents, left to right.
fn amounts(line: &str) -> Vec<i64> {
    line.split_whitespace()
        .filter(|w| w.contains('.') && w.chars().any(|c| c.is_ascii_digit()))
        .filter_map(|w| {
            let w = w.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '(' || c == ')'));
            // Money has two decimals; "1.5" or a version number "2.0.1" isn't an amount.
            let (_, cents) = w.rsplit_once('.')?;
            if cents.trim_end_matches(')').len() != 2 {
                return None;
            }
            crate::bank::parse_amount_cents(w).ok()
        })
        .filter(|c| (1..=MAX_LINE_CENTS).contains(c))
        .collect()
}

/// Reads hints from a document's text lines (see the module comment).
pub fn hints_from_lines(lines: &[String]) -> Hints {
    let mut hints = Hints::default();
    'words: for word in TOTAL_WORDS {
        for line in lines {
            let upper = line.to_uppercase();
            // "Subtotal" isn't the total.
            if upper.contains(word) && !upper.contains(&format!("SUB{word}")) && !upper.contains(&format!("SUB {word}")) {
                if let Some(last) = amounts(line).last() {
                    hints.amount_cents = Some(*last);
                    break 'words;
                }
            }
        }
    }
    if hints.amount_cents.is_none() {
        hints.amount_cents = lines.iter().flat_map(|l| amounts(l)).max();
    }
    hints.date = lines.iter().flat_map(|l| l.split_whitespace().map(|w| w.trim_matches(|c: char| c == ',' || c == ':'))).find_map(|w| {
        (w.chars().filter(char::is_ascii_digit).count() >= 4).then(|| crate::bank::parse_date(w).ok()).flatten()
    });
    hints.name = lines
        .iter()
        .map(|l| l.trim())
        .find(|l| l.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .map(|l| l.chars().take(100).collect::<String>());
    hints
}

fn hints_for(file_name: &str, content: &[u8]) -> Hints {
    if media_type(file_name) != "application/pdf" {
        return Hints::default();
    }
    crate::bank_pdf::text_lines(content).map(|lines| hints_from_lines(&lines)).unwrap_or_default()
}

const SELECT: &str = "SELECT id, file_name, media_type, size_bytes, note, amount_cents, document_date, name_hint, added_at FROM inbox_documents";

/// Puts a file in the inbox; returns it with what LocalBooks read from it.
pub async fn add(pool: &SqlitePool, file_name: &str, content: &[u8]) -> Result<InboxDocument, String> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err("INBOX_SIZE: the inbox takes files of up to 25 MB".into());
    }
    let name = std::path::Path::new(file_name).file_name().and_then(|n| n.to_str()).unwrap_or("document");
    let hints = hints_for(name, content);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO inbox_documents (file_name, media_type, size_bytes, content, amount_cents, document_date, name_hint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(name)
    .bind(media_type(name))
    .bind(i64::try_from(content.len()).map_err(|_| "INBOX_SIZE: the inbox takes files of up to 25 MB".to_string())?)
    .bind(content)
    .bind(hints.amount_cents)
    .bind(hints.date)
    .bind(hints.name)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    get(pool, id).await
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<InboxDocument, String> {
    sqlx::query_as::<_, InboxDocument>(&format!("{SELECT} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "INBOX_NOT_FOUND: that document is no longer in the inbox".into())
}

/// The inbox, oldest first.
pub async fn list(pool: &SqlitePool) -> Result<Vec<InboxDocument>, String> {
    sqlx::query_as::<_, InboxDocument>(&format!("{SELECT} ORDER BY added_at, id")).fetch_all(pool).await.map_err(db_err)
}

/// A document's file, for the preview: (file name, media type, bytes).
pub async fn content(pool: &SqlitePool, id: i64) -> Result<(String, String, Vec<u8>), String> {
    sqlx::query_as("SELECT file_name, media_type, content FROM inbox_documents WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "INBOX_NOT_FOUND: that document is no longer in the inbox".into())
}

/// A note on a waiting document ("ask Pat which job"); empty clears it.
pub async fn set_note(pool: &SqlitePool, id: i64, note: Option<&str>) -> Result<InboxDocument, String> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > 500) {
        return Err("INBOX_NOTE_LONG: keep the note under 500 characters".into());
    }
    sqlx::query("UPDATE inbox_documents SET note = ?1 WHERE id = ?2").bind(note).bind(id).execute(pool).await.map_err(db_err)?;
    get(pool, id).await
}

/// Takes a document out of the inbox without entering it.
pub async fn remove(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let done = sqlx::query("DELETE FROM inbox_documents WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("INBOX_NOT_FOUND: that document is no longer in the inbox".into());
    }
    Ok(())
}

/// Attaches a waiting document to the posted transaction it was entered as, and takes it out of
/// the inbox, in one write.
pub async fn attach(pool: &SqlitePool, id: i64, journal_entry_id: i64) -> Result<AttachmentView, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let posted: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journal_entries WHERE id = ?1 AND status = 'POSTED')")
            .bind(journal_entry_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if !posted {
            return Err("ATTACH_NOT_POSTED: attach files to a posted transaction".to_string());
        }
        let attachment: Option<i64> = sqlx::query_scalar(
            "INSERT INTO attachments (journal_entry_id, file_name, media_type, size_bytes, content)
             SELECT ?2, file_name, media_type, size_bytes, content FROM inbox_documents WHERE id = ?1 RETURNING id",
        )
        .bind(id)
        .bind(journal_entry_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let attachment = attachment.ok_or("INBOX_NOT_FOUND: that document is no longer in the inbox")?;
        sqlx::query("DELETE FROM inbox_documents WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
        Ok(attachment)
    }
    .await;
    let attachment = match result {
        Ok(a) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            a
        }
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::workflows::list_attachments(pool, journal_entry_id)
        .await?
        .into_iter()
        .find(|a| a.id == attachment)
        .ok_or_else(|| "ATTACH_NOT_SAVED: the file was not saved".into())
}

#[cfg(test)]
mod tests;
