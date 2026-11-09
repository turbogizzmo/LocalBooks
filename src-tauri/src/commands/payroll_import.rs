//! Payroll import: a payroll service's journal export posted as journal entries.

use super::prelude::*;
use crate::payroll_import::{self, PayrollChoices, PayrollImport, PayrollPreview, PostedPayroll};

handlers![preview_payroll_import, post_payroll_import, list_payroll_imports];

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("PAYROLL_UNREADABLE: {}: {e}", path.display()))
}

/// Reads and checks a payroll export: the entries it would post, its account matches and problems.
#[tauri::command]
pub async fn preview_payroll_import(db: State<'_, Db>, path: PathBuf, choices: PayrollChoices) -> Result<PayrollPreview, String> {
    let bytes = read_file(&path)?;
    payroll_import::preview(&db.pool().await, &path.to_string_lossy(), &bytes, &choices).await
}

/// Posts every entry of a payroll export, or none.
#[tauri::command]
pub async fn post_payroll_import(app: AppHandle, db: State<'_, Db>, path: PathBuf, choices: PayrollChoices) -> Result<PostedPayroll, String> {
    let bytes = read_file(&path)?;
    let posted = payroll_import::post(&db.pool().await, &path.to_string_lossy(), &bytes, &choices).await?;
    notify(&app);
    Ok(posted)
}

#[tauri::command]
pub async fn list_payroll_imports(db: State<'_, Db>) -> Result<Vec<PayrollImport>, String> {
    payroll_import::history(&db.pool().await).await
}
