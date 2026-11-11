//! Sales form numbers: the number a new form will get, shown in its number box before it is saved,
//! and whether a typed number is free.

use super::prelude::*;
use crate::doc_numbers::DocKind;

handlers![
    get_next_document_number,
    get_document_number_problem,
];

#[tauri::command]
pub async fn get_next_document_number(db: State<'_, Db>, kind: DocKind) -> Result<i64, String> {
    let mut conn = db.pool().await.acquire().await.map_err(crate::ledger::db_err)?;
    crate::doc_numbers::next_number(&mut conn, kind).await
}

/// Why `number` can't be used for a new form of `kind` (already used, or out of range), or `None`
/// when it is free.
#[tauri::command]
pub async fn get_document_number_problem(db: State<'_, Db>, kind: DocKind, number: i64) -> Result<Option<String>, String> {
    let mut conn = db.pool().await.acquire().await.map_err(crate::ledger::db_err)?;
    Ok(crate::doc_numbers::check_number(&mut conn, kind, number).await.err())
}
