//! Reports outside the custom report engine (`list_reports.rs`): the list reports, summaries of the
//! financial statements and reports over time, purchase orders, inventory and the audit trail; and
//! saving several reports as one workbook (Process Multiple Reports).

use super::prelude::*;
use crate::accountant_reports::AccountantReport;
use crate::list_reports::{self, ListReportInfo};

handlers![
    list_list_reports,
    get_list_report,
    save_xlsx_book,
];

#[tauri::command]
pub async fn list_list_reports() -> Result<Vec<ListReportInfo>, String> {
    Ok(list_reports::catalog())
}

#[tauri::command]
pub async fn get_list_report(db: State<'_, Db>, id: String, start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> Result<AccountantReport, String> {
    list_reports::run(&db.pool().await, &id, start_date, end_date, Local::now().date_naive()).await
}

/// Saves `sheets` as one workbook, a worksheet each, through the native Save dialog. Returns the
/// saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_xlsx_book(app: AppHandle, db: State<'_, Db>, sheets: Vec<xlsx_export::Sheet>, filename: String) -> Result<Option<String>, String> {
    let bytes = xlsx_export::to_xlsx_book(&sheets, &preferences::currency_symbol(&db.pool().await).await?)?;
    let Some(path) = super::report_export::choose_export_path(app, filename).await? else { return Ok(None) };
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}
