//! Full-ledger import from desktop company files, through a staging sandbox.
//!
//! This is the fallback for files whose invoice lines openqbw cannot find (see `qbw_import`):
//! [`crate::qbw_company::read_ledger`] decodes the file's chart of accounts and every posting
//! line, and this module stages, maps and posts them.
//!
//! 1. [`stage`] stores the source accounts, transactions and lines in `ledger_import_*`, outside
//!    the ledger.
//! 2. Each source account is mapped to a LocalBooks account ([`set_account`]), or
//!    [`auto_map_accounts`] matches existing accounts by name and type (receivables and payables
//!    go to the system accounts) and creates the rest with their own type and parent.
//! 3. [`post_batch`] posts every balanced source transaction as one journal entry, all inside a
//!    single database transaction. Transactions with a line that could not be read do not
//!    balance; they are listed for manual entry and never posted.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::ledger::{self, db_err, EntrySource, JournalEntry, JournalLine};
use crate::qbw_company::Ledger;

/// Description given to accounts the importer creates (also how a reversal finds them).
const CREATED_NOTE: &str = "Created by the desktop migration wizard from";

/// Identifies a company's transaction set: the same books read twice give the same value.
fn fingerprint(ledger: &Ledger) -> String {
    let mut hash = Sha256::new();
    for t in ledger.transactions.iter().chain(&ledger.unbalanced) {
        hash.update(t.id.to_le_bytes());
        hash.update(t.date.to_string().as_bytes());
        for l in &t.lines {
            hash.update(l.account_id.to_le_bytes());
            hash.update(l.cents.to_le_bytes());
        }
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Stores a ledger read as a new staging batch and returns its id.
pub async fn stage(pool: &SqlitePool, source_path: &Path, file_size_bytes: i64, ledger: &Ledger) -> Result<i64, String> {
    let q = &ledger.quality;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let file_name = source_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let batch_id: i64 = sqlx::query_scalar(
        "INSERT INTO ledger_import_batches
            (source_path, file_name, file_size_bytes, fingerprint, row_pages, decoded_pages, unreadable_lines,
             rejected_lines, accounts_matching_stored, accounts_with_stored_balance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
    )
    .bind(source_path.display().to_string())
    .bind(&file_name)
    .bind(file_size_bytes)
    .bind(fingerprint(ledger))
    .bind(q.pages.row_pages as i64)
    .bind(q.pages.decoded_pages as i64)
    .bind(q.unreadable_lines as i64)
    .bind(q.rejected_lines as i64)
    .bind(q.accounts_matching_stored as i64)
    .bind(q.accounts_with_stored_balance as i64)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;

    for a in &ledger.accounts {
        sqlx::query(
            "INSERT INTO ledger_import_accounts
                (batch_id, source_id, name, full_name, account_type, parent_source_id, is_active, stored_balance_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(batch_id)
        .bind(a.id)
        .bind(&a.name)
        .bind(&a.full_name)
        .bind(a.account_type)
        .bind(a.parent_id)
        .bind(a.active as i64)
        .bind(a.stored_balance_cents)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    for t in ledger.transactions.iter().chain(&ledger.unbalanced) {
        sqlx::query(
            "INSERT INTO ledger_import_transactions (batch_id, source_id, kind, date, doc_num, name, memo, imbalance_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(batch_id)
        .bind(t.id)
        .bind(t.kind)
        .bind(t.date.to_string())
        .bind(&t.doc_num)
        .bind(&t.name)
        .bind(&t.memo)
        .bind(t.imbalance_cents())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for (n, l) in t.lines.iter().enumerate() {
            sqlx::query(
                "INSERT INTO ledger_import_lines (batch_id, source_txn_id, line_no, source_account_id, cents, memo)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(batch_id)
            .bind(t.id)
            .bind(n as i64)
            .bind(l.account_id)
            .bind(l.cents)
            .bind(&l.memo)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(batch_id)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SourceAccountView {
    pub source_id: i64,
    pub full_name: String,
    pub account_type: String,
    pub is_active: bool,
    pub stored_balance_cents: Option<i64>,
    /// Total of the lines read for this account (balanced transactions only).
    pub read_balance_cents: i64,
    pub line_count: i64,
    pub account_id: Option<i64>,
    pub created_by_import: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnbalancedView {
    pub kind: String,
    pub date: String,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub imbalance_cents: i64,
}

/// The copy taken just before the import posted, which "Start over" restores.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOverPoint {
    pub file_name: String,
    /// Local time the copy was taken, `YYYY-MM-DDTHH:MM:SS`.
    pub taken_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerBatchSummary {
    pub id: i64,
    pub file_name: String,
    pub source_path: String,
    pub file_size_bytes: i64,
    pub imported_at: String,
    pub posted_at: Option<String>,
    pub row_pages: i64,
    pub decoded_pages: i64,
    pub unreadable_lines: i64,
    pub rejected_lines: i64,
    pub accounts_matching_stored: i64,
    pub accounts_with_stored_balance: i64,
    pub transaction_count: i64,
    pub line_count: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    /// Balanced transactions not yet in the books.
    pub postable_count: i64,
    /// Entries from this import in effect (posted and not voided).
    pub live_entry_count: i64,
    pub by_kind: Vec<(String, i64)>,
    pub accounts: Vec<SourceAccountView>,
    pub unbalanced: Vec<UnbalancedView>,
    /// Present while the pre-import copy exists on disk.
    pub start_over: Option<StartOverPoint>,
}

pub async fn batch_summary(pool: &SqlitePool, batch_id: i64) -> Result<LedgerBatchSummary, String> {
    #[derive(sqlx::FromRow)]
    struct Head {
        file_name: String,
        source_path: String,
        file_size_bytes: i64,
        imported_at: String,
        posted_at: Option<String>,
        row_pages: i64,
        decoded_pages: i64,
        unreadable_lines: i64,
        rejected_lines: i64,
        accounts_matching_stored: i64,
        accounts_with_stored_balance: i64,
        pre_post_backup: Option<String>,
    }
    let head: Head = sqlx::query_as("SELECT * FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("QBW_NO_BATCH: import #{batch_id} does not exist"))?;
    let (transaction_count, first_date, last_date, postable_count, live_entry_count): (i64, Option<String>, Option<String>, i64, i64) =
        sqlx::query_as(
            "SELECT COUNT(*), MIN(t.date), MAX(t.date),
                    COALESCE(SUM(t.imbalance_cents = 0 AND (t.journal_entry_id IS NULL OR e.is_voided = 1)), 0),
                    COALESCE(SUM(t.journal_entry_id IS NOT NULL AND e.is_voided = 0), 0)
             FROM ledger_import_transactions t LEFT JOIN journal_entries e ON e.id = t.journal_entry_id
             WHERE t.batch_id = ?1",
        )
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let line_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_import_lines WHERE batch_id = ?1")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let by_kind: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, COUNT(*) FROM ledger_import_transactions WHERE batch_id = ?1 AND imbalance_cents = 0 GROUP BY kind ORDER BY COUNT(*) DESC",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let accounts: Vec<SourceAccountView> = sqlx::query_as(
        "SELECT a.source_id, a.full_name, a.account_type, a.is_active, a.stored_balance_cents,
                COALESCE((SELECT SUM(l.cents) FROM ledger_import_lines l JOIN ledger_import_transactions t
                          ON t.batch_id = l.batch_id AND t.source_id = l.source_txn_id
                          WHERE l.batch_id = a.batch_id AND l.source_account_id = a.source_id AND t.imbalance_cents = 0), 0) AS read_balance_cents,
                (SELECT COUNT(*) FROM ledger_import_lines l WHERE l.batch_id = a.batch_id AND l.source_account_id = a.source_id) AS line_count,
                a.account_id, a.created_by_import
         FROM ledger_import_accounts a WHERE a.batch_id = ?1 ORDER BY a.full_name COLLATE NOCASE",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let unbalanced: Vec<UnbalancedView> = sqlx::query_as(
        "SELECT kind, date, doc_num, name, imbalance_cents FROM ledger_import_transactions
         WHERE batch_id = ?1 AND imbalance_cents <> 0 ORDER BY date, source_id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(LedgerBatchSummary {
        id: batch_id,
        file_name: head.file_name,
        source_path: head.source_path,
        file_size_bytes: head.file_size_bytes,
        imported_at: head.imported_at,
        posted_at: head.posted_at,
        row_pages: head.row_pages,
        decoded_pages: head.decoded_pages,
        unreadable_lines: head.unreadable_lines,
        rejected_lines: head.rejected_lines,
        accounts_matching_stored: head.accounts_matching_stored,
        accounts_with_stored_balance: head.accounts_with_stored_balance,
        transaction_count,
        line_count,
        first_date,
        last_date,
        postable_count,
        live_entry_count,
        by_kind,
        accounts,
        unbalanced,
        start_over: head.pre_post_backup.as_deref().and_then(start_over_point),
    })
}

fn start_over_point(path: &str) -> Option<StartOverPoint> {
    let meta = std::fs::metadata(path).ok()?;
    let taken: chrono::DateTime<chrono::Local> = meta.modified().ok()?.into();
    Some(StartOverPoint {
        file_name: Path::new(path).file_name()?.to_string_lossy().into_owned(),
        taken_at: taken.format("%Y-%m-%dT%H:%M:%S").to_string(),
    })
}

/// Records the copy of the books taken just before this batch posted.
pub async fn set_pre_post_backup(pool: &SqlitePool, batch_id: i64, path: &Path) -> Result<(), String> {
    sqlx::query("UPDATE ledger_import_batches SET pre_post_backup = ?2 WHERE id = ?1")
        .bind(batch_id)
        .bind(path.display().to_string())
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The pre-import copy "Start over" restores, when it was taken and still exists.
pub async fn pre_post_backup(pool: &SqlitePool, batch_id: i64) -> Result<std::path::PathBuf, String> {
    require_batch(pool, batch_id).await?;
    let path: Option<String> = sqlx::query_scalar("SELECT pre_post_backup FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    match path.map(std::path::PathBuf::from) {
        Some(p) if p.is_file() => Ok(p),
        Some(p) => Err(format!("QBW_NO_START_OVER: the copy taken before this import ({}) is no longer in the backups folder", p.display())),
        None => Err("QBW_NO_START_OVER: this import was posted before LocalBooks kept a copy for Start over; use Reverse posted entries instead".into()),
    }
}

pub async fn list_batches(pool: &SqlitePool) -> Result<Vec<LedgerBatchSummary>, String> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM ledger_import_batches ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(batch_summary(pool, id).await?);
    }
    Ok(out)
}

async fn require_batch(pool: &SqlitePool, batch_id: i64) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    exists.map(|_| ()).ok_or_else(|| format!("QBW_NO_BATCH: import #{batch_id} does not exist"))
}

/// Maps one source account to a LocalBooks account (`None` clears the choice).
pub async fn set_account(pool: &SqlitePool, batch_id: i64, source_id: i64, account_id: Option<i64>) -> Result<(), String> {
    require_batch(pool, batch_id).await?;
    if let Some(id) = account_id {
        let active: Option<i64> = sqlx::query_scalar("SELECT is_active FROM chart_of_accounts WHERE id = ?1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
        match active {
            None => return Err(format!("QBW_BAD_ACCOUNT: account #{id} does not exist")),
            Some(0) => return Err("QBW_BAD_ACCOUNT: that account is inactive".into()),
            Some(_) => {}
        }
    }
    let updated = sqlx::query(
        "UPDATE ledger_import_accounts SET account_id = ?3, created_by_import = 0 WHERE batch_id = ?1 AND source_id = ?2",
    )
    .bind(batch_id)
    .bind(source_id)
    .bind(account_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if updated.rows_affected() == 0 {
        return Err(format!("QBW_NO_ACCOUNT: import #{batch_id} has no source account #{source_id}"));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMapResult {
    pub matched: i64,
    pub created: i64,
}

/// Maps every unmapped source account that has lines: to an existing active account with the same
/// full name and type, receivables and payables to the system accounts, and otherwise to a new
/// account of the same type under the (already mapped) parent. Parents are handled first.
pub async fn auto_map_accounts(pool: &SqlitePool, batch_id: i64) -> Result<AutoMapResult, String> {
    require_batch(pool, batch_id).await?;
    let file_name: String = sqlx::query_scalar("SELECT file_name FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    #[derive(sqlx::FromRow, Clone)]
    struct Source {
        source_id: i64,
        name: String,
        full_name: String,
        account_type: String,
        parent_source_id: Option<i64>,
        is_active: i64,
        account_id: Option<i64>,
        used: i64,
    }
    let sources: Vec<Source> = sqlx::query_as(
        "SELECT a.source_id, a.name, a.full_name, a.account_type, a.parent_source_id, a.is_active, a.account_id,
                EXISTS (SELECT 1 FROM ledger_import_lines l WHERE l.batch_id = a.batch_id AND l.source_account_id = a.source_id) AS used
         FROM ledger_import_accounts a WHERE a.batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let by_id: HashMap<i64, Source> = sources.iter().map(|s| (s.source_id, s.clone())).collect();
    // A parent is needed when any descendant is used.
    let mut needed: HashMap<i64, bool> = HashMap::new();
    for s in sources.iter().filter(|s| s.used != 0) {
        let mut cur = Some(s.source_id);
        while let Some(id) = cur {
            if needed.insert(id, true).is_some() {
                break;
            }
            cur = by_id.get(&id).and_then(|p| p.parent_source_id);
        }
    }
    let depth = |s: &Source| {
        let (mut d, mut cur) = (0, s.parent_source_id);
        while let Some(p) = cur.filter(|_| d < 32) {
            d += 1;
            cur = by_id.get(&p).and_then(|x| x.parent_source_id);
        }
        d
    };
    let mut order: Vec<&Source> = sources.iter().filter(|s| needed.contains_key(&s.source_id)).collect();
    order.sort_by_key(|s| (depth(s), s.full_name.to_lowercase()));

    let mut result = AutoMapResult::default();
    let mut tx = pool.begin().await.map_err(db_err)?;
    let mut mapped: HashMap<i64, i64> = sources.iter().filter_map(|s| Some((s.source_id, s.account_id?))).collect();
    for s in order {
        if mapped.contains_key(&s.source_id) {
            continue;
        }
        let parent = s.parent_source_id.and_then(|p| mapped.get(&p).copied());
        // Same name, type and parent; receivables and payables fold into the system accounts.
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM chart_of_accounts
             WHERE is_active = 1 AND account_type = ?2 AND name = ?1 COLLATE NOCASE AND COALESCE(parent_id, 0) = COALESCE(?3, 0)
             UNION ALL
             SELECT id FROM chart_of_accounts
             WHERE ?2 IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE') AND account_type = ?2 AND is_system = 1
             LIMIT 1",
        )
        .bind(&s.name)
        .bind(&s.account_type)
        .bind(parent)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let account_id = match existing {
            Some(id) => {
                result.matched += 1;
                id
            }
            None => {
                let note = if s.is_active == 0 {
                    format!("{CREATED_NOTE} {file_name} ({}; inactive there)", s.full_name)
                } else {
                    format!("{CREATED_NOTE} {file_name} ({})", s.full_name)
                };
                // A sibling of another type may already use the name.
                let taken: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM chart_of_accounts WHERE name = ?1 COLLATE NOCASE AND COALESCE(parent_id, 0) = COALESCE(?2, 0)",
                )
                .bind(&s.name)
                .bind(parent)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
                let name = if taken.is_some() { format!("{} (imported)", s.name) } else { s.name.clone() };
                let id: i64 = match sqlx::query_scalar(
                    "INSERT INTO chart_of_accounts (name, account_type, parent_id, description) VALUES (?1, ?2, ?3, ?4) RETURNING id",
                )
                .bind(&name)
                .bind(&s.account_type)
                .bind(parent)
                .bind(note)
                .fetch_one(&mut *tx)
                .await
                {
                    Ok(id) => id,
                    Err(e) => return Err(ledger::rollback(tx, format!("{} (account {})", db_err(e), s.full_name)).await),
                };
                result.created += 1;
                id
            }
        };
        let created = existing.is_none() as i64;
        if let Err(e) = sqlx::query(
            "UPDATE ledger_import_accounts SET account_id = ?3, created_by_import = ?4 WHERE batch_id = ?1 AND source_id = ?2",
        )
        .bind(batch_id)
        .bind(s.source_id)
        .bind(account_id)
        .bind(created)
        .execute(&mut *tx)
        .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
        mapped.insert(s.source_id, account_id);
    }
    tx.commit().await.map_err(db_err)?;
    Ok(result)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostLedgerResult {
    pub entries_posted: i64,
    pub lines_posted: i64,
    pub skipped_unbalanced: i64,
    pub skipped_already_posted: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostLedgerInput {
    pub batch_id: i64,
}

fn describe(kind: &str, doc_num: Option<&str>, name: Option<&str>, memo: Option<&str>) -> String {
    let mut parts = vec![match doc_num {
        Some(n) => format!("{kind} #{n}"),
        None => kind.to_string(),
    }];
    parts.extend(name.map(str::to_string));
    parts.extend(memo.map(str::to_string));
    parts.join(" · ")
}

/// Posts every balanced, unposted source transaction of the batch as one journal entry each, all
/// inside a single transaction. Refuses while a used account is unmapped, and when the same books
/// are already posted from another import.
pub async fn post_batch(pool: &SqlitePool, input: &PostLedgerInput) -> Result<PostLedgerResult, String> {
    let batch_id = input.batch_id;
    require_batch(pool, batch_id).await?;
    let duplicate: Option<String> = sqlx::query_scalar(
        "SELECT o.file_name FROM ledger_import_batches b JOIN ledger_import_batches o ON o.fingerprint = b.fingerprint AND o.id <> b.id
         WHERE b.id = ?1 AND EXISTS (
             SELECT 1 FROM ledger_import_transactions t JOIN journal_entries e ON e.id = t.journal_entry_id
             WHERE t.batch_id = o.id AND e.is_voided = 0)
         LIMIT 1",
    )
    .bind(batch_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    if let Some(other) = duplicate {
        return Err(format!("QBW_ALREADY_IMPORTED: these books are already posted from another import of {other}; reverse that import first"));
    }

    let unmapped: Vec<String> = sqlx::query_scalar(
        "SELECT a.full_name FROM ledger_import_accounts a
         WHERE a.batch_id = ?1 AND a.account_id IS NULL
           AND EXISTS (SELECT 1 FROM ledger_import_lines l JOIN ledger_import_transactions t
                       ON t.batch_id = l.batch_id AND t.source_id = l.source_txn_id
                       WHERE l.batch_id = a.batch_id AND l.source_account_id = a.source_id AND t.imbalance_cents = 0)
         ORDER BY a.full_name",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if !unmapped.is_empty() {
        return Err(format!("QBW_UNMAPPED: choose an account for {} before posting ({})", unmapped.len(), unmapped.join(", ")));
    }
    let map: HashMap<i64, i64> = sqlx::query_as::<_, (i64, i64)>(
        "SELECT source_id, account_id FROM ledger_import_accounts WHERE batch_id = ?1 AND account_id IS NOT NULL",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    .into_iter()
    .collect();

    #[derive(sqlx::FromRow)]
    struct Txn {
        source_id: i64,
        kind: String,
        date: String,
        doc_num: Option<String>,
        name: Option<String>,
        memo: Option<String>,
        imbalance_cents: i64,
        live: i64,
    }
    let txns: Vec<Txn> = sqlx::query_as(
        "SELECT t.source_id, t.kind, t.date, t.doc_num, t.name, t.memo, t.imbalance_cents,
                (t.journal_entry_id IS NOT NULL AND e.is_voided = 0) AS live
         FROM ledger_import_transactions t LEFT JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 ORDER BY t.date, t.source_id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut lines: BTreeMap<i64, Vec<(i64, i64, Option<String>)>> = BTreeMap::new();
    for (txn, account, cents, memo) in sqlx::query_as::<_, (i64, i64, i64, Option<String>)>(
        "SELECT source_txn_id, source_account_id, cents, memo FROM ledger_import_lines WHERE batch_id = ?1 ORDER BY source_txn_id, line_no",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?
    {
        lines.entry(txn).or_default().push((account, cents, memo));
    }

    let mut result = PostLedgerResult::default();
    let mut tx = pool.begin().await.map_err(db_err)?;
    for t in txns {
        if t.imbalance_cents != 0 {
            result.skipped_unbalanced += 1;
            continue;
        }
        if t.live != 0 {
            result.skipped_already_posted += 1;
            continue;
        }
        let Some(source_lines) = lines.get(&t.source_id) else { continue };
        let date: NaiveDate = t.date.parse().map_err(|_| format!("QBW_BAD_DATE: staged date '{}' is not a date", t.date))?;
        let entry_lines: Vec<JournalLine> = source_lines
            .iter()
            .map(|(account, cents, memo)| {
                let mut line = if *cents < 0 { JournalLine::debit(map[account], -cents) } else { JournalLine::credit(map[account], *cents) };
                line.memo = memo.clone();
                line
            })
            .collect();
        let entry = JournalEntry {
            id: None,
            date,
            description: describe(&t.kind, t.doc_num.as_deref(), t.name.as_deref(), t.memo.as_deref()),
            lines: entry_lines,
            currency: None,
        };
        let entry_id = match ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
            Ok(id) => id,
            Err(err) => return Err(ledger::rollback(tx, format!("{err} (imported {})", entry.description)).await),
        };
        if let Err(e) = sqlx::query("UPDATE ledger_import_transactions SET journal_entry_id = ?3 WHERE batch_id = ?1 AND source_id = ?2")
            .bind(batch_id)
            .bind(t.source_id)
            .bind(entry_id)
            .execute(&mut *tx)
            .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
        result.entries_posted += 1;
        result.lines_posted += source_lines.len() as i64;
    }
    if result.entries_posted > 0 {
        if let Err(e) = sqlx::query("UPDATE ledger_import_batches SET posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
            .bind(batch_id)
            .execute(&mut *tx)
            .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(result)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseLedgerResult {
    pub entries_voided: i64,
    pub accounts_deactivated: i64,
}

/// Undoes a posted import: voids every entry it posted and deactivates the accounts it created
/// once they have no other activity.
pub async fn reverse_batch(pool: &SqlitePool, batch_id: i64) -> Result<ReverseLedgerResult, String> {
    let entries: Vec<i64> = sqlx::query_scalar(
        "SELECT t.journal_entry_id FROM ledger_import_transactions t JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 AND e.is_voided = 0 ORDER BY t.journal_entry_id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if entries.is_empty() {
        return Err("QBW_NOTHING_TO_REVERSE: this import has no posted entries in effect".into());
    }
    let mut result = ReverseLedgerResult::default();
    for entry in entries {
        ledger::void_transaction(pool, entry).await?;
        result.entries_voided += 1;
    }
    // Children before parents, so a parent is not left with an active child.
    let accounts: Vec<i64> = sqlx::query_scalar(
        "SELECT a.id FROM ledger_import_accounts m JOIN chart_of_accounts a ON a.id = m.account_id
         WHERE m.batch_id = ?1 AND m.created_by_import = 1 AND a.is_active = 1 AND a.is_system = 0
           AND NOT EXISTS (
               SELECT 1 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
               WHERE l.account_id = a.id AND e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL)
         ORDER BY a.parent_id IS NULL, a.id DESC",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for account in accounts {
        sqlx::query("UPDATE chart_of_accounts SET is_active = 0 WHERE id = ?1").bind(account).execute(pool).await.map_err(db_err)?;
        result.accounts_deactivated += 1;
    }
    Ok(result)
}

/// Deletes a batch's staged rows. Refuses while it has posted entries in effect.
pub async fn discard_batch(pool: &SqlitePool, batch_id: i64) -> Result<(), String> {
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ledger_import_transactions t JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 AND e.is_voided = 0",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if live > 0 {
        return Err("QBW_POSTED: reverse this import's posted entries before deleting it".into());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for sql in [
        "DELETE FROM ledger_import_lines WHERE batch_id = ?1",
        "DELETE FROM ledger_import_transactions WHERE batch_id = ?1",
        "DELETE FROM ledger_import_accounts WHERE batch_id = ?1",
        "DELETE FROM ledger_import_batches WHERE id = ?1",
    ] {
        sqlx::query(sql).bind(batch_id).execute(&mut *tx).await.map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::qbw_company::{LedgerQuality, SourceAccount, SourceLine, SourceTransaction};

    fn account(id: i64, name: &str, kind: &'static str, parent: Option<i64>) -> SourceAccount {
        SourceAccount {
            id,
            name: name.into(),
            full_name: name.into(),
            account_type: kind,
            parent_id: parent,
            active: true,
            stored_balance_cents: None,
        }
    }

    fn txn(id: i64, date: &str, lines: &[(i64, i64)]) -> SourceTransaction {
        SourceTransaction {
            id,
            kind: "Check",
            date: date.parse().unwrap(),
            doc_num: Some(id.to_string()),
            name: Some("Vendor".into()),
            memo: None,
            lines: lines.iter().map(|&(account_id, cents)| SourceLine { account_id, cents, memo: None }).collect(),
        }
    }

    fn ledger() -> Ledger {
        Ledger {
            accounts: vec![
                account(1, "Riverside Bank", "BANK", None),
                account(2, "Hosting", "EXPENSE", None),
                account(3, "Domain Names", "EXPENSE", Some(2)),
                account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
                account(5, "Unused", "EXPENSE", None),
            ],
            // A check: debit Internet 120.00, credit Checking. Negative = debit, as in the file.
            transactions: vec![txn(10, "2023-03-01", &[(3, -12_000), (1, 12_000)]), txn(11, "2023-03-02", &[(4, -500), (1, 500)])],
            unbalanced: vec![txn(12, "2023-03-03", &[(3, -700)])],
            quality: LedgerQuality::default(),
        }
    }

    async fn balance(pool: &SqlitePool, account_id: i64) -> i64 {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id WHERE l.account_id = ?1 AND e.status = 'POSTED'",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Full pipeline on a real company file into a fresh test database: set LOCALBOOKS_QBW_CHECK
    /// and run `cargo test --release -- --ignored posts_a_real --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn posts_a_real_company_file() {
        let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
        let ledger = tokio::task::spawn_blocking(move || crate::qbw_company::read_ledger(Path::new(&path))).await.unwrap().unwrap();
        let pool = test_pool().await;
        let started = std::time::Instant::now();
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 0, &ledger).await.unwrap();
        let mapped = auto_map_accounts(&pool, batch).await.unwrap();
        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap();
        println!("staged, mapped {mapped:?}, posted {posted:?} in {:.1}s", started.elapsed().as_secs_f64());

        // LocalBooks' balance per mapped account against the file's stored balance (negated:
        // the file stores debits as negative, LocalBooks reports debit-normal balances).
        let summary = batch_summary(&pool, batch).await.unwrap();
        let mut per_target: HashMap<i64, (i64, Vec<String>)> = HashMap::new();
        for a in summary.accounts.iter().filter(|a| a.line_count > 0) {
            let e = per_target.entry(a.account_id.unwrap()).or_default();
            e.0 += a.stored_balance_cents.unwrap_or(0);
            e.1.push(a.full_name.clone());
        }
        let (mut same, mut differ) = (0, 0);
        for (account, (stored, names)) in &per_target {
            let books = balance(&pool, *account).await;
            if books == -stored {
                same += 1;
            } else {
                differ += 1;
                println!("  {:<45} books {:>14} file {:>14}", names.join(" + "), crate::ledger::format_cents(books), crate::ledger::format_cents(-stored));
            }
        }
        println!("LocalBooks accounts matching the file: {same}, differing: {differ}");
        let total: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM journal_lines").fetch_one(&pool).await.unwrap();
        assert_eq!(total, 0, "the books must balance");
    }

    #[tokio::test]
    async fn start_over_puts_back_the_books_from_just_before_the_post() {
        let dir = std::env::temp_dir().join(format!("localbooks-start-over-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::Db::open_at(dir.join("live.db"), crate::db::test_config(&dir)).await.unwrap();
        let pool = db.pool().await;
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger()).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();
        let err = pre_post_backup(&pool, batch).await.unwrap_err();
        assert!(err.starts_with("QBW_NO_START_OVER"), "{err}");

        // What the post command does: copy the books, post, record the copy.
        let copy = crate::backup::pre_import_snapshot(&db, batch).await.unwrap();
        assert!(copy.file_name().unwrap().to_string_lossy().starts_with(&format!("pre-import-{batch}-")));
        post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap();
        set_pre_post_backup(&pool, batch, &copy).await.unwrap();
        let posted = batch_summary(&pool, batch).await.unwrap();
        assert_eq!((posted.live_entry_count, posted.postable_count), (2, 0));
        assert!(posted.start_over.is_some());
        let kinds: Vec<String> = crate::backup::list_backups(db.backups_dir()).unwrap().into_iter().map(|b| b.kind).collect();
        assert!(kinds.contains(&"PRE_IMPORT".to_string()), "{kinds:?}");

        let result = crate::backup::restore(&db, &pre_post_backup(&pool, batch).await.unwrap()).await.unwrap();
        let pool = db.pool().await;
        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
        assert_eq!(entries, 0, "no entries and no reversals remain");
        let again = batch_summary(&pool, batch).await.unwrap();
        assert_eq!((again.live_entry_count, again.postable_count), (0, 2));
        assert!(again.accounts.iter().filter(|a| a.line_count > 0).all(|a| a.account_id.is_some()), "mapping kept");
        assert!(again.start_over.is_none());
        assert!(Path::new(&result.safety_backup.path).is_file());

        // The restored books can post the import again.
        let reposted = post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap();
        assert_eq!(reposted.entries_posted, 2);

        // A recorded copy that has gone missing is reported, not restored.
        set_pre_post_backup(&pool, batch, &dir.join("gone.db")).await.unwrap();
        assert!(pre_post_backup(&pool, batch).await.unwrap_err().contains("no longer in the backups folder"));
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_ledger_maps_posts_once_and_reverses() {
        let pool = test_pool().await;
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger()).await.unwrap();
        let summary = batch_summary(&pool, batch).await.unwrap();
        assert_eq!((summary.transaction_count, summary.postable_count, summary.unbalanced.len()), (3, 2, 1));

        let err = post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap_err();
        assert!(err.starts_with("QBW_UNMAPPED"), "{err}");

        let mapped = auto_map_accounts(&pool, batch).await.unwrap();
        // Receivables fold into the system account; the bank, the expense and its subaccount are
        // created; the unused account is left alone.
        assert_eq!(mapped, AutoMapResult { matched: 1, created: 3 });
        let accounts = batch_summary(&pool, batch).await.unwrap().accounts;
        let id = |name: &str| accounts.iter().find(|a| a.full_name == name).unwrap().account_id;
        assert!(id("Unused").is_none());
        let domains = id("Domain Names").unwrap();
        let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM chart_of_accounts WHERE id = ?1").bind(domains).fetch_one(&pool).await.unwrap();
        assert_eq!(parent, id("Hosting"));

        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap();
        assert_eq!(posted, PostLedgerResult { entries_posted: 2, lines_posted: 4, skipped_unbalanced: 1, skipped_already_posted: 0 });
        assert_eq!(balance(&pool, domains).await, 12_000);
        assert_eq!(balance(&pool, id("Riverside Bank").unwrap()).await, -12_500);
        let memo: String = sqlx::query_scalar("SELECT memo FROM journal_entries WHERE id = (SELECT journal_entry_id FROM ledger_import_transactions WHERE source_id = 10)")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(memo, "Check #10 · Vendor");

        // Posting again posts nothing new; a second import of the same books is refused.
        let again = post_batch(&pool, &PostLedgerInput { batch_id: batch }).await.unwrap();
        assert_eq!(again.entries_posted, 0);
        let second = stage(&pool, Path::new("/books/Books copy.qbw"), 1, &ledger()).await.unwrap();
        auto_map_accounts(&pool, second).await.unwrap();
        let err = post_batch(&pool, &PostLedgerInput { batch_id: second }).await.unwrap_err();
        assert!(err.starts_with("QBW_ALREADY_IMPORTED"), "{err}");
        discard_batch(&pool, second).await.unwrap();

        assert!(discard_batch(&pool, batch).await.unwrap_err().starts_with("QBW_POSTED"));
        let reversed = reverse_batch(&pool, batch).await.unwrap();
        assert_eq!(reversed.entries_voided, 2);
        assert_eq!(reversed.accounts_deactivated, 3);
        assert_eq!(balance(&pool, domains).await, 0);
        discard_batch(&pool, batch).await.unwrap();
    }
}
