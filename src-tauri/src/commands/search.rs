//! Search.

use super::prelude::*;
use crate::custom_reports::find::{self, FindQuery, FindResult};

handlers![
    search_books,
    find_transactions,
];

#[tauri::command]
pub async fn search_books(db: State<'_, Db>, query: String) -> Result<Vec<SearchHit>, String> {
    workflows::search(&db.pool().await, &query).await
}

#[tauri::command]
pub async fn find_transactions(db: State<'_, Db>, query: FindQuery) -> Result<FindResult, String> {
    find::find(&db.pool().await, &query, Local::now().date_naive()).await
}
