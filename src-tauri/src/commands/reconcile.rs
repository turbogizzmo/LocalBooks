//! Reconciliation.

use super::prelude::*;

handlers![
    get_reconcile_state,
    get_unreconciled_lines,
    list_reconciliations,
    start_reconciliation,
    set_lines_cleared,
    lock_reconciliation,
    cancel_reconciliation,
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
) -> Result<Reconciliation, String> {
    reconcile::start(&db.pool().await, account_id, statement_end_date, statement_ending_balance_cents).await
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
