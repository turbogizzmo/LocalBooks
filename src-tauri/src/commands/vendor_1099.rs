//! 1099 report.

use super::prelude::*;

handlers![
    get_1099_report,
];

#[tauri::command]
pub async fn get_1099_report(db: State<'_, Db>, year: i32) -> Result<Vec<Vendor1099Row>, String> {
    let (pool, session) = signed_in(&db).await?;
    workflows::vendors_1099(&pool, year, sensitive_access(&pool, &session).await?).await
}
