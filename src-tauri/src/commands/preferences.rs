//! Preferences, tax calendar and closing date.

use super::prelude::*;
use super::company_files::company_changed;

handlers![
    get_preferences,
    set_preference,
    list_tax_forms,
    get_fiscal_periods,
    get_tax_calendar,
    get_closing_status,
    set_closing_date,
];

#[tauri::command]
pub async fn get_preferences(db: State<'_, Db>) -> Result<std::collections::BTreeMap<String, String>, String> {
    preferences::get_all(&db.pool().await).await
}

#[tauri::command]
pub async fn set_preference(app: AppHandle, db: State<'_, Db>, key: String, value: String) -> Result<String, String> {
    let stored = preferences::set(&db.pool().await, &key, &value).await?;
    if key == "company_name" {
        company_changed(&app).await;
    }
    if key == "auto_lock_minutes" {
        db.refresh_gate().await;
    }
    notify(&app);
    Ok(stored)
}

#[tauri::command]
pub async fn list_tax_forms() -> Result<Vec<(String, String)>, String> {
    Ok(preferences::TAX_FORMS.iter().map(|(c, l)| (c.to_string(), l.to_string())).collect())
}

#[tauri::command]
pub async fn get_fiscal_periods(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<FiscalPeriods, String> {
    let month = preferences::fiscal_start_month(&db.pool().await).await?;
    Ok(preferences::fiscal_periods(as_of.unwrap_or_else(|| Local::now().date_naive()), month))
}

#[tauri::command]
pub async fn get_tax_calendar(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<Vec<TaxDeadline>, String> {
    let pool = db.pool().await;
    let month = preferences::fiscal_start_month(&pool).await?;
    let form = preferences::get(&pool, "tax_form").await?;
    Ok(preferences::tax_calendar(&form, month, as_of.unwrap_or_else(|| Local::now().date_naive())))
}

#[tauri::command]
pub async fn get_closing_status(db: State<'_, Db>) -> Result<ClosingStatus, String> {
    closing::status(&db.pool().await).await
}

#[tauri::command]
pub async fn set_closing_date(app: AppHandle, db: State<'_, Db>, input: ClosingInput) -> Result<ClosingStatus, String> {
    let status = closing::set_closing_date(&db.pool().await, &input).await?;
    notify(&app);
    Ok(status)
}
