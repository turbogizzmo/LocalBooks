//! Closing Date Exceptions: postings and voids made in a closed period (`closing::exceptions`).

use super::prelude::*;
use crate::closing::ClosingException;

handlers![get_closing_exceptions];

#[tauri::command]
pub async fn get_closing_exceptions(db: State<'_, Db>) -> Result<Vec<ClosingException>, String> {
    closing::exceptions(&db.pool().await).await
}
