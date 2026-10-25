//! Hash chain over the ledger audit trail (see migration 0041).
//!
//! Triggers write `ledger_audit_log` rows unsealed. Before a write transaction commits,
//! [`seal`] stamps each new row with this computer's actor and links it into a SHA-256 chain:
//! `row_hash = sha256(json [prev_hash, id, timestamp, action_type, target_entity_id, actor, payload])`,
//! with the first row chained to 64 zeros. After the commit, [`anchor`] records the newest sealed row
//! (id and hash) outside the company file, in the operating system's credential store, keyed by the
//! file's `ledger_chain.file_id`.
//!
//! [`verify`] recomputes the chain and compares it with that record, so rewriting or removing audit
//! rows is detected even by someone who dropped the protecting triggers and rebuilt the chain inside
//! the file. A file this computer has no record for (new, moved here, or its identity changed) can
//! only be checked for internal consistency; the verification says so.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use tokio::sync::Mutex;

use crate::ledger::db_err;
use crate::secrets::SecretStore;

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const KEYCHAIN_SERVICE: &str = "LocalBooks audit trail";
const ANCHORS_FILE: &str = "audit-anchors.json";

static ANCHORS: OnceLock<SecretStore> = OnceLock::new();
/// The record per file id as this process last read or wrote it, so commits never move it back.
static ANCHORED: Mutex<Option<HashMap<String, Record>>> = Mutex::const_new(None);

/// What this computer keeps about one file's chain: its newest sealed row, and the day this computer
/// first started keeping the record. Only changes made after that day can be checked against it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    row_id: i64,
    row_hash: String,
    since: String,
}

impl Record {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.row_id, self.row_hash, self.since)
    }

    fn decode(value: &str) -> Result<Record, String> {
        let damaged = || "AUDIT_ANCHOR_DAMAGED: the audit trail record on this computer is unreadable".to_string();
        let mut parts = value.splitn(3, ':');
        let row_id = parts.next().and_then(|id| id.parse().ok()).ok_or_else(damaged)?;
        let row_hash = parts.next().filter(|h| h.len() == 64).ok_or_else(damaged)?.to_string();
        let since = parts.next().unwrap_or_default().to_string();
        Ok(Record { row_id, row_hash, since })
    }
}

/// Where chain heads are recorded. Installed once at startup; without it (tests, the command-line
/// tools) rows are still sealed but nothing is recorded outside the file.
pub fn install_anchor_store(config_dir: &std::path::Path) {
    let _ = ANCHORS.set(SecretStore::for_this_computer(config_dir, KEYCHAIN_SERVICE, ANCHORS_FILE, "the audit trail record"));
}

#[cfg(test)]
thread_local! {
    /// Tests record heads per test thread, so parallel tests never share a record file.
    static TEST_ANCHORS: std::cell::RefCell<Option<SecretStore>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn install_test_anchor_store(path: std::path::PathBuf) {
    TEST_ANCHORS.with(|s| *s.borrow_mut() = Some(SecretStore::in_file(path, KEYCHAIN_SERVICE, "the audit trail record")));
}

fn anchor_store() -> Option<SecretStore> {
    #[cfg(test)]
    if let Some(store) = TEST_ANCHORS.with(|s| s.borrow().clone()) {
        return Some(store);
    }
    ANCHORS.get().cloned()
}

/// The LocalBooks user signed in to the open company file, set by [`crate::db::Db::refresh_gate`].
/// One company file is open per process, so one value serves every write.
#[cfg(not(test))]
static SIGNED_IN: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

#[cfg(test)]
thread_local! {
    /// Tests open and close company files in parallel, each on its own thread.
    static SIGNED_IN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub fn set_signed_in(username: Option<String>) {
    #[cfg(not(test))]
    {
        *SIGNED_IN.write().expect("signed-in lock") = username;
    }
    #[cfg(test)]
    SIGNED_IN.with(|s| *s.borrow_mut() = username);
}

fn signed_in() -> Option<String> {
    #[cfg(not(test))]
    return SIGNED_IN.read().expect("signed-in lock").clone();
    #[cfg(test)]
    SIGNED_IN.with(|s| s.borrow().clone())
}

/// Who is writing: the signed-in LocalBooks user when the file has users, then the operating-system
/// user and the app version.
fn actor() -> String {
    static COMPUTER: OnceLock<String> = OnceLock::new();
    let computer = COMPUTER.get_or_init(|| {
        let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "unknown user".into());
        format!("{user} (LocalBooks {})", env!("CARGO_PKG_VERSION"))
    });
    match signed_in().as_deref() {
        Some(username) => format!("{username} as {computer}"),
        None => computer.clone(),
    }
}

fn row_hash(prev: &str, id: i64, timestamp: &str, action: &str, target: i64, actor: &str, payload: &str) -> String {
    let canonical = serde_json::json!([prev, id, timestamp, action, target, actor, payload]).to_string();
    Sha256::digest(canonical.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The newest sealed row of a file, to record outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainHead {
    pub file_id: String,
    pub row_id: i64,
    pub row_hash: String,
}

type AuditRow = (i64, String, String, i64, Option<String>, String, Option<String>, Option<String>);

const ROW_SELECT: &str =
    "SELECT id, timestamp, action_type, target_entity_id, actor, payload_snapshot, prev_hash, row_hash FROM ledger_audit_log";

/// Seals every unsealed audit row, in id order, inside the caller's transaction. Returns the chain
/// head, or `None` when the file has no audit rows yet.
pub async fn seal(conn: &mut SqliteConnection) -> Result<Option<ChainHead>, String> {
    let file_id: String = sqlx::query_scalar("SELECT file_id FROM ledger_chain WHERE id = 1").fetch_one(&mut *conn).await.map_err(db_err)?;
    let unsealed: Vec<AuditRow> =
        sqlx::query_as(&format!("{ROW_SELECT} WHERE row_hash IS NULL ORDER BY id")).fetch_all(&mut *conn).await.map_err(db_err)?;
    for (id, timestamp, action, target, row_actor, payload, _, _) in unsealed {
        let prev: Option<String> = sqlx::query_scalar("SELECT row_hash FROM ledger_audit_log WHERE id < ?1 ORDER BY id DESC LIMIT 1")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db_err)?
            .flatten();
        let prev = match prev {
            Some(prev) => prev,
            None if !sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM ledger_audit_log WHERE id < ?1)")
                .bind(id)
                .fetch_one(&mut *conn)
                .await
                .map_err(db_err)? =>
            {
                GENESIS.to_string()
            }
            None => return Err(format!("AUDIT_CHAIN_GAP: audit record {id} follows an unsealed record")),
        };
        let row_actor = row_actor.unwrap_or_else(actor);
        let hash = row_hash(&prev, id, &timestamp, &action, target, &row_actor, &payload);
        sqlx::query("UPDATE ledger_audit_log SET actor = ?1, prev_hash = ?2, row_hash = ?3 WHERE id = ?4 AND row_hash IS NULL")
            .bind(&row_actor)
            .bind(&prev)
            .bind(&hash)
            .bind(id)
            .execute(&mut *conn)
            .await
            .map_err(db_err)?;
    }
    let head: Option<(i64, String)> =
        sqlx::query_as("SELECT id, row_hash FROM ledger_audit_log WHERE row_hash IS NOT NULL ORDER BY id DESC LIMIT 1")
            .fetch_optional(&mut *conn)
            .await
            .map_err(db_err)?;
    Ok(head.map(|(row_id, row_hash)| ChainHead { file_id, row_id, row_hash }))
}

/// Whether the chain in this transaction still contains the head this computer last recorded for the
/// file. Only then may the record move forward: after records were removed, SQLite reuses their ids,
/// so advancing blindly would overwrite the evidence. Call inside the write transaction, after [`seal`].
pub async fn continues_record(conn: &mut SqliteConnection, head: &Option<ChainHead>) -> Result<bool, String> {
    let Some(head) = head else { return Ok(true) };
    let Some(Record { row_id, row_hash, .. }) = recorded_cached(&head.file_id).await? else { return Ok(true) };
    let current: Option<String> = sqlx::query_scalar("SELECT row_hash FROM ledger_audit_log WHERE id = ?1")
        .bind(row_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .flatten();
    Ok(current.as_deref() == Some(row_hash.as_str()))
}

/// Records `head` outside the company file. Best effort: the commit has already happened, and a
/// record that lags behind only means the newest rows are checked for consistency alone.
pub async fn anchor(head: Option<ChainHead>) {
    if let Some(head) = head {
        if let Err(err) = record(&head, false).await {
            eprintln!("LocalBooks: could not record the audit trail head: {err}");
        }
    }
}

async fn record(head: &ChainHead, replace: bool) -> Result<(), String> {
    let Some(store) = anchor_store() else { return Ok(()) };
    let mut anchored = ANCHORED.lock().await;
    let anchored = anchored.get_or_insert_with(HashMap::new);
    let known = match anchored.get(&head.file_id) {
        Some(known) => Some(known.clone()),
        None => recorded(&head.file_id).await?,
    };
    if !replace && known.as_ref().is_some_and(|k| k.row_id >= head.row_id) {
        return Ok(());
    }
    // The day the record was started stays put, even when a restore moves the head back.
    let since = known.map(|k| k.since).filter(|s| !s.is_empty()).unwrap_or_else(|| chrono::Local::now().date_naive().to_string());
    let record = Record { row_id: head.row_id, row_hash: head.row_hash.clone(), since };
    store.put(&head.file_id, &record.encode()).await?;
    anchored.insert(head.file_id.clone(), record);
    Ok(())
}

/// Seals and records the current head unconditionally: after the app itself replaces the company
/// file (restoring a backup), whose trail may legitimately be behind the previous record.
pub async fn reanchor(pool: &SqlitePool) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let head = seal(&mut tx).await?;
    tx.commit().await.map_err(db_err)?;
    match head {
        Some(head) => record(&head, true).await,
        None => Ok(()),
    }
}

/// The record for `file_id`, read from the store once per process and then kept in memory.
async fn recorded_cached(file_id: &str) -> Result<Option<Record>, String> {
    if anchor_store().is_none() {
        return Ok(None);
    }
    let mut anchored = ANCHORED.lock().await;
    let anchored = anchored.get_or_insert_with(HashMap::new);
    if let Some(known) = anchored.get(file_id) {
        return Ok(Some(known.clone()));
    }
    let stored = recorded(file_id).await?;
    if let Some(stored) = &stored {
        anchored.insert(file_id.to_string(), stored.clone());
    }
    Ok(stored)
}

async fn recorded(file_id: &str) -> Result<Option<Record>, String> {
    let Some(store) = anchor_store() else { return Ok(None) };
    store.get(file_id).await?.map(|value| Record::decode(&value)).transpose()
}

/// Outcome of checking the chain.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainVerification {
    pub records_sealed: i64,
    /// Rows written but not sealed yet (only the newest rows, outside a write transaction).
    pub records_pending: i64,
    /// The day this computer started keeping a record of the chain (YYYY-MM-DD), when it has one.
    /// Changes made before then cannot be checked against it, only for internal consistency.
    pub recorded_since: Option<String>,
    /// (audit row id, problem)
    pub problems: Vec<(i64, String)>,
}

/// Recomputes every sealed row and compares the chain with the record kept on this computer.
pub async fn verify(pool: &SqlitePool) -> Result<ChainVerification, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let file_id: String = sqlx::query_scalar("SELECT file_id FROM ledger_chain WHERE id = 1").fetch_one(&mut *conn).await.map_err(db_err)?;
    let rows: Vec<AuditRow> = sqlx::query_as(&format!("{ROW_SELECT} ORDER BY id")).fetch_all(&mut *conn).await.map_err(db_err)?;

    let mut problems = Vec::new();
    let (mut sealed, mut pending) = (0, 0);
    let mut prev = GENESIS.to_string();
    let mut prev_was_pending = false;
    let mut hashes: HashMap<i64, String> = HashMap::with_capacity(rows.len());
    for (id, timestamp, action, target, row_actor, payload, prev_hash, stored) in rows {
        let Some(stored) = stored else {
            pending += 1;
            prev_was_pending = true;
            continue;
        };
        sealed += 1;
        if prev_was_pending {
            problems.push((id, "Sealed after an unsealed audit record (the chain was cut)".to_string()));
        }
        if prev_hash.as_deref() != Some(prev.as_str()) {
            problems.push((id, "Audit record does not link to the one before it (records removed, inserted or reordered)".to_string()));
        }
        let expected = row_hash(prev_hash.as_deref().unwrap_or(GENESIS), id, &timestamp, &action, target, row_actor.as_deref().unwrap_or(""), &payload);
        if expected != stored {
            problems.push((id, "Audit record was changed after it was sealed".to_string()));
        }
        hashes.insert(id, stored.clone());
        prev = stored;
        prev_was_pending = false;
    }

    let anchor = recorded(&file_id).await?;
    if let Some(Record { row_id: anchored_id, row_hash: anchored_hash, .. }) = &anchor {
        match hashes.get(anchored_id) {
            None => problems.push((
                *anchored_id,
                "Audit record missing: this computer recorded the trail up to here, so records were removed or this is an older copy of the file".to_string(),
            )),
            Some(hash) if hash != anchored_hash => problems.push((
                *anchored_id,
                "Audit trail differs from the one this computer recorded: it was rewritten, or this is a copy that has diverged".to_string(),
            )),
            Some(_) => {}
        }
    }
    problems.sort();
    problems.dedup();
    let recorded_since = anchor.map(|a| a.since).map(|s| if s.is_empty() { "an unknown date".to_string() } else { s });
    Ok(ChainVerification { records_sealed: sealed, records_pending: pending, recorded_since, problems })
}

#[cfg(test)]
mod tests {
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
}
