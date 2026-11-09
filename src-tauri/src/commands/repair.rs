//! File checkup, rebuild and forensic corrections.

use super::prelude::*;

handlers![
    file_repair_checkup,
    file_repair_rebuild,
    propose_forensic_corrections,
    apply_forensic_correction,
];

/// File Check & Repair check-up of the live company file (read-only).
#[tauri::command]
pub async fn file_repair_checkup(db: State<'_, Db>) -> Result<CheckupReport, String> {
    file_repair::checkup_live(&db).await
}

/// File Check & Repair rebuild: snapshot, restore this build's safeguards and indexes, compact, re-check.
#[tauri::command]
pub async fn file_repair_rebuild(app: AppHandle, db: State<'_, Db>) -> Result<RebuildResult, String> {
    let result = file_repair::rebuild_live(&db).await?;
    notify(&app);
    Ok(result)
}

/// For each entry that no longer matches its posting snapshot: the correcting entry that would
/// restore it, or why none can.
#[tauri::command]
pub async fn propose_forensic_corrections(db: State<'_, Db>) -> Result<Vec<CorrectionProposal>, String> {
    forensic::propose_corrections(&db.pool().await).await
}

/// Posts one reviewed correction (only if it still matches what was reviewed).
#[tauri::command]
pub async fn apply_forensic_correction(app: AppHandle, db: State<'_, Db>, input: ApplyCorrectionInput) -> Result<AppliedCorrection, String> {
    let applied = forensic::apply_correction(&db.pool().await, &input).await?;
    notify(&app);
    Ok(applied)
}
