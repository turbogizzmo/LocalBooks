//! Paying bills by NACHA ACH file: vendor bank details, originator details and ACH files.

use super::prelude::*;

handlers![
    get_vendor_bank,
    set_vendor_bank,
    get_ach_originator,
    set_ach_originator,
    pay_bills_by_ach,
    save_ach_file,
    list_ach_files,
];

#[tauri::command]
pub async fn get_vendor_bank(db: State<'_, Db>, vendor_id: i64) -> Result<Option<VendorBankView>, String> {
    let (pool, session) = signed_in(&db).await?;
    ach::get_vendor_bank(&pool, vendor_id, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn set_vendor_bank(db: State<'_, Db>, vendor_id: i64, input: Option<VendorBankInput>) -> Result<Option<VendorBankView>, String> {
    let (pool, session) = signed_in(&db).await?;
    ach::set_vendor_bank(&pool, vendor_id, input, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn get_ach_originator(db: State<'_, Db>, account_id: i64) -> Result<Option<OriginatorDetails>, String> {
    ach::get_ach_originator(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn set_ach_originator(db: State<'_, Db>, input: OriginatorDetails) -> Result<OriginatorDetails, String> {
    ach::set_ach_originator(&db.pool().await, input).await
}

/// Pay Bills › Pay by ACH file: records the payments, then writes the file to `path`. When the
/// payments are recorded but the file cannot be written, it can be saved again from ACH files.
#[tauri::command]
pub async fn pay_bills_by_ach(app: AppHandle, db: State<'_, Db>, input: AchPaymentInput, path: PathBuf) -> Result<AchFileView, String> {
    let (pool, session) = signed_in(&db).await?;
    let access = sensitive_access(&pool, &session).await?;
    let file = ach::pay_bills_by_ach(&pool, input, access, Local::now().naive_local()).await?;
    notify(&app);
    ach::save_ach_file(&pool, file.id, &path, access).await.map_err(|err| {
        format!("ACH_FILE_NOT_SAVED: the payments are recorded, but the file was not saved ({err}). Save it from ACH files.")
    })
}

#[tauri::command]
pub async fn save_ach_file(db: State<'_, Db>, id: i64, path: PathBuf) -> Result<AchFileView, String> {
    let (pool, session) = signed_in(&db).await?;
    ach::save_ach_file(&pool, id, &path, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn list_ach_files(db: State<'_, Db>) -> Result<Vec<AchFileView>, String> {
    ach::list_ach_files(&db.pool().await).await
}
