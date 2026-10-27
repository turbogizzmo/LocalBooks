//! Report export (CSV, Excel, PNG, save, print).

use super::prelude::*;

handlers![
    report_csv,
    save_report_to_disk,
    save_xlsx,
    save_report_xlsx,
    save_png,
    print_report,
    finish_printing,
];

/// Renders a financial statement as CSV (`profit_and_loss` needs start/end, `balance_sheet` needs as_of).
#[tauri::command]
pub async fn report_csv(
    db: State<'_, Db>,
    report: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<ClassFilter>,
) -> Result<String, String> {
    export_engine::render(&db.pool().await, &report, start_date, end_date, as_of, class_filter.as_ref()).await
}

/// Prompts with the native Save dialog for `filename`, filtered to its extension. `None` if cancelled.
async fn choose_export_path(app: AppHandle, filename: String) -> Result<Option<PathBuf>, String> {
    let extension = std::path::Path::new(&filename).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "csv".into());
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&filename)
            .add_filter(extension.to_uppercase(), &[extension.as_str()])
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    target.into_path().map(Some).map_err(|e| format!("EXPORT_BAD_PATH: {e}"))
}

/// Prompts with the native Save dialog and writes `report_html_or_matrix` to the chosen file.
/// Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_report_to_disk(app: AppHandle, report_html_or_matrix: String, filename: String) -> Result<Option<String>, String> {
    let Some(path) = choose_export_path(app, filename).await? else { return Ok(None) };
    export_engine::write_atomically(&path, report_html_or_matrix.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

/// Renders a screen's report as an Excel workbook and saves it through the native Save dialog.
/// Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_xlsx(app: AppHandle, db: State<'_, Db>, sheet: xlsx_export::Sheet, filename: String) -> Result<Option<String>, String> {
    let bytes = xlsx_export::to_xlsx(&sheet, &preferences::currency_symbol(&db.pool().await).await?)?;
    let Some(path) = choose_export_path(app, filename).await? else { return Ok(None) };
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// Renders a financial statement (as [`report_csv`] does) as an Excel workbook and saves it.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_report_xlsx(
    app: AppHandle,
    db: State<'_, Db>,
    report: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<ClassFilter>,
    filename: String,
) -> Result<Option<String>, String> {
    let bytes = xlsx_export::render_report(&db.pool().await, &report, start_date, end_date, as_of, class_filter.as_ref()).await?;
    let Some(path) = choose_export_path(app, filename).await? else { return Ok(None) };
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// Saves a picture of a graph (`png_base64`, the PNG's bytes in base64) through the native Save
/// dialog. Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_png(app: AppHandle, png_base64: String, filename: String) -> Result<Option<String>, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(png_base64.trim()).map_err(|e| format!("EXPORT_BAD_IMAGE: {e}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("EXPORT_BAD_IMAGE: not a PNG image".into());
    }
    let Some(path) = choose_export_path(app, filename).await? else { return Ok(None) };
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// Opens the system print dialog for the current window (Print / Save as PDF). With `title` (a
/// form's "Invoice 1001 · Acme") the window carries it until `finish_printing`, since the dialog
/// names the job, and the PDF it saves, after the window.
#[tauri::command]
pub async fn print_report(window: tauri::WebviewWindow, title: Option<String>) -> Result<(), String> {
    if let Some(title) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let _ = window.set_title(title);
    }
    window.print().map_err(|e| format!("PRINT_FAILED: {e}"))
}

/// Gives the window back its company title once the print dialog closes.
#[tauri::command]
pub async fn finish_printing(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    window.set_title(&super::company_files::window_title(&app).await).map_err(|e| format!("PRINT_FAILED: {e}"))
}
