//! Importing a spreadsheet the user laid out: customers, vendors, transactions or journal entries,
//! mapped column by column, checked row by row, then staged for the import review (Settings › Import & Migration).

use super::migration::{IngestResult, QBW_PROGRESS};
use super::prelude::*;
use super::report_export::choose_export_path;
use crate::bank::DateOrder;
use crate::sheet_import::{self as sheet, Check, Field, Mapping, SheetKind};

handlers![read_import_sheet, check_import_sheet, stage_import_sheet, save_import_template];

/// How many rows of the sheet the mapping step shows.
const PREVIEW_ROWS: usize = 6;

/// What the mapping step shows: the headings, the first rows, the kind's fields and the suggested
/// mapping (field id → column).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPreview {
    file_name: String,
    headers: Vec<String>,
    preview: Vec<(usize, Vec<String>)>,
    row_count: usize,
    fields: Vec<Field>,
    mapping: Mapping,
}

fn read_sheet(file_path: &str) -> Result<(String, i64, sheet::Sheet), String> {
    let path = PathBuf::from(file_path);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !sheet::EXTENSIONS.contains(&ext.as_str()) {
        let kinds = sheet::EXTENSIONS.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ");
        return Err(format!("SHEET_BAD_EXTENSION: {} is not a spreadsheet: choose a {kinds} file", path.display()));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("SHEET_UNREADABLE: {}: {e}", path.display()))?;
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file_path.to_string());
    let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    Ok((name.clone(), size, sheet::read(&name, &bytes)?))
}

/// Reads a spreadsheet's headings and first rows, with the suggested mapping for `kind`.
#[tauri::command]
pub async fn read_import_sheet(file_path: String, kind: SheetKind) -> Result<SheetPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (file_name, _, s) = read_sheet(&file_path)?;
        Ok(SheetPreview {
            file_name,
            mapping: sheet::suggest(kind, &s.headers),
            preview: s.rows.iter().take(PREVIEW_ROWS).cloned().collect(),
            row_count: s.rows.len(),
            headers: s.headers,
            fields: sheet::fields(kind),
        })
    })
    .await
    .map_err(|e| format!("SHEET_UNREADABLE: {e}"))?
}

/// Checks every row with the mapping chosen; lists each row that would fail.
#[tauri::command]
pub async fn check_import_sheet(file_path: String, kind: SheetKind, mapping: Mapping, date_order: Option<DateOrder>) -> Result<Check, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, _, s) = read_sheet(&file_path)?;
        Ok(sheet::check(kind, &s, &mapping, date_order))
    })
    .await
    .map_err(|e| format!("SHEET_UNREADABLE: {e}"))?
}

/// Stages a sheet with no problems as a new import, for the same review as every other import.
/// Refused while any row would fail. Nothing is posted.
#[tauri::command]
pub async fn stage_import_sheet(app: AppHandle, db: State<'_, Db>, file_path: String, kind: SheetKind, mapping: Mapping, date_order: Option<DateOrder>) -> Result<IngestResult, String> {
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Reading file", done: 0, total: 1 });
    let path = file_path.clone();
    let (size, ledger) = tauri::async_runtime::spawn_blocking(move || {
        let (_, size, s) = read_sheet(&path)?;
        let ledger = sheet::to_ledger(kind, &s, &mapping, date_order).map_err(|c| {
            let first = c.problems.first().map(|p| if p.row == 0 { p.message.clone() } else { format!("row {}: {}", p.row, p.message) }).unwrap_or_default();
            format!("SHEET_PROBLEMS: {} row problem(s) to fix before importing; the first is {first}", c.problems.len())
        })?;
        Ok::<_, String>((size, ledger))
    })
    .await
    .map_err(|e| format!("SHEET_UNREADABLE: {e}"))??;
    if ledger.transactions.is_empty() && ledger.customers.is_empty() && ledger.vendors.is_empty() {
        return Err("SHEET_EMPTY: the spreadsheet has no rows to import".into());
    }
    let pool = db.pool().await;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 0, total: 1 });
    let batch_id = qbw_ledger_import::stage_as(&pool, Path::new(&file_path), size, &ledger, Some("SHEET")).await?;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 1, total: 1 });
    Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await?, sources: Vec::new() })
}

/// Saves the template for `kind` (a CSV whose headings are the field names) through the Save
/// dialog. Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_import_template(app: AppHandle, kind: SheetKind) -> Result<Option<String>, String> {
    let name = format!("{} template.csv", kind.title());
    let Some(path) = choose_export_path(app, name).await? else { return Ok(None) };
    export_engine::write_atomically(&path, sheet::template(kind).as_bytes())?;
    Ok(Some(path.display().to_string()))
}
