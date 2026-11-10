//! The Customer & Vendor Profile lists: Terms, Payment Method, Customer / Vendor / Job Type and
//! Customer Message.

use super::prelude::*;
use crate::profile_lists as pl;

handlers![
    list_terms,
    create_terms,
    update_terms,
    set_terms_active,
    delete_terms,
    list_payment_methods,
    create_payment_method,
    update_payment_method,
    set_payment_method_active,
    delete_payment_method,
    list_profile_entries,
    create_profile_entry,
    rename_profile_entry,
    set_profile_entry_active,
    delete_profile_entry,
];

#[tauri::command]
pub async fn list_terms(db: State<'_, Db>) -> Result<Vec<pl::TermsView>, String> {
    pl::list_terms(&db.pool().await).await
}

#[tauri::command]
pub async fn create_terms(app: AppHandle, db: State<'_, Db>, input: pl::TermsInput) -> Result<pl::TermsView, String> {
    let out = pl::create_terms(&db.pool().await, input).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn update_terms(app: AppHandle, db: State<'_, Db>, id: i64, input: pl::TermsInput) -> Result<pl::TermsView, String> {
    let out = pl::update_terms(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn set_terms_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<pl::TermsView, String> {
    let out = pl::set_terms_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn delete_terms(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    pl::delete_terms(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_payment_methods(db: State<'_, Db>) -> Result<Vec<pl::PaymentMethodView>, String> {
    pl::list_payment_methods(&db.pool().await).await
}

#[tauri::command]
pub async fn create_payment_method(app: AppHandle, db: State<'_, Db>, input: pl::PaymentMethodInput) -> Result<pl::PaymentMethodView, String> {
    let out = pl::create_payment_method(&db.pool().await, input).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn update_payment_method(app: AppHandle, db: State<'_, Db>, id: i64, input: pl::PaymentMethodInput) -> Result<pl::PaymentMethodView, String> {
    let out = pl::update_payment_method(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn set_payment_method_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<pl::PaymentMethodView, String> {
    let out = pl::set_payment_method_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn delete_payment_method(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    pl::delete_payment_method(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_profile_entries(db: State<'_, Db>) -> Result<Vec<pl::ProfileEntryView>, String> {
    pl::list_profile_entries(&db.pool().await).await
}

#[tauri::command]
pub async fn create_profile_entry(app: AppHandle, db: State<'_, Db>, list: pl::ProfileList, name: String) -> Result<pl::ProfileEntryView, String> {
    let out = pl::create_profile_entry(&db.pool().await, list, &name).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn rename_profile_entry(app: AppHandle, db: State<'_, Db>, id: i64, name: String) -> Result<pl::ProfileEntryView, String> {
    let out = pl::rename_profile_entry(&db.pool().await, id, &name).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn set_profile_entry_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<pl::ProfileEntryView, String> {
    let out = pl::set_profile_entry_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(out)
}

#[tauri::command]
pub async fn delete_profile_entry(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    pl::delete_profile_entry(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
