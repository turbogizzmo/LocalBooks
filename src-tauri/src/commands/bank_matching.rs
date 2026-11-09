//! Bank feed matching (downloaded lines to transactions already entered) and Add several at once.

use super::prelude::*;
use crate::bank::matching::{self, MatchCandidate, MatchedLine};

handlers![
    list_bank_match_candidates,
    match_bank_transaction,
    unmatch_bank_transaction,
    list_matched_bank_transactions,
    approve_bank_transactions,
];

/// The entered transactions a pending bank line could be, nearest date first.
#[tauri::command]
pub async fn list_bank_match_candidates(db: State<'_, Db>, transaction_id: i64) -> Result<Vec<MatchCandidate>, String> {
    matching::candidates(&db.pool().await, transaction_id).await
}

#[tauri::command]
pub async fn match_bank_transaction(app: AppHandle, db: State<'_, Db>, transaction_id: i64, journal_entry_id: i64) -> Result<(), String> {
    matching::match_line(&db.pool().await, transaction_id, journal_entry_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn unmatch_bank_transaction(app: AppHandle, db: State<'_, Db>, transaction_id: i64) -> Result<(), String> {
    matching::unmatch(&db.pool().await, transaction_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_matched_bank_transactions(db: State<'_, Db>) -> Result<Vec<MatchedLine>, String> {
    matching::list_matched(&db.pool().await).await
}

/// One line of Add several at once.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BankApprovalInput {
    pub transaction_id: i64,
    pub category_account_id: i64,
    pub class_id: Option<i64>,
    pub payee_name: Option<String>,
}

/// Adds several pending lines at once, all or none; returns their entries.
#[tauri::command]
pub async fn approve_bank_transactions(app: AppHandle, db: State<'_, Db>, suggester: State<'_, Suggester>, lines: Vec<BankApprovalInput>) -> Result<Vec<i64>, String> {
    let file = db.path().await?.display().to_string();
    let approvals = lines
        .into_iter()
        .map(|l| (l.transaction_id, bank::Approval { category_account_id: l.category_account_id, class_id: l.class_id, payee_name: l.payee_name }))
        .collect();
    let entries = bank::approve_many(&db.pool().await, &suggester, &file, approvals).await?;
    notify(&app);
    Ok(entries)
}
