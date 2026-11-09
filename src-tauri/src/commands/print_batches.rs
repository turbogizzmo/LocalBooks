//! File › Print Forms batches: credit memos, sales receipts, purchase orders, bill payment stubs
//! and mailing labels.

use super::prelude::*;
use crate::forms::RenderedForm;
use crate::print_batches::{self, BatchKind, BatchRow, LabelNames, LabelRow};

handlers![list_print_batch, list_label_names, render_mailing_labels];

#[tauri::command]
pub async fn list_print_batch(db: State<'_, Db>, kind: BatchKind, from: NaiveDate, to: NaiveDate) -> Result<Vec<BatchRow>, String> {
    print_batches::list(&db.pool().await, kind, from, to).await
}

#[tauri::command]
pub async fn list_label_names(db: State<'_, Db>, names: LabelNames) -> Result<Vec<LabelRow>, String> {
    print_batches::label_rows(&db.pool().await, names).await
}

#[tauri::command]
pub async fn render_mailing_labels(db: State<'_, Db>, names: LabelNames, ids: Vec<i64>, skip: usize) -> Result<RenderedForm, String> {
    print_batches::render_labels(&db.pool().await, names, &ids, skip).await
}
