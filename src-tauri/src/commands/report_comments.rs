//! Comments on report figures: commented reports and their comments.

use super::prelude::*;
use crate::report_comments::{self, CommentInput, CommentedReport, CommentedReportSummary};

handlers![comment_on_report_figure, get_commented_report, list_commented_reports, delete_report_comment, delete_commented_report];

#[tauri::command]
pub async fn comment_on_report_figure(app: AppHandle, db: State<'_, Db>, input: CommentInput) -> Result<CommentedReport, String> {
    let (pool, session) = signed_in(&db).await?;
    let author = session.as_ref().map_or_else(|| "this computer".to_string(), |s| s.user.display_name.clone());
    drop(session);
    let report = report_comments::add_comment(&pool, &input, &author).await?;
    notify(&app);
    Ok(report)
}

#[tauri::command]
pub async fn get_commented_report(db: State<'_, Db>, id: i64) -> Result<CommentedReport, String> {
    report_comments::get(&db.pool().await, id).await
}

#[tauri::command]
pub async fn list_commented_reports(db: State<'_, Db>) -> Result<Vec<CommentedReportSummary>, String> {
    report_comments::list(&db.pool().await).await
}

#[tauri::command]
pub async fn delete_report_comment(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    report_comments::delete_comment(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_commented_report(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    report_comments::delete_report(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
