//! Editing posted documents.

use super::prelude::*;

handlers![
    replace_invoice,
    replace_bill,
    replace_expense,
    replace_journal_entry,
    get_document_lines,
];

#[tauri::command]
pub async fn replace_invoice(app: AppHandle, db: State<'_, Db>, id: i64, input: InvoiceInput) -> Result<i64, String> {
    let new_id = edits::replace_invoice(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_bill(app: AppHandle, db: State<'_, Db>, id: i64, input: BillInput) -> Result<i64, String> {
    let new_id = edits::replace_bill(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_expense(app: AppHandle, db: State<'_, Db>, id: i64, input: ExpenseInput) -> Result<i64, String> {
    let new_id = edits::replace_expense(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_journal_entry(app: AppHandle, db: State<'_, Db>, id: i64, entry: JournalEntry, adjusting: Option<bool>) -> Result<i64, String> {
    let new_id = edits::replace_journal_entry_flagged(&db.pool().await, id, entry, adjusting).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn get_document_lines(db: State<'_, Db>, kind: String, id: i64) -> Result<Vec<DocumentLineView>, String> {
    edits::document_lines(&db.pool().await, &kind, id).await
}
