//! Condense Data: archive closed years and continue in a smaller company file.

use super::company_files::company_changed;
use super::prelude::*;
use crate::condense::{self, CondensePreview, CondenseResult, CondenseStatus};

handlers![
    preview_condense,
    condense_company_file,
    get_condense_status,
];

/// What condensing through `cutoff` would summarize and keep (read-only).
#[tauri::command]
pub async fn preview_condense(db: State<'_, Db>, cutoff: NaiveDate) -> Result<CondensePreview, String> {
    condense::preview(&db.pool().await, cutoff).await
}

/// Archives the books, condenses a copy through `cutoff`, verifies it against the archive and
/// switches to it. The books are left as they were if any step or check fails.
#[tauri::command]
pub async fn condense_company_file(app: AppHandle, db: State<'_, Db>, cutoff: NaiveDate) -> Result<CondenseResult, String> {
    let result = condense::condense_live(&db, cutoff).await?;
    company_changed(&app).await;
    notify(&app);
    Ok(result)
}

/// Whether the open file is an archive, or a condensed file and where its archive is.
#[tauri::command]
pub async fn get_condense_status(db: State<'_, Db>) -> Result<CondenseStatus, String> {
    condense::status(&db.pool().await).await
}
