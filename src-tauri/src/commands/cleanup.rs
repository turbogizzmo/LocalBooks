//! Accounting › Clean Up Books.

use super::prelude::*;
use crate::cleanup::{self as cleanup_books, ApplyCreditsInput, CleanupResult, MatchItem, WriteOffFilter, WriteOffInput};

handlers![
    list_unapplied_credits,
    apply_cleanup_credits,
    list_undeposited_review,
    match_undeposited_funds,
    unmatch_undeposited_funds,
    preview_write_offs,
    write_off_invoices,
    list_list_changes,
    preview_inventory_comparison,
];

#[tauri::command]
pub async fn list_unapplied_credits(db: State<'_, Db>) -> Result<cleanup_books::UnappliedReview, String> {
    cleanup_books::unapplied(&db.pool().await).await
}

#[tauri::command]
pub async fn apply_cleanup_credits(app: AppHandle, db: State<'_, Db>, input: ApplyCreditsInput) -> Result<CleanupResult, String> {
    let done = cleanup_books::apply_credits(&db.pool().await, &input).await?;
    notify(&app);
    Ok(done)
}

#[tauri::command]
pub async fn list_undeposited_review(db: State<'_, Db>) -> Result<cleanup_books::UndepositedReview, String> {
    cleanup_books::undeposited(&db.pool().await).await
}

#[tauri::command]
pub async fn match_undeposited_funds(app: AppHandle, db: State<'_, Db>, items: Vec<MatchItem>) -> Result<CleanupResult, String> {
    let done = cleanup_books::match_undeposited(&db.pool().await, &items).await?;
    notify(&app);
    Ok(done)
}

#[tauri::command]
pub async fn unmatch_undeposited_funds(app: AppHandle, db: State<'_, Db>, match_id: i64) -> Result<CleanupResult, String> {
    let done = cleanup_books::unmatch_undeposited(&db.pool().await, match_id).await?;
    notify(&app);
    Ok(done)
}

#[tauri::command]
pub async fn preview_write_offs(db: State<'_, Db>, filter: WriteOffFilter) -> Result<Vec<cleanup_books::WriteOffCandidate>, String> {
    cleanup_books::write_off_candidates(&db.pool().await, &filter).await
}

#[tauri::command]
pub async fn write_off_invoices(app: AppHandle, db: State<'_, Db>, input: WriteOffInput) -> Result<cleanup_books::WriteOffResult, String> {
    let done = cleanup_books::write_off(&db.pool().await, &input).await?;
    notify(&app);
    Ok(done)
}

#[tauri::command]
pub async fn list_list_changes(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<cleanup_books::ListChange>, String> {
    cleanup_books::list_changes(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn preview_inventory_comparison(db: State<'_, Db>, as_of: NaiveDate) -> Result<cleanup_books::InventoryComparison, String> {
    cleanup_books::compare_inventory(&db.pool().await, as_of).await
}
