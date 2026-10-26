//! Chart of accounts.

use super::prelude::*;

handlers![
    list_accounts,
    list_account_types,
    create_account,
    update_account,
    set_account_active,
];

#[tauri::command]
pub async fn list_accounts(db: State<'_, Db>) -> Result<Vec<AccountView>, String> {
    reports::list_accounts(&db.pool().await).await
}

#[tauri::command]
pub async fn list_account_types(db: State<'_, Db>) -> Result<Vec<AccountTypeView>, String> {
    accounts::list_account_types(&db.pool().await).await
}

#[tauri::command]
pub async fn create_account(app: AppHandle, db: State<'_, Db>, input: AccountInput) -> Result<AccountView, String> {
    let account = accounts::create_account(&db.pool().await, input).await?;
    notify(&app);
    Ok(account)
}

#[tauri::command]
pub async fn update_account(app: AppHandle, db: State<'_, Db>, id: i64, input: AccountInput) -> Result<AccountView, String> {
    let account = accounts::update_account(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(account)
}

#[tauri::command]
pub async fn set_account_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<AccountView, String> {
    let account = accounts::set_account_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(account)
}
