//! Bank file import, SimpleFIN connections and the bank feed.

use super::prelude::*;

handlers![
    inspect_bank_file,
    import_bank_csv,
    simplefin_list,
    simplefin_connect,
    simplefin_reconnect,
    simplefin_refresh_accounts,
    simplefin_link,
    simplefin_unlink,
    simplefin_suggested_start,
    simplefin_sync,
    simplefin_disconnect,
    list_bank_transactions,
    approve_bank_transaction,
    exclude_bank_transaction,
];

/// Reads a bank file (CSV, OFX, QFX, PDF statement) and reports how it would be imported, importing nothing.
#[tauri::command]
pub async fn inspect_bank_file(db: State<'_, Db>, path: PathBuf, mapping: Option<ColumnMapping>, bank_account_id: Option<i64>) -> Result<BankFileInspection, String> {
    bank::inspect_file(&db.pool().await, &path, mapping, bank_account_id).await
}

#[tauri::command]
pub async fn import_bank_csv(
    app: AppHandle,
    db: State<'_, Db>,
    path: PathBuf,
    bank_account_id: i64,
    mapping: Option<ColumnMapping>,
) -> Result<ImportSummary, String> {
    let summary = bank::import_file(&db.pool().await, &path, bank_account_id, mapping).await?;
    notify(&app);
    Ok(summary)
}

#[tauri::command]
pub async fn simplefin_list(db: State<'_, Db>, sf: State<'_, SimpleFin>) -> Result<Vec<ConnectionView>, String> {
    sf.list(&db.pool().await).await
}

/// Claims a setup token (it works once) and lists the connection's accounts.
#[tauri::command]
pub async fn simplefin_connect(db: State<'_, Db>, sf: State<'_, SimpleFin>, token: String) -> Result<ConnectionView, String> {
    sf.connect(&db.pool().await, &token).await
}

#[tauri::command]
pub async fn simplefin_reconnect(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64, token: String) -> Result<ConnectionView, String> {
    sf.reconnect(&db.pool().await, connection_id, &token).await
}

#[tauri::command]
pub async fn simplefin_refresh_accounts(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<ConnectionView, String> {
    sf.refresh_accounts(&db.pool().await, connection_id).await
}

#[tauri::command]
pub async fn simplefin_link(db: State<'_, Db>, account_id: i64, bank_account_id: i64, sync_from: NaiveDate) -> Result<(), String> {
    simplefin::link_account(&db.pool().await, account_id, bank_account_id, sync_from, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn simplefin_unlink(db: State<'_, Db>, account_id: i64) -> Result<(), String> {
    simplefin::unlink_account(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn simplefin_suggested_start(db: State<'_, Db>, bank_account_id: i64) -> Result<NaiveDate, String> {
    simplefin::suggested_start(&db.pool().await, bank_account_id, Local::now().date_naive()).await
}

/// Fetches new posted transactions for the connection's linked accounts into Bank Feeds.
#[tauri::command]
pub async fn simplefin_sync(app: AppHandle, db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<SyncSummary, String> {
    let summary = sf.sync(&db.pool().await, connection_id, chrono::Utc::now()).await?;
    notify(&app);
    Ok(summary)
}

/// Removes the connection and the access saved on this computer; imported lines stay.
#[tauri::command]
pub async fn simplefin_disconnect(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<(), String> {
    sf.disconnect(&db.pool().await, connection_id).await
}

#[tauri::command]
pub async fn list_bank_transactions(db: State<'_, Db>, suggester: State<'_, Suggester>) -> Result<Vec<BankTransactionView>, String> {
    let file = db.path().await?.display().to_string();
    bank::list_pending(&db.pool().await, &suggester, &file).await
}

#[tauri::command]
pub async fn approve_bank_transaction(
    app: AppHandle,
    db: State<'_, Db>,
    suggester: State<'_, Suggester>,
    transaction_id: i64,
    category_account_id: i64,
    class_id: Option<i64>,
    payee_name: Option<String>,
) -> Result<i64, String> {
    let file = db.path().await?.display().to_string();
    let approval = bank::Approval { category_account_id, class_id, payee_name };
    let entry_id = bank::approve(&db.pool().await, &suggester, &file, transaction_id, approval).await?;
    notify(&app);
    Ok(entry_id)
}

#[tauri::command]
pub async fn exclude_bank_transaction(
    app: AppHandle,
    db: State<'_, Db>,
    transaction_id: i64,
) -> Result<(), String> {
    bank::exclude(&db.pool().await, transaction_id).await?;
    notify(&app);
    Ok(())
}
