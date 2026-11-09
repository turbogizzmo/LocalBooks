//! Reconciliation.

use super::prelude::*;
use crate::reconcile::{ReconcileCharge, StatementFile};

handlers![
    get_reconcile_state,
    get_unreconciled_lines,
    list_reconciliations,
    start_reconciliation,
    set_lines_cleared,
    lock_reconciliation,
    cancel_reconciliation,
    undo_last_reconciliation,
    attach_reconciliation_statement,
    list_reconciliation_statements,
    save_reconciliation_statement,
    remove_reconciliation_statement,
];

#[tauri::command]
pub async fn get_reconcile_state(db: State<'_, Db>, account_id: i64) -> Result<ReconcileAccountState, String> {
    reconcile::account_state(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn get_unreconciled_lines(db: State<'_, Db>, account_id: i64) -> Result<Vec<UnreconciledLine>, String> {
    reconcile::unreconciled_lines(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn list_reconciliations(db: State<'_, Db>, account_id: i64) -> Result<Vec<Reconciliation>, String> {
    reconcile::list_reconciliations(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn start_reconciliation(
    db: State<'_, Db>,
    account_id: i64,
    statement_end_date: NaiveDate,
    statement_ending_balance_cents: i64,
    charges: Option<Vec<ReconcileCharge>>,
) -> Result<Reconciliation, String> {
    reconcile::start_with_charges(&db.pool().await, account_id, statement_end_date, statement_ending_balance_cents, &charges.unwrap_or_default()).await
}

#[tauri::command]
pub async fn set_lines_cleared(
    db: State<'_, Db>,
    reconciliation_id: i64,
    line_ids: Vec<i64>,
    cleared: bool,
) -> Result<ReconcileProgress, String> {
    reconcile::set_lines_cleared(&db.pool().await, reconciliation_id, &line_ids, cleared).await
}

#[tauri::command]
pub async fn lock_reconciliation(
    app: AppHandle,
    db: State<'_, Db>,
    reconciliation_id: i64,
) -> Result<Reconciliation, String> {
    let rec = reconcile::lock(&db.pool().await, reconciliation_id).await?;
    notify(&app);
    Ok(rec)
}

#[tauri::command]
pub async fn cancel_reconciliation(db: State<'_, Db>, reconciliation_id: i64) -> Result<(), String> {
    reconcile::cancel(&db.pool().await, reconciliation_id).await
}

/// Undo Last Reconciliation: an audited release of the account's last reconciliation.
#[tauri::command]
pub async fn undo_last_reconciliation(app: AppHandle, db: State<'_, Db>, reconciliation_id: i64, reason: Option<String>) -> Result<i64, String> {
    let released = reconcile::undo_last(&db.pool().await, reconciliation_id, reason.as_deref()).await?;
    notify(&app);
    Ok(released)
}

/// Attaches the statement file chosen in the open dialog (read here, so the page never handles file bytes).
#[tauri::command]
pub async fn attach_reconciliation_statement(app: AppHandle, db: State<'_, Db>, reconciliation_id: i64, path: String) -> Result<StatementFile, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    if meta.len() as usize > workflows::MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let content = std::fs::read(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    let saved = reconcile::attach_statement(&db.pool().await, reconciliation_id, &path, &content).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn list_reconciliation_statements(db: State<'_, Db>, reconciliation_id: i64) -> Result<Vec<StatementFile>, String> {
    reconcile::statement_files(&db.pool().await, reconciliation_id).await
}

/// Saves a copy of a statement file where the user chooses.
#[tauri::command]
pub async fn save_reconciliation_statement(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<Option<String>, String> {
    let (name, content) = reconcile::statement_content(&db.pool().await, id).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&name).blocking_save_file())
        .await
        .map_err(|e| format!("ATTACH_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("ATTACH_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &content)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn remove_reconciliation_statement(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    reconcile::remove_statement(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
