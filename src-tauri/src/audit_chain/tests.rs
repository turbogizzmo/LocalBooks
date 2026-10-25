use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

async fn post(pool: &SqlitePool, cents: i64) -> i64 {
    let id = |n: &'static str| async move {
        sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(pool).await.unwrap()
    };
    let (checking, sales) = (id("1010").await, id("4000").await);
    post_journal_entry(
        pool,
        JournalEntry {
            id: None,
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            description: "Sale".into(),
            lines: vec![JournalLine::debit(checking, cents), JournalLine::credit(sales, cents)],
            currency: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn every_write_seals_its_audit_rows_with_an_actor() {
    let pool = test_pool().await;
    post(&pool, 1_000).await;
    let unsealed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE row_hash IS NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(unsealed, 0, "the commit sealed the migration records and the new POST");
    let actor: String = sqlx::query_scalar("SELECT actor FROM ledger_audit_log WHERE action_type = 'POST'").fetch_one(&pool).await.unwrap();
    assert!(actor.contains("LocalBooks"), "{actor}");
    let chain = verify(&pool).await.unwrap();
    assert!(chain.problems.is_empty(), "{:?}", chain.problems);
    assert!(chain.records_sealed > 1);

    // The database refuses a forged seal or a second seal.
    let err = sqlx::query("UPDATE ledger_audit_log SET row_hash = ?1 WHERE action_type = 'POST'")
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(db_err(err).starts_with("AUDIT_IMMUTABLE"));
    let err = sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot, prev_hash, row_hash) VALUES ('POST', 1, '{}', ?1, ?1)")
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(db_err(err).starts_with("AUDIT_IMMUTABLE"));
}

#[tokio::test]
async fn an_unreadable_record_never_blocks_a_write() {
    let dir = std::env::temp_dir().join(format!("localbooks-anchor-damaged-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(ANCHORS_FILE);
    std::fs::write(&path, b"not json").unwrap();
    install_test_anchor_store(path);
    let pool = test_pool().await;
    post(&pool, 1_000).await;
    post(&pool, 2_000).await;
    let unsealed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE row_hash IS NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(unsealed, 0, "rows are still sealed even though the record can't be read");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rewriting_the_trail_inside_the_file_breaks_the_chain() {
    let pool = test_pool().await;
    let first = post(&pool, 1_000).await;
    post(&pool, 2_000).await;

    sqlx::query("DROP TRIGGER trg_ledger_audit_no_update").execute(&pool).await.unwrap();
    sqlx::query("UPDATE ledger_audit_log SET payload_snapshot = json_set(payload_snapshot, '$.memo', 'Edited') WHERE action_type = 'POST' AND target_entity_id = ?1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    let chain = verify(&pool).await.unwrap();
    assert!(chain.problems.iter().any(|(_, p)| p == "Audit record was changed after it was sealed"), "{:?}", chain.problems);
}

#[tokio::test]
async fn the_record_outside_the_file_catches_a_rebuilt_chain_and_truncation() {
    let dir = std::env::temp_dir().join(format!("localbooks-anchor-{}-{:?}", std::process::id(), std::thread::current().id()));
    install_test_anchor_store(dir.join(ANCHORS_FILE));
    let pool = test_pool().await;
    post(&pool, 1_000).await;
    let last = post(&pool, 2_000).await;
    let chain = verify(&pool).await.unwrap();
    assert!(chain.recorded_since.is_some() && chain.problems.is_empty(), "{chain:?}");
    assert_eq!(chain.recorded_since.as_deref(), Some(chrono::Local::now().date_naive().to_string().as_str()));

    // Remove the newest POST record and rebuild the hashes so the chain is internally consistent.
    sqlx::query("DROP TRIGGER trg_ledger_audit_no_delete").execute(&pool).await.unwrap();
    sqlx::query("DROP TRIGGER trg_ledger_audit_no_update").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1").bind(last).execute(&pool).await.unwrap();
    sqlx::query("UPDATE ledger_audit_log SET prev_hash = NULL, row_hash = NULL").execute(&pool).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    seal(&mut conn).await.unwrap();
    drop(conn);

    let chain = verify(&pool).await.unwrap();
    assert!(chain.problems.iter().any(|(_, p)| p.starts_with("Audit record missing") || p.starts_with("Audit trail differs")), "{:?}", chain.problems);
    let _ = std::fs::remove_dir_all(dir);
}
