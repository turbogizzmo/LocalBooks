//! Tax software export (TXF) for companies that file Schedule C.

use super::prelude::*;
use crate::txf::{self, TxfExport};

handlers![preview_txf_export, save_txf_export];

/// What the TXF file for the period would hold, and what it leaves out.
#[tauri::command]
pub async fn preview_txf_export(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<TxfExport, String> {
    txf::export(&db.pool().await, start_date, end_date, Local::now().date_naive()).await
}

/// Writes the TXF file for the period to `path` (chosen in a save dialog).
#[tauri::command]
pub async fn save_txf_export(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate, path: PathBuf) -> Result<TxfExport, String> {
    let out = txf::export(&db.pool().await, start_date, end_date, Local::now().date_naive()).await?;
    std::fs::write(&path, out.text.as_bytes()).map_err(|e| format!("TXF_NOT_SAVED: the file could not be written ({e})"))?;
    Ok(out)
}
