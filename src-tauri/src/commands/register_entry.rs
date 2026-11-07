//! Typing transactions into a register, and a transaction's history.

use super::prelude::*;
use crate::registers::entry::{RegisterEntryInput, RegisterEntryPosted};
use crate::registers::history::TransactionHistory;

handlers![
    post_register_entry,
    get_transaction_history,
];

#[tauri::command]
pub async fn post_register_entry(app: AppHandle, db: State<'_, Db>, input: RegisterEntryInput) -> Result<RegisterEntryPosted, String> {
    let posted = registers::entry::post_register_entry(&db.pool().await, input).await?;
    notify(&app);
    Ok(posted)
}

#[tauri::command]
pub async fn get_transaction_history(db: State<'_, Db>, journal_entry_id: i64) -> Result<TransactionHistory, String> {
    registers::history::transaction_history(&db.pool().await, journal_entry_id).await
}
