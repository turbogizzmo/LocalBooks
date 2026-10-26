//! Journal entries, voiding, the journal register and the audit log.

use super::prelude::*;

handlers![
    post_journal_entry,
    void_transaction,
    list_journal_entries,
    get_journal_entry_lines,
    list_audit_log,
    verify_ledger_integrity,
];

#[tauri::command]
pub async fn post_journal_entry(
    app: AppHandle,
    db: State<'_, Db>,
    entry: JournalEntry,
    adjusting: Option<bool>,
) -> Result<i64, String> {
    let id = ledger::post_journal_entry_flagged(&db.pool().await, entry, adjusting.unwrap_or(false)).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn void_transaction(app: AppHandle, db: State<'_, Db>, journal_entry_id: i64) -> Result<(), String> {
    ledger::void_transaction(&db.pool().await, journal_entry_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_journal_entries(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<JournalEntryView>, String> {
    ledger::list_journal_entries(&db.pool().await, limit.unwrap_or(500)).await
}

#[tauri::command]
pub async fn get_journal_entry_lines(db: State<'_, Db>, journal_entry_id: i64) -> Result<Vec<JournalLineView>, String> {
    ledger::journal_entry_lines(&db.pool().await, journal_entry_id).await
}

#[tauri::command]
pub async fn list_audit_log(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<AuditEntry>, String> {
    ledger::list_audit_log(&db.pool().await, limit.unwrap_or(1000)).await
}

/// Compares every posted entry with the snapshot recorded when it was posted.
#[tauri::command]
pub async fn verify_ledger_integrity(db: State<'_, Db>) -> Result<LedgerVerification, String> {
    ledger::verify_ledger_audit(&db.pool().await).await
}
