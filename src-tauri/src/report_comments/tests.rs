use super::*;
use crate::db::test_pool;
use serde_json::json;

fn snapshot() -> Value {
    json!({
        "columns": [{ "label": "", "kind": "text" }, { "label": "Total", "kind": "money" }],
        "rows": [
            { "kind": "section", "depth": 0, "cells": ["Income", null] },
            { "kind": "detail", "depth": 1, "cells": ["Design services", 1_250_000] },
            { "kind": "total", "depth": 0, "cells": ["Total Income", 1_250_000] }
        ]
    })
}

fn first(row: i64, col: i64, body: &str) -> CommentInput {
    CommentInput { report_id: None, title: Some("Profit and Loss".into()), subtitle: Some("January 2026".into()), snapshot: Some(snapshot()), row, col, figure: "$12,500.00".into(), body: body.into() }
}

#[tokio::test]
async fn the_first_comment_keeps_the_report_and_more_attach_to_it() {
    let pool = test_pool().await;
    let report = add_comment(&pool, &first(1, 1, "Includes the Quillfeather retainer"), "Dana").await.unwrap();
    assert_eq!((report.title.as_str(), report.subtitle.as_str(), report.created_by.as_str()), ("Profit and Loss", "January 2026", "Dana"));
    assert_eq!(report.snapshot, snapshot(), "kept exactly as shown");
    let c = &report.comments[0];
    assert_eq!((c.row_index, c.col_index, c.figure.as_str(), c.line_label.as_str(), c.body.as_str()), (1, 1, "$12,500.00", "Design services", "Includes the Quillfeather retainer"));

    let more = CommentInput { report_id: Some(report.id), title: None, subtitle: None, snapshot: None, row: 2, col: 1, figure: "$12,500.00".into(), body: "Up 8% on December".into() };
    let report = add_comment(&pool, &more, "Sam").await.unwrap();
    assert_eq!(report.comments.len(), 2);
    assert_eq!(report.comments[1].line_label, "Total Income");

    let listed = list(&pool).await.unwrap();
    assert_eq!((listed.len(), listed[0].comment_count, listed[0].title.as_str()), (1, 2, "Profit and Loss"));

    delete_comment(&pool, report.comments[0].id).await.unwrap();
    assert_eq!(get(&pool, report.id).await.unwrap().comments.len(), 1);
    delete_report(&pool, report.id).await.unwrap();
    assert!(list(&pool).await.unwrap().is_empty());
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_comments").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0, "its comments go with it");
}

#[tokio::test]
async fn comments_need_a_figure_and_words_and_a_bad_one_keeps_nothing() {
    let pool = test_pool().await;
    for (row, col, why) in [(0, 1, "a heading's empty cell"), (9, 1, "no such line"), (1, 5, "no such column"), (-1, 0, "negative")] {
        assert!(add_comment(&pool, &first(row, col, "x"), "Dana").await.unwrap_err().starts_with("COMMENT_NO_FIGURE"), "{why}");
    }
    assert!(add_comment(&pool, &first(1, 1, "   "), "Dana").await.unwrap_err().starts_with("COMMENT_EMPTY"));
    assert!(add_comment(&pool, &first(1, 1, &"x".repeat(2001)), "Dana").await.unwrap_err().starts_with("COMMENT_TOO_LONG"));
    let mut shapeless = first(1, 1, "x");
    shapeless.snapshot = Some(json!({ "rows": [] }));
    assert!(add_comment(&pool, &shapeless, "Dana").await.unwrap_err().starts_with("COMMENT_BAD_REPORT"));
    let gone = CommentInput { report_id: Some(99), ..first(1, 1, "x") };
    assert!(add_comment(&pool, &gone, "Dana").await.unwrap_err().starts_with("COMMENT_NO_REPORT"));
    assert!(list(&pool).await.unwrap().is_empty(), "a refused first comment keeps no report");
}
