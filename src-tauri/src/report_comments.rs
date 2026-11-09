//! Comments on report figures (gaps row T50).
//!
//! Commenting on a figure keeps the report as it was shown: its title, subtitle, columns and rows
//! (`commented_reports.snapshot_json`). Each comment points at one cell of that copy and keeps the
//! figure's text and its line's label. A commented report is listed under Reports › Commented
//! Reports, opens with its comments numbered beside their figures, and prints with them. The
//! copy never changes when the books do: it is the report the comments were made on.
//!
//! The snapshot is `{ "columns": [{ "label", "kind" }], "rows": [{ "kind", "depth", "cells": [...] }] }`,
//! the report engine's shape ([`crate::custom_reports::CustomReport`]); the financial statements
//! are sent in the same shape.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// A commented report, with its comments.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentedReport {
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub snapshot: Value,
    pub created_by: String,
    pub created_at: String,
    pub comments: Vec<ReportComment>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReportComment {
    pub id: i64,
    pub row_index: i64,
    pub col_index: i64,
    pub figure: String,
    pub line_label: String,
    pub body: String,
    pub author: String,
    pub created_at: String,
}

/// A row of Reports › Commented Reports.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CommentedReportSummary {
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub created_by: String,
    pub created_at: String,
    pub comment_count: i64,
    pub last_comment_at: Option<String>,
}

/// A comment to add: on the report `report_id`, or (the first comment) on a new copy of the report
/// given by `title`, `subtitle` and `snapshot`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentInput {
    #[serde(default)]
    pub report_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub snapshot: Option<Value>,
    pub row: i64,
    pub col: i64,
    /// The figure as shown on screen.
    pub figure: String,
    pub body: String,
}

/// The snapshot's rows and columns, checked to be the report shape.
fn shape(snapshot: &Value) -> Result<(&Vec<Value>, &Vec<Value>), String> {
    let bad = || "COMMENT_BAD_REPORT: the report could not be kept".to_string();
    let columns = snapshot.get("columns").and_then(Value::as_array).filter(|c| !c.is_empty()).ok_or_else(bad)?;
    let rows = snapshot.get("rows").and_then(Value::as_array).ok_or_else(bad)?;
    let text = |v: &Value, key: &str| v.get(key).is_some_and(Value::is_string);
    if columns.iter().any(|c| !text(c, "label") || !text(c, "kind")) || rows.iter().any(|r| r.get("cells").is_none_or(|c| !c.is_array())) {
        return Err(bad());
    }
    Ok((columns, rows))
}

/// The label of row `row` (its first cell), and whether cell `col` holds a figure.
fn cell(snapshot: &Value, row: i64, col: i64) -> Result<String, String> {
    let (columns, rows) = shape(snapshot)?;
    let missing = || "COMMENT_NO_FIGURE: there is no figure there to comment on".to_string();
    let r = rows.get(usize::try_from(row).map_err(|_| missing())?).ok_or_else(missing)?;
    let c = usize::try_from(col).map_err(|_| missing())?;
    // The first column holds the line's name, not a figure.
    if c == 0 || c >= columns.len() {
        return Err(missing());
    }
    let cells = r.get("cells").and_then(Value::as_array).ok_or_else(missing)?;
    if cells.get(c).is_none_or(Value::is_null) {
        return Err(missing());
    }
    Ok(cells.first().and_then(Value::as_str).unwrap_or_default().trim().chars().take(200).collect())
}

/// Adds a comment on a figure, keeping the report first when this is its first comment. Returns
/// the commented report with all its comments.
pub async fn add_comment(pool: &SqlitePool, input: &CommentInput, author: &str) -> Result<CommentedReport, String> {
    let body = input.body.trim();
    if body.is_empty() {
        return Err("COMMENT_EMPTY: write the comment".into());
    }
    if body.chars().count() > 2000 {
        return Err("COMMENT_TOO_LONG: keep a comment under 2,000 characters".into());
    }
    let figure: String = input.figure.trim().chars().take(100).collect();
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<i64, String> = async {
        let (report_id, snapshot) = match input.report_id {
            Some(id) => {
                let text: String = sqlx::query_scalar("SELECT snapshot_json FROM commented_reports WHERE id = ?1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(|| "COMMENT_NO_REPORT: that commented report is gone".to_string())?;
                (id, serde_json::from_str::<Value>(&text).map_err(|_| "COMMENT_BAD_REPORT: the kept report is damaged".to_string())?)
            }
            None => {
                let title = input.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| "COMMENT_BAD_REPORT: the report has no title".to_string())?;
                let snapshot = input.snapshot.clone().ok_or_else(|| "COMMENT_BAD_REPORT: the report could not be kept".to_string())?;
                shape(&snapshot)?;
                let id: i64 = sqlx::query_scalar("INSERT INTO commented_reports (title, subtitle, snapshot_json, created_by) VALUES (?1, ?2, ?3, ?4) RETURNING id")
                    .bind(title.chars().take(200).collect::<String>())
                    .bind(input.subtitle.as_deref().unwrap_or_default().chars().take(500).collect::<String>())
                    .bind(snapshot.to_string())
                    .bind(author)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_err)?;
                (id, snapshot)
            }
        };
        let line = cell(&snapshot, input.row, input.col)?;
        sqlx::query("INSERT INTO report_comments (commented_report_id, row_index, col_index, figure, line_label, body, author) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .bind(report_id)
            .bind(input.row)
            .bind(input.col)
            .bind(&figure)
            .bind(line)
            .bind(body)
            .bind(author)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok(report_id)
    }
    .await;
    let report_id = match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            id
        }
        Err(e) => return Err(crate::ledger::rollback(tx, e).await),
    };
    get(pool, report_id).await
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<CommentedReport, String> {
    let row: (i64, String, String, String, String, String) =
        sqlx::query_as("SELECT id, title, subtitle, snapshot_json, created_by, created_at FROM commented_reports WHERE id = ?1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .ok_or_else(|| "COMMENT_NO_REPORT: that commented report is gone".to_string())?;
    let comments: Vec<ReportComment> = sqlx::query_as(
        "SELECT id, row_index, col_index, figure, line_label, body, author, created_at FROM report_comments
         WHERE commented_report_id = ?1 ORDER BY row_index, col_index, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let snapshot = serde_json::from_str(&row.3).map_err(|_| "COMMENT_BAD_REPORT: the kept report is damaged".to_string())?;
    Ok(CommentedReport { id: row.0, title: row.1, subtitle: row.2, snapshot, created_by: row.4, created_at: row.5, comments })
}

/// Commented reports, the most recently commented first.
pub async fn list(pool: &SqlitePool) -> Result<Vec<CommentedReportSummary>, String> {
    sqlx::query_as(
        "SELECT r.id, r.title, r.subtitle, r.created_by, r.created_at,
                (SELECT COUNT(*) FROM report_comments c WHERE c.commented_report_id = r.id) AS comment_count,
                (SELECT MAX(created_at) FROM report_comments c WHERE c.commented_report_id = r.id) AS last_comment_at
         FROM commented_reports r ORDER BY COALESCE(last_comment_at, r.created_at) DESC, r.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Removes one comment. A report left without comments stays listed until it is deleted.
pub async fn delete_comment(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM report_comments WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// Removes a commented report and its comments. The books are not affected.
pub async fn delete_report(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM commented_reports WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests;
