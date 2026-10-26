//! Search.

use super::prelude::*;

handlers![
    search_books,
];

#[tauri::command]
pub async fn search_books(db: State<'_, Db>, query: String) -> Result<Vec<SearchHit>, String> {
    workflows::search(&db.pool().await, &query).await
}
