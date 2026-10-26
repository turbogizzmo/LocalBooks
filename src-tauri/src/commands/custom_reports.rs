//! Custom reports: the report builder, its presets and saved (memorized) reports.

use super::prelude::*;
use crate::custom_reports::{self, CustomReport, Preset, ReportSettings, SavedReport};

handlers![
    get_custom_report,
    list_report_presets,
    list_saved_reports,
    save_custom_report,
    delete_saved_report,
];

#[tauri::command]
pub async fn get_custom_report(db: State<'_, Db>, settings: ReportSettings) -> Result<CustomReport, String> {
    custom_reports::run(&db.pool().await, &settings, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn list_report_presets() -> Result<Vec<Preset>, String> {
    Ok(custom_reports::presets())
}

#[tauri::command]
pub async fn list_saved_reports(db: State<'_, Db>) -> Result<Vec<SavedReport>, String> {
    custom_reports::list_saved(&db.pool().await).await
}

/// Saves the report screen's `settings` under `name`; `replace` overwrites a report of that name.
#[tauri::command]
pub async fn save_custom_report(db: State<'_, Db>, name: String, preset: Option<String>, settings: serde_json::Value, replace: bool) -> Result<SavedReport, String> {
    custom_reports::save(&db.pool().await, &name, preset.as_deref(), &settings, replace).await
}

#[tauri::command]
pub async fn delete_saved_report(db: State<'_, Db>, id: i64) -> Result<(), String> {
    custom_reports::delete_saved(&db.pool().await, id).await
}
