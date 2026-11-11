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
pub async fn replace_invoice(app: AppHandle, db: State<'_, Db>, id: i64, input: InvoiceInput, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    crate::sales::require_listed(&pool, crate::sales::Party::Customer, &input.customer_name).await?;
    let replacing: Option<i64> = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(id).fetch_optional(&pool).await.map_err(crate::ledger::db_err)?.flatten();
    guards::check(&pool, guards::invoice(&input, replacing), confirmed.as_deref()).await?;
    let new_id = edits::replace_invoice(&pool, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_bill(app: AppHandle, db: State<'_, Db>, id: i64, input: BillInput, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    crate::sales::require_listed(&pool, crate::sales::Party::Vendor, &input.vendor_name).await?;
    guards::check(&pool, guards::bill(&input, Some(id)), confirmed.as_deref()).await?;
    let new_id = edits::replace_bill(&pool, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_expense(app: AppHandle, db: State<'_, Db>, id: i64, input: ExpenseInput, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    crate::sales::require_listed_payee(&pool, input.payee_name.as_deref()).await?;
    guards::check(&pool, guards::expense(&input, Some(id)), confirmed.as_deref()).await?;
    let new_id = edits::replace_expense(&pool, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_journal_entry(app: AppHandle, db: State<'_, Db>, id: i64, entry: JournalEntry, adjusting: Option<bool>, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::journal(&entry), confirmed.as_deref()).await?;
    let new_id = edits::replace_journal_entry_flagged(&pool, id, entry, adjusting).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn get_document_lines(db: State<'_, Db>, kind: String, id: i64) -> Result<Vec<DocumentLineView>, String> {
    edits::document_lines(&db.pool().await, &kind, id).await
}
