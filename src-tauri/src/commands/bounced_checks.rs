//! Record Bounced Check.

use super::prelude::*;
use crate::bounced_checks::{self, BounceCandidate, BounceInput, BouncedCheckView};

handlers![
    list_bounce_candidates,
    list_bounced_checks,
    record_bounced_check,
    void_bounced_check,
];

#[tauri::command]
pub async fn list_bounce_candidates(db: State<'_, Db>) -> Result<Vec<BounceCandidate>, String> {
    bounced_checks::candidates(&db.pool().await).await
}

#[tauri::command]
pub async fn list_bounced_checks(db: State<'_, Db>) -> Result<Vec<BouncedCheckView>, String> {
    bounced_checks::list(&db.pool().await).await
}

#[tauri::command]
pub async fn record_bounced_check(app: AppHandle, db: State<'_, Db>, input: BounceInput) -> Result<BouncedCheckView, String> {
    let bounce = bounced_checks::record(&db.pool().await, input).await?;
    notify(&app);
    Ok(bounce)
}

#[tauri::command]
pub async fn void_bounced_check(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    bounced_checks::void(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
