//! Export from an installed QuickBooks (Preview): LocalBooks asks the QuickBooks running on this
//! computer for the open company's lists and Journal, and stages them like a report export.

use super::migration::{IngestResult, QBW_PROGRESS};
use super::prelude::*;
use crate::installed_qb;

handlers![
    inspect_installed_quickbooks,
    export_from_installed_quickbooks,
];

/// What LocalBooks can see of QuickBooks on this computer. It asks QuickBooks nothing, so no
/// permission prompt appears.
#[tauri::command]
pub async fn inspect_installed_quickbooks() -> Result<installed_qb::Status, String> {
    tauri::async_runtime::spawn_blocking(installed_qb::status).await.map_err(|e| format!("QB_STATUS_FAILED: {e}"))
}

/// Asks the running QuickBooks for the open company's data, read-only, and reads it into a new
/// staging batch named after the company, reporting progress as `qbw-import-progress` events.
/// Nothing is posted to the ledger. On a Mac, macOS asks the first time whether LocalBooks may
/// control QuickBooks; on Windows, QuickBooks asks whether to let LocalBooks read the file.
#[tauri::command]
pub async fn export_from_installed_quickbooks(app: AppHandle, db: State<'_, Db>) -> Result<IngestResult, String> {
    let emitter = app.clone();
    let export = tauri::async_runtime::spawn_blocking(move || {
        installed_qb::export_installed(&mut |done, total| {
            let _ = emitter.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Asking QuickBooks", done, total });
        })
    })
    .await
    .map_err(|e| format!("QB_EXPORT_FAILED: QuickBooks' answers could not be read ({e})"))??;
    let ledger = &export.read.ledger;
    if ledger.transactions.is_empty() && ledger.unbalanced.is_empty() && ledger.accounts.is_empty() {
        return Err("QB_EXPORT_EMPTY: QuickBooks sent no accounts or transactions for this company".into());
    }
    // Named after the company; the extension marks where it came from.
    let company = export.company.as_deref().unwrap_or("QuickBooks company").replace(['/', '\\', ':'], "-");
    let pool = db.pool().await;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 0, total: 1 });
    let batch_id = qbw_ledger_import::stage(&pool, Path::new(&format!("{company}.qbxml")), export.size, ledger).await?;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 1, total: 1 });
    let sources = export.read.sources.into_iter().chain(export.notes).collect();
    Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await?, sources })
}
