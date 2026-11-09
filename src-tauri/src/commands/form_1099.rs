//! Form 1099-NEC: vendors' W-9 details, and the year's forms with their 1096 summary.

use super::prelude::*;
use crate::form_1099::{self, Summary1096, W9Input, W9};

handlers![get_vendor_w9, set_vendor_w9, get_1099_nec_summary];

#[tauri::command]
pub async fn get_vendor_w9(db: State<'_, Db>, vendor_id: i64) -> Result<W9, String> {
    let (pool, session) = signed_in(&db).await?;
    form_1099::get_w9(&pool, sensitive_access(&pool, &session).await?, vendor_id).await
}

#[tauri::command]
pub async fn set_vendor_w9(app: AppHandle, db: State<'_, Db>, vendor_id: i64, input: W9Input) -> Result<W9, String> {
    let (pool, session) = signed_in(&db).await?;
    let saved = form_1099::set_w9(&pool, sensitive_access(&pool, &session).await?, vendor_id, &input).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn get_1099_nec_summary(db: State<'_, Db>, year: i32) -> Result<Summary1096, String> {
    let (pool, session) = signed_in(&db).await?;
    form_1099::nec_summary(&pool, year, sensitive_access(&pool, &session).await?).await
}
