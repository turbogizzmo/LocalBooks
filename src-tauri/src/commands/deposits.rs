//! Deposits and transfers.

use super::prelude::*;

handlers![
    list_undeposited,
    post_bank_deposit,
    post_transfer,
    list_banking_activity,
];

#[tauri::command]
pub async fn list_undeposited(db: State<'_, Db>) -> Result<Vec<UndepositedItem>, String> {
    banking::list_undeposited(&db.pool().await).await
}

#[tauri::command]
pub async fn post_bank_deposit(app: AppHandle, db: State<'_, Db>, input: DepositInput) -> Result<i64, String> {
    let id = banking::post_bank_deposit(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn post_transfer(app: AppHandle, db: State<'_, Db>, input: TransferInput) -> Result<i64, String> {
    let id = banking::post_transfer(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_banking_activity(db: State<'_, Db>) -> Result<Vec<BankingActivity>, String> {
    banking::list_banking_activity(&db.pool().await).await
}
