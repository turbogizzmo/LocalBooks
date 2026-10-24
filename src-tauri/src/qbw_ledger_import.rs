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
use crate::qbw_full_import::{FullImportCounts, Outcome, Staged};

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
            "INSERT INTO ledger_import_transactions (batch_id, source_id, kind, date, doc_num, name, memo, imbalance_cents, document)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(batch_id)
        .bind(t.id)
        .bind(t.kind)
        .bind(t.date.to_string())
        .bind(&t.doc_num)
        .bind(&t.name)
        .bind(&t.memo)
        .bind(t.imbalance_cents())
        .bind(t.document.as_ref().map(|d| serde_json::to_string(d).expect("documents serialize")))
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
    for (kind, parties) in [("CUSTOMER", &ledger.customers), ("VENDOR", &ledger.vendors)] {
        for p in parties {
            sqlx::query(
                "INSERT INTO ledger_import_parties
                    (batch_id, kind, source_id, name, parent_source_id, company_name, email, phone, address, terms_days,
                     tax_id, is_1099, is_active, balance_cents, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )
            .bind(batch_id)
            .bind(kind)
            .bind(p.id)
            .bind(&p.name)
            .bind(p.parent_id)
            .bind(&p.company_name)
            .bind(&p.email)
            .bind(&p.phone)
            .bind(&p.address)
            .bind(p.terms_days)
            .bind(&p.tax_id)
            .bind(p.is_1099 as i64)
            .bind(p.active as i64)
            .bind(p.balance_cents)
            .bind(&p.notes)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    }
    for item in &ledger.items {
        sqlx::query(
            "INSERT INTO ledger_import_items
                (batch_id, source_id, name, description, rate_cents, income_source_account_id, item_kind, is_taxable, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(batch_id)
        .bind(item.id)
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.rate_cents)
        .bind(item.income_account_id)
        .bind(item.kind)
        .bind(item.taxable as i64)
        .bind(item.active as i64)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    for (n, u) in ledger.unreadable.iter().enumerate() {
        sqlx::query(
            "INSERT INTO ledger_import_unreadable
                (batch_id, line_no, kind, source_txn_id, date, doc_num, name, source_account_id, cents, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(batch_id)
        .bind(n as i64)
        .bind(u.kind)
        .bind(u.transaction_id)
        .bind(u.date.map(|d| d.to_string()))
        .bind(&u.doc_num)
        .bind(&u.name)
        .bind(u.account_id)
        .bind(u.cents)
        .bind(u.outcome.code())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
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

/// A line row that did not decode in full (see `qbw_company::UnreadableLine`).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableView {
    pub kind: String,
    pub date: Option<String>,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub account_name: Option<String>,
    pub cents: Option<i64>,
    /// `RECOVERED`, `NO_AMOUNT` or `UNREADABLE`.
    pub outcome: String,
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

/// How far a batch's customers, vendors and items have been brought into LocalBooks.
#[derive(Debug, Clone, Default, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ListsView {
    pub customers: i64,
    pub customers_done: i64,
    pub vendors: i64,
    pub vendors_done: i64,
    pub items: i64,
    pub items_done: i64,
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
    pub unreadable: Vec<UnreadableView>,
    /// Customers, vendors and items read from the file, and how many are in LocalBooks.
    pub lists: ListsView,
    /// Present while the pre-import copy exists on disk.
    pub start_over: Option<StartOverPoint>,
    /// How the batch was last posted: `JOURNAL` or `FULL`.
    pub mode: String,
    /// Sales documents a full import would create, by type (`invoice`, `payment`, ...).
    pub documents: Vec<(String, i64)>,
    /// What the posted transactions became (`INVOICE`, `PAYMENT`, `JOURNAL`, ...).
    pub posted_as: Vec<(String, i64)>,
    /// Documents a full import had to post as journal entries, and why.
    pub fallbacks: Vec<FallbackView>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FallbackView {
    pub kind: String,
    pub date: String,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub reason: String,
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
        mode: String,
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
                    COALESCE(SUM(t.imbalance_cents = 0 AND (t.journal_entry_id IS NULL OR e.is_voided = 1)
                                 AND EXISTS (SELECT 1 FROM ledger_import_lines l WHERE l.batch_id = t.batch_id AND l.source_txn_id = t.source_id)), 0),
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
    let unreadable: Vec<UnreadableView> = sqlx::query_as(
        "SELECT u.kind, u.date, u.doc_num, u.name, a.full_name AS account_name, u.cents, u.outcome
         FROM ledger_import_unreadable u
         LEFT JOIN ledger_import_accounts a ON a.batch_id = u.batch_id AND a.source_id = u.source_account_id
         WHERE u.batch_id = ?1 ORDER BY u.outcome = 'UNREADABLE' DESC, u.date, u.line_no",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let lists: ListsView = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'CUSTOMER') AS customers,
            (SELECT COUNT(*) FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'CUSTOMER' AND party_id IS NOT NULL) AS customers_done,
            (SELECT COUNT(*) FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'VENDOR') AS vendors,
            (SELECT COUNT(*) FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'VENDOR' AND party_id IS NOT NULL) AS vendors_done,
            (SELECT COUNT(*) FROM ledger_import_items WHERE batch_id = ?1) AS items,
            (SELECT COUNT(*) FROM ledger_import_items WHERE batch_id = ?1 AND product_id IS NOT NULL) AS items_done",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let documents: Vec<(String, i64)> = sqlx::query_as(
        "SELECT json_extract(document, '$.type'), COUNT(*) FROM ledger_import_transactions
         WHERE batch_id = ?1 AND document IS NOT NULL AND imbalance_cents = 0 GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let posted_as: Vec<(String, i64)> = sqlx::query_as(
        "SELECT t.posted_as, COUNT(*) FROM ledger_import_transactions t LEFT JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 AND t.posted_as IS NOT NULL AND t.posted_as <> 'NOT_IMPORTED' AND (e.id IS NULL OR e.is_voided = 0)
         GROUP BY 1 ORDER BY 2 DESC",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let fallbacks: Vec<FallbackView> = sqlx::query_as(
        "SELECT t.kind, t.date, t.doc_num, t.name,
                CASE WHEN t.posted_as = 'NOT_IMPORTED' THEN 'not brought in: ' || t.fallback_reason ELSE t.fallback_reason END AS reason
         FROM ledger_import_transactions t LEFT JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 AND (e.id IS NULL OR e.is_voided = 0) AND t.fallback_reason IS NOT NULL ORDER BY t.date, t.source_id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(LedgerBatchSummary {
        mode: head.mode,
        documents,
        posted_as,
        fallbacks,
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
        unreadable,
        lists,
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
    /// Accounts an earlier import created and a reversal deactivated, put back into use.
    pub reactivated: i64,
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
                (EXISTS (SELECT 1 FROM ledger_import_lines l WHERE l.batch_id = a.batch_id AND l.source_account_id = a.source_id)
                 -- Estimates and purchase orders have no ledger lines; their form lines name accounts too.
                 OR EXISTS (SELECT 1 FROM ledger_import_transactions t, json_each(t.document, '$.lines') j
                            WHERE t.batch_id = a.batch_id AND json_extract(t.document, '$.type') IN ('estimate', 'purchaseOrder')
                              AND json_extract(j.value, '$.accountId') = a.source_id)) AS used
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
        // 1. an active account with the same name, type and parent;
        // 2. one an earlier import created for this source account (same type and parent, named
        //    after it, possibly with an "(imported…)" suffix), even if a reversal deactivated it;
        // 3. for receivables and payables, the system account.
        let existing: Option<(i64, i64)> = sqlx::query_as(
            "SELECT id, is_active FROM (
                 SELECT id, is_active, 1 AS rank FROM chart_of_accounts
                 WHERE is_active = 1 AND account_type = ?2 AND name = ?1 COLLATE NOCASE AND COALESCE(parent_id, 0) = COALESCE(?3, 0)
                 UNION ALL
                 SELECT id, is_active, 2 FROM chart_of_accounts
                 WHERE account_type = ?2 AND COALESCE(parent_id, 0) = COALESCE(?3, 0) AND is_system = 0
                   AND description LIKE ?4 || '%'
                   AND (name = ?1 COLLATE NOCASE OR name LIKE ?1 || ' (imported%)')
                 UNION ALL
                 SELECT id, is_active, 3 FROM chart_of_accounts
                 WHERE ?2 IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE') AND account_type = ?2 AND is_system = 1
             ) ORDER BY rank, is_active DESC, id LIMIT 1",
        )
        .bind(&s.name)
        .bind(&s.account_type)
        .bind(parent)
        .bind(CREATED_NOTE)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let account_id = match existing {
            Some((id, active)) => {
                if active == 0 {
                    if let Err(e) = sqlx::query("UPDATE chart_of_accounts SET is_active = 1 WHERE id = ?1").bind(id).execute(&mut *tx).await {
                        return Err(ledger::rollback(tx, db_err(e)).await);
                    }
                    result.reactivated += 1;
                } else {
                    result.matched += 1;
                }
                id
            }
            None => {
                let note = if s.is_active == 0 {
                    format!("{CREATED_NOTE} {file_name} ({}; inactive there)", s.full_name)
                } else {
                    format!("{CREATED_NOTE} {file_name} ({})", s.full_name)
                };
                // Sibling names are unique (active or not, any type): take the first free one of
                // "Name", "Name (imported)", "Name (imported 2)", …
                let mut name = s.name.clone();
                for n in 1.. {
                    let taken: Option<i64> = sqlx::query_scalar(
                        "SELECT id FROM chart_of_accounts WHERE name = ?1 COLLATE NOCASE AND COALESCE(parent_id, 0) = COALESCE(?2, 0)",
                    )
                    .bind(&name)
                    .bind(parent)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db_err)?;
                    if taken.is_none() {
                        break;
                    }
                    name = if n == 1 { format!("{} (imported)", s.name) } else { format!("{} (imported {n})", s.name) };
                }
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
        // Created now, or reactivated: this import owns it, so reversing it deactivates it again.
        let created = existing.is_none_or(|(_, active)| active == 0) as i64;
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
pub struct ImportListsResult {
    pub customers_matched: i64,
    pub customers_created: i64,
    pub vendors_matched: i64,
    pub vendors_created: i64,
    pub items_matched: i64,
    pub items_created: i64,
    /// Items whose account in the file is not an income account in LocalBooks (or is not mapped),
    /// created against the fallback income account.
    pub items_on_fallback_account: i64,
}

/// An email LocalBooks will store (it checks `%_@_%._%`); anything else goes to the notes.
fn storable_email(email: Option<&str>) -> Option<&str> {
    email.filter(|e| {
        let Some((local, domain)) = e.split_once('@') else { return false };
        !local.is_empty() && domain.split_once('.').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
    })
}

/// Brings the batch's customers, vendors and items into LocalBooks: each matches an existing
/// record of the same name (names are unique, regardless of case), or is created with the file's
/// details. Customers' jobs come after their customer. Items post to the LocalBooks account
/// their own income account is mapped to, so map accounts first; an item whose account is not
/// mapped uses the first active income account. Runs again safely: mapped rows are skipped.
pub async fn import_lists(pool: &SqlitePool, batch_id: i64) -> Result<ImportListsResult, String> {
    require_batch(pool, batch_id).await?;
    #[derive(sqlx::FromRow)]
    struct Party {
        kind: String,
        source_id: i64,
        name: String,
        parent_source_id: Option<i64>,
        company_name: Option<String>,
        email: Option<String>,
        phone: Option<String>,
        address: Option<String>,
        terms_days: Option<i64>,
        tax_id: Option<String>,
        is_1099: i64,
        is_active: i64,
        notes: Option<String>,
    }
    let parties: Vec<Party> = sqlx::query_as(
        "SELECT kind, source_id, name, parent_source_id, company_name, email, phone, address, terms_days, tax_id, is_1099,
                is_active, notes
         FROM ledger_import_parties WHERE batch_id = ?1 AND party_id IS NULL
         ORDER BY kind, length(name) - length(replace(name, ':', '')), name",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut result = ImportListsResult::default();
    let mut tx = pool.begin().await.map_err(db_err)?;
    for p in parties {
        let table = if p.kind == "CUSTOMER" { "customers" } else { "vendors" };
        let existing: Option<i64> = sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE name = ?1 COLLATE NOCASE"))
            .bind(&p.name)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        let (id, created) = match existing {
            Some(id) => (id, false),
            None => {
                let email = storable_email(p.email.as_deref());
                let notes = match (p.email.as_deref().filter(|_| email.is_none()), p.notes.as_deref()) {
                    (Some(e), Some(n)) => Some(format!("Email: {e}\n{n}")),
                    (Some(e), None) => Some(format!("Email: {e}")),
                    (None, n) => n.map(str::to_string),
                };
                let inserted = if p.kind == "CUSTOMER" {
                    let parent: Option<i64> = match p.parent_source_id {
                        Some(ps) => sqlx::query_scalar("SELECT party_id FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'CUSTOMER' AND source_id = ?2")
                            .bind(batch_id)
                            .bind(ps)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(db_err)?
                            .flatten(),
                        None => None,
                    };
                    sqlx::query_scalar(
                        "INSERT INTO customers (name, company_name, email, phone, billing_address, terms_days, notes, parent_id, is_active)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
                    )
                    .bind(&p.name)
                    .bind(&p.company_name)
                    .bind(email)
                    .bind(&p.phone)
                    .bind(&p.address)
                    .bind(p.terms_days)
                    .bind(&notes)
                    .bind(parent)
                    .bind(p.is_active)
                    .fetch_one(&mut *tx)
                    .await
                } else {
                    sqlx::query_scalar(
                        "INSERT INTO vendors (name, company_name, email, phone, address, terms_days, tax_id, is_1099, notes, is_active)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
                    )
                    .bind(&p.name)
                    .bind(&p.company_name)
                    .bind(email)
                    .bind(&p.phone)
                    .bind(&p.address)
                    .bind(p.terms_days)
                    .bind(&p.tax_id)
                    .bind(p.is_1099)
                    .bind(&notes)
                    .bind(p.is_active)
                    .fetch_one(&mut *tx)
                    .await
                };
                match inserted {
                    Ok(id) => (id, true),
                    Err(e) => return Err(ledger::rollback(tx, format!("{} ({} {})", db_err(e), p.kind.to_lowercase(), p.name)).await),
                }
            }
        };
        match (p.kind.as_str(), created) {
            ("CUSTOMER", true) => result.customers_created += 1,
            ("CUSTOMER", false) => result.customers_matched += 1,
            (_, true) => result.vendors_created += 1,
            (_, false) => result.vendors_matched += 1,
        }
        if let Err(e) = sqlx::query("UPDATE ledger_import_parties SET party_id = ?4, created_by_import = ?5 WHERE batch_id = ?1 AND kind = ?2 AND source_id = ?3")
            .bind(batch_id)
            .bind(&p.kind)
            .bind(p.source_id)
            .bind(id)
            .bind(created as i64)
            .execute(&mut *tx)
            .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
    }

    #[derive(sqlx::FromRow)]
    struct Item {
        source_id: i64,
        name: String,
        description: Option<String>,
        rate_cents: i64,
        /// The LocalBooks account the item's own account maps to, when that is an income account.
        mapped_account: Option<i64>,
        /// The item's account in the file, when it cannot be the item's LocalBooks account.
        other_account: Option<String>,
        is_taxable: i64,
        is_active: i64,
    }
    // LocalBooks items post to income accounts only; the desktop program also allows expense and
    // cost accounts (for example a "Parts" item). Those items use the fallback income account,
    // and their description says which account they used in the file.
    let items: Vec<Item> = sqlx::query_as(
        "SELECT i.source_id, i.name, i.description, i.rate_cents,
                CASE WHEN t.classification = 'INCOME' THEN a.account_id END AS mapped_account,
                CASE WHEN a.account_id IS NULL OR t.classification <> 'INCOME' THEN a.full_name END AS other_account,
                i.is_taxable, i.is_active
         FROM ledger_import_items i
         LEFT JOIN ledger_import_accounts a ON a.batch_id = i.batch_id AND a.source_id = i.income_source_account_id
         LEFT JOIN chart_of_accounts c ON c.id = a.account_id
         LEFT JOIN account_types t ON t.code = c.account_type
         WHERE i.batch_id = ?1 AND i.product_id IS NULL ORDER BY i.name",
    )
    .bind(batch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    let fallback: Option<i64> = sqlx::query_scalar(
        "SELECT a.id FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
         WHERE a.is_active = 1 AND t.classification = 'INCOME' ORDER BY a.account_type <> 'INCOME', a.account_number, a.id LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    for item in items {
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM products_services WHERE name = ?1 COLLATE NOCASE")
            .bind(&item.name)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
        let (id, created) = match existing {
            Some(id) => {
                result.items_matched += 1;
                (id, false)
            }
            None => {
                let account = match item.mapped_account.or(fallback) {
                    Some(a) => a,
                    None => return Err(ledger::rollback(tx, "QBW_NO_INCOME_ACCOUNT: add an income account before bringing in items".into()).await),
                };
                let description = match (&item.other_account, &item.description) {
                    (Some(other), Some(d)) => Some(format!("{d} (posted to {other} in the original file)")),
                    (Some(other), None) => Some(format!("Posted to {other} in the original file")),
                    (None, d) => d.clone(),
                };
                if item.mapped_account.is_none() {
                    result.items_on_fallback_account += 1;
                }
                let inserted = sqlx::query_scalar(
                    "INSERT INTO products_services (name, description, rate_cents, income_account_id, is_taxable, is_active)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
                )
                .bind(&item.name)
                .bind(&description)
                .bind(item.rate_cents.clamp(0, crate::ledger::MAX_LINE_CENTS))
                .bind(account)
                .bind(item.is_taxable)
                .bind(item.is_active)
                .fetch_one(&mut *tx)
                .await;
                match inserted {
                    Ok(id) => {
                        result.items_created += 1;
                        (id, true)
                    }
                    Err(e) => return Err(ledger::rollback(tx, format!("{} (item {})", db_err(e), item.name)).await),
                }
            }
        };
        if let Err(e) = sqlx::query("UPDATE ledger_import_items SET product_id = ?3, created_by_import = ?4 WHERE batch_id = ?1 AND source_id = ?2")
            .bind(batch_id)
            .bind(item.source_id)
            .bind(id)
            .bind(created as i64)
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
pub struct PostLedgerResult {
    pub entries_posted: i64,
    pub lines_posted: i64,
    pub skipped_unbalanced: i64,
    pub skipped_already_posted: i64,
    /// What a full import made of the transactions.
    pub full: Option<FullImportCounts>,
}

/// How a batch posts: every transaction as a journal entry, or sales documents as LocalBooks
/// invoices, credit memos, sales receipts, payments and deposits (see `qbw_full_import`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImportMode {
    #[default]
    Journal,
    Full,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostLedgerInput {
    pub batch_id: i64,
    #[serde(default)]
    pub mode: ImportMode,
}

pub(crate) fn describe(kind: &str, doc_num: Option<&str>, name: Option<&str>, memo: Option<&str>) -> String {
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
        document: Option<String>,
    }
    let txns: Vec<Txn> = sqlx::query_as(
        "SELECT t.source_id, t.kind, t.date, t.doc_num, t.name, t.memo, t.imbalance_cents,
                (t.journal_entry_id IS NOT NULL AND e.is_voided = 0) AS live, t.document
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
        // Unmapped accounts only occur on unbalanced transactions, which are never posted.
        lines.entry(txn).or_default().push((map.get(&account).copied().unwrap_or(0), cents, memo));
    }

    if input.mode == ImportMode::Full {
        import_lists(pool, batch_id).await?;
    }
    let mut result = PostLedgerResult::default();
    let mut staged = Vec::with_capacity(txns.len());
    for t in txns {
        if t.imbalance_cents != 0 {
            result.skipped_unbalanced += 1;
            continue;
        }
        if t.live != 0 {
            result.skipped_already_posted += 1;
            continue;
        }
        let source_lines = lines.remove(&t.source_id).unwrap_or_default();
        let date: NaiveDate = t.date.parse().map_err(|_| format!("QBW_BAD_DATE: staged date '{}' is not a date", t.date))?;
        let document = match (input.mode, &t.document) {
            (ImportMode::Full, Some(json)) => serde_json::from_str(json).ok(),
            _ => None,
        };
        // Only a full import brings in what posts nothing (estimates, purchase orders).
        if source_lines.is_empty() && document.is_none() {
            continue;
        }
        staged.push(Staged { source_id: t.source_id, kind: t.kind, date, doc_num: t.doc_num, name: t.name, memo: t.memo, lines: source_lines, document });
    }
    if input.mode == ImportMode::Full && result.skipped_already_posted > 0 {
        return Err("QBW_FULL_AFTER_POST: this import is already posted as journal entries; use Start over before a full import".into());
    }

    let mut tx = pool.begin().await.map_err(db_err)?;
    let outcomes = match input.mode {
        ImportMode::Journal => {
            let mut outcomes = Vec::with_capacity(staged.len());
            for t in &staged {
                let entry = JournalEntry {
                    id: None,
                    date: t.date,
                    description: describe(&t.kind, t.doc_num.as_deref(), t.name.as_deref(), t.memo.as_deref()),
                    lines: t
                        .lines
                        .iter()
                        .map(|(account, cents, memo)| {
                            let mut line = if *cents < 0 { JournalLine::debit(*account, -cents) } else { JournalLine::credit(*account, *cents) };
                            line.memo = memo.clone();
                            line
                        })
                        .collect(),
                    currency: None,
                };
                match ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
                    Ok(id) => outcomes.push(Outcome { source_id: t.source_id, entry_id: Some(id), posted_as: "JOURNAL", fallback_reason: None, entries: 1 }),
                    Err(err) => return Err(ledger::rollback(tx, format!("{err} (imported {})", entry.description)).await),
                }
            }
            outcomes
        }
        ImportMode::Full => match crate::qbw_full_import::post_all(&mut tx, batch_id, &map, &staged).await {
            Ok((outcomes, counts)) => {
                result.full = Some(counts);
                outcomes
            }
            Err(err) => return Err(ledger::rollback(tx, err).await),
        },
    };
    let line_counts: HashMap<i64, usize> = staged.iter().map(|t| (t.source_id, t.lines.len())).collect();
    for o in &outcomes {
        if let Err(e) = sqlx::query(
            "UPDATE ledger_import_transactions SET journal_entry_id = ?3, posted_as = ?4, fallback_reason = ?5 WHERE batch_id = ?1 AND source_id = ?2",
        )
        .bind(batch_id)
        .bind(o.source_id)
        .bind(o.entry_id)
        .bind(o.posted_as)
        .bind(&o.fallback_reason)
        .execute(&mut *tx)
        .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
        result.entries_posted += o.entries;
        result.lines_posted += line_counts[&o.source_id] as i64;
    }
    if let Err(e) = sqlx::query("UPDATE ledger_import_batches SET mode = ?2 WHERE id = ?1")
        .bind(batch_id)
        .bind(if input.mode == ImportMode::Full { "FULL" } else { "JOURNAL" })
        .execute(&mut *tx)
        .await
    {
        return Err(ledger::rollback(tx, db_err(e)).await);
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
    /// Customers, vendors and items the import created, now inactive.
    pub list_records_deactivated: i64,
}

/// Undoes a posted import: voids every entry it posted and deactivates the accounts it created
/// once they have no other activity.
pub async fn reverse_batch(pool: &SqlitePool, batch_id: i64) -> Result<ReverseLedgerResult, String> {
    let mode: Option<String> = sqlx::query_scalar("SELECT mode FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if mode.as_deref() == Some("FULL") {
        // Its invoices, payments and deposits are linked to one another; voiding entries one by one
        // would leave documents behind, so the undo is putting back the books from before the post.
        return Err("QBW_FULL_USE_START_OVER: a full import is undone with Start over, which puts back the books from just before it posted".into());
    }
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
    // Customers, vendors and items the import created go inactive too (their history is kept).
    for sql in [
        "UPDATE customers SET is_active = 0 WHERE is_active = 1 AND id IN
             (SELECT party_id FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'CUSTOMER' AND created_by_import = 1)",
        "UPDATE vendors SET is_active = 0 WHERE is_active = 1 AND id IN
             (SELECT party_id FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'VENDOR' AND created_by_import = 1)",
        "UPDATE products_services SET is_active = 0 WHERE is_active = 1 AND id IN
             (SELECT product_id FROM ledger_import_items WHERE batch_id = ?1 AND created_by_import = 1)",
    ] {
        let done = sqlx::query(sql).bind(batch_id).execute(pool).await.map_err(db_err)?;
        result.list_records_deactivated += done.rows_affected() as i64;
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
        "DELETE FROM ledger_import_unreadable WHERE batch_id = ?1",
        "DELETE FROM ledger_import_parties WHERE batch_id = ?1",
        "DELETE FROM ledger_import_items WHERE batch_id = ?1",
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
    use crate::qbw_company::{LedgerQuality, LineOutcome, SourceAccount, SourceItem, SourceLine, SourceParty, SourceTransaction, UnreadableLine};

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
            document: None,
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
            unreadable: vec![UnreadableLine {
                kind: "Check",
                transaction_id: Some(13),
                date: "2023-03-04".parse().ok(),
                doc_num: Some("13".into()),
                name: None,
                account_id: Some(3),
                cents: Some(-250),
                outcome: LineOutcome::Recovered,
            }],
            customers: vec![],
            vendors: vec![],
            items: vec![],
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

    /// A full import: an invoice with sales tax, a prepayment dated before it, a second payment,
    /// a deposit with a fee taken out, and an invoice whose customer is missing (falls back).
    #[tokio::test]
    async fn a_full_import_recreates_invoices_payments_and_deposits() {
        use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
        let pool = test_pool().await;
        let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
            kind,
            document: Some(document),
            ..txn(id, date, lines)
        };
        let invoice = |customer_id| SourceDocument::Invoice {
            customer_id,
            number: Some("501".into()),
            due_date: "2024-02-09".parse().ok(),
            lines: vec![
                DocumentLine { item_id: Some(200), description: None, quantity_e4: 20_000, rate_cents: 5_000, cents: 10_000, account_id: 6 },
                DocumentLine { item_id: None, description: Some("Sales tax".into()), quantity_e4: 10_000, rate_cents: 1_000, cents: 1_000, account_id: 8 },
            ],
        };
        let payment = |cents| SourceDocument::Payment {
            customer_id: 100,
            reference: Some("1234".into()),
            method: "CHECK".into(),
            applied_to: vec![Applied { transaction_id: 20, cents }],
        };
        let ledger = Ledger {
            accounts: vec![
                account(1, "Riverside Bank", "BANK", None),
                account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
                account(6, "Consulting Income", "INCOME", None),
                account(7, "Undeposited Funds", "OTHER_CURRENT_ASSET", None),
                account(8, "Sales Tax Payable", "OTHER_CURRENT_LIABILITY", None),
                account(9, "Merchant Fees", "EXPENSE", None),
            ],
            transactions: vec![
                doc(21, "Payment", "2024-01-05", &[(7, -6_000), (4, 6_000)], payment(6_000)),
                doc(20, "Invoice", "2024-01-10", &[(4, -11_000), (6, 10_000), (8, 1_000)], invoice(100)),
                doc(22, "Payment", "2024-01-20", &[(7, -5_000), (4, 5_000)], payment(5_000)),
                doc(
                    23,
                    "Deposit",
                    "2024-01-21",
                    &[(1, -10_900), (9, -100), (7, 6_000), (7, 5_000)],
                    SourceDocument::Deposit { payments: vec![Applied { transaction_id: 21, cents: 6_000 }, Applied { transaction_id: 22, cents: 5_000 }] },
                ),
                doc(24, "Invoice", "2024-01-25", &[(4, -2_000), (6, 2_000)], invoice(999)),
            ],
            unbalanced: vec![],
            unreadable: vec![],
            customers: vec![SourceParty {
                id: 100,
                name: "Acme Hosting".into(),
                parent_id: None,
                company_name: None,
                email: None,
                phone: None,
                address: None,
                terms_days: Some(30),
                tax_id: None,
                is_1099: false,
                active: true,
                balance_cents: Some(2_000),
                notes: None,
            }],
            vendors: vec![],
            items: vec![SourceItem {
                id: 200,
                name: "Consulting".into(),
                description: None,
                rate_cents: 5_000,
                income_account_id: Some(6),
                kind: "Service",
                taxable: true,
                active: true,
            }],
            quality: LedgerQuality::default(),
        };
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();
        let summary = batch_summary(&pool, batch).await.unwrap();
        assert_eq!(summary.documents.iter().map(|d| d.1).sum::<i64>(), 5);

        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
        let full = posted.full.unwrap();
        assert_eq!((full.invoices, full.payments, full.deposits, full.fell_back), (1, 2, 1, 1), "{full:?}");

        let (number, status, total, tax, items): (i64, String, i64, i64, i64) = sqlx::query_as(
            "SELECT invoice_number, status, total_cents, tax_cents, (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id) FROM invoices i",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((number, status.as_str(), total, tax, items), (501, "PAID", 11_000, 1_000, 1), "the prepayment applies too");
        let (deposit_total, linked): (i64, i64) =
            sqlx::query_as("SELECT d.total_cents, (SELECT COUNT(invoice_payment_id) FROM deposit_lines WHERE deposit_id = d.id) FROM deposits d")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((deposit_total, linked), (11_000, 2), "the deposit is gross, with both payments swept out of Undeposited Funds");
        assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty());

        // The books match the file exactly: the bank got the net, the fee is an expense.
        let summary = batch_summary(&pool, batch).await.unwrap();
        let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
        assert_eq!(balance(&pool, mapped("Riverside Bank")).await, 10_900);
        assert_eq!(balance(&pool, mapped("Merchant Fees")).await, 100);
        assert_eq!(balance(&pool, mapped("Accounts Receivable")).await, 2_000);

        assert_eq!(summary.mode, "FULL");
        assert_eq!(summary.fallbacks.len(), 1);
        assert_eq!(summary.fallbacks[0].reason, "its customer was not imported");
        let err = reverse_batch(&pool, batch).await.unwrap_err();
        assert!(err.starts_with("QBW_FULL_USE_START_OVER"), "{err}");
    }

    /// The purchase side of a full import: a bill paid by a bill payment dated before it, a check,
    /// a card charge, and a card credit (no LocalBooks form, so a journal entry).
    #[tokio::test]
    async fn a_full_import_recreates_bills_payments_checks_and_card_charges() {
        use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
        let pool = test_pool().await;
        let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
            kind,
            document: Some(document),
            ..txn(id, date, lines)
        };
        let expense = |number: Option<&str>| SourceDocument::Expense { vendor_id: Some(300), number: number.map(str::to_string), applied_to: vec![] };
        let ledger = Ledger {
            accounts: vec![
                account(1, "Riverside Bank", "BANK", None),
                account(5, "Accounts Payable", "ACCOUNTS_PAYABLE", None),
                account(10, "Office Expense", "EXPENSE", None),
                account(11, "Visa", "CREDIT_CARD", None),
                account(12, "Supplies", "EXPENSE", None),
            ],
            transactions: vec![
                doc(
                    31,
                    "Bill payment",
                    "2024-02-05",
                    &[(1, 5_000), (5, -5_000)],
                    SourceDocument::BillPayment { vendor_id: Some(300), reference: Some("990".into()), applied_to: vec![Applied { transaction_id: 30, cents: 5_000 }] },
                ),
                doc(
                    30,
                    "Bill",
                    "2024-02-10",
                    &[(5, 5_000), (10, -5_000)],
                    SourceDocument::Bill {
                        vendor_id: 300,
                        number: Some("B-7".into()),
                        due_date: "2024-03-10".parse().ok(),
                        lines: vec![DocumentLine { item_id: None, description: Some("Paper".into()), quantity_e4: 10_000, rate_cents: 5_000, cents: -5_000, account_id: 10 }],
                    },
                ),
                doc(32, "Check", "2024-02-12", &[(1, 2_500), (12, -2_500)], expense(Some("1001"))),
                doc(33, "Credit card charge", "2024-02-13", &[(11, 1_200), (10, -1_200)], expense(None)),
                doc(34, "Credit card charge", "2024-02-14", &[(11, -300), (10, 300)], expense(None)),
            ],
            unbalanced: vec![],
            unreadable: vec![],
            customers: vec![],
            vendors: vec![SourceParty {
                id: 300,
                name: "Paper Supply Co".into(),
                parent_id: None,
                company_name: None,
                email: None,
                phone: None,
                address: None,
                terms_days: None,
                tax_id: None,
                is_1099: false,
                active: true,
                balance_cents: Some(0),
                notes: None,
            }],
            items: vec![],
            quality: LedgerQuality::default(),
        };
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();
        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
        let full = posted.full.unwrap();
        assert_eq!((full.bills, full.bill_payments, full.checks, full.expenses, full.fell_back), (1, 1, 1, 1, 1), "{full:?}");

        let (status, bill_number, lines): (String, Option<String>, i64) =
            sqlx::query_as("SELECT status, bill_number, (SELECT COUNT(*) FROM bill_lines WHERE bill_id = b.id) FROM bills b").fetch_one(&pool).await.unwrap();
        assert_eq!((status.as_str(), bill_number.as_deref(), lines), ("PAID", Some("B-7"), 1), "the payment dated before the bill still applies");
        let checks: Vec<(String, Option<String>, i64)> =
            sqlx::query_as("SELECT kind, check_number, total_cents FROM expenses ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(checks, vec![("CHECK".into(), Some("1001".into()), 2_500), ("EXPENSE".into(), None, 1_200)]);

        let summary = batch_summary(&pool, batch).await.unwrap();
        assert_eq!(summary.fallbacks.len(), 1);
        assert_eq!(summary.fallbacks[0].reason, "it is a refund or card credit, which has no LocalBooks form");
        let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
        assert_eq!(balance(&pool, mapped("Riverside Bank")).await, -7_500);
        assert_eq!(balance(&pool, mapped("Visa")).await, -900);
        assert_eq!(balance(&pool, mapped("Office Expense")).await, 5_900);
        assert_eq!(balance(&pool, mapped("Accounts Payable")).await, 0);
    }

    /// Transfers, estimates and purchase orders in a full import. Estimates and purchase orders
    /// post nothing: one that cannot be brought in is listed, and none counts as postable.
    #[tokio::test]
    async fn a_full_import_brings_transfers_estimates_and_purchase_orders() {
        use crate::qbw_documents::{DocumentLine, SourceDocument};
        let pool = test_pool().await;
        let line = |item_id, cents, account_id| DocumentLine { item_id, description: None, quantity_e4: 30_000, rate_cents: 1_000, cents, account_id };
        let estimate = |customer_id| SourceDocument::Estimate {
            customer_id,
            date: "2024-04-01".parse().unwrap(),
            number: Some("77".into()),
            expires: "2024-05-01".parse().ok(),
            memo: Some("Spring work".into()),
            lines: vec![line(Some(200), 3_000, 6), DocumentLine { item_id: None, description: Some("Tax".into()), quantity_e4: 10_000, rate_cents: 240, cents: 240, account_id: 8 }],
        };
        let lineless = |id, kind, document: SourceDocument| SourceTransaction { kind, document: Some(document), ..txn(id, "2024-04-01", &[]) };
        let party = |id, name: &str| SourceParty {
            id,
            name: name.into(),
            parent_id: None,
            company_name: None,
            email: None,
            phone: None,
            address: None,
            terms_days: None,
            tax_id: None,
            is_1099: false,
            active: true,
            balance_cents: None,
            notes: None,
        };
        let ledger = Ledger {
            accounts: vec![
                account(1, "Riverside Bank", "BANK", None),
                account(2, "Savings", "BANK", None),
                account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
                account(6, "Consulting Income", "INCOME", None),
                account(8, "Sales Tax Payable", "OTHER_CURRENT_LIABILITY", None),
                account(10, "Office Expense", "EXPENSE", None),
            ],
            transactions: vec![
                SourceTransaction { kind: "Transfer", document: Some(SourceDocument::Transfer), ..txn(40, "2024-04-02", &[(2, -20_000), (1, 20_000)]) },
                SourceTransaction { kind: "Transfer", document: Some(SourceDocument::Transfer), ..txn(41, "2024-04-03", &[(4, -500), (1, 500)]) },
                lineless(42, "Estimate", estimate(100)),
                lineless(43, "Estimate", estimate(999)),
                lineless(
                    44,
                    "Purchase order",
                    SourceDocument::PurchaseOrder {
                        vendor_id: 300,
                        date: "2024-04-01".parse().unwrap(),
                        number: Some("PO-5".into()),
                        expected: None,
                        memo: None,
                        received: true,
                        lines: vec![line(None, 3_000, 10)],
                    },
                ),
            ],
            unbalanced: vec![],
            unreadable: vec![],
            customers: vec![party(100, "Acme Hosting")],
            vendors: vec![party(300, "Paper Supply Co")],
            items: vec![SourceItem {
                id: 200,
                name: "Consulting".into(),
                description: None,
                rate_cents: 1_000,
                income_account_id: Some(6),
                kind: "Service",
                taxable: true,
                active: true,
            }],
            quality: LedgerQuality::default(),
        };
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();
        assert_eq!(batch_summary(&pool, batch).await.unwrap().postable_count, 2, "only the transfers post");

        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
        let full = posted.full.unwrap();
        assert_eq!((full.transfers, full.estimates, full.purchase_orders, full.not_imported, full.fell_back), (1, 1, 1, 1, 1), "{full:?}");
        assert_eq!(posted.entries_posted, 2);

        let (number, total, tax, status, lines): (i64, i64, i64, String, i64) = sqlx::query_as(
            "SELECT estimate_number, total_cents, tax_cents, status, (SELECT COUNT(*) FROM estimate_lines WHERE estimate_id = e.id) FROM estimates e",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((number, total, tax, status.as_str(), lines), (77, 3_240, 240, "PENDING", 1));
        let (po_number, po_status, po_total): (i64, String, i64) = sqlx::query_as("SELECT po_number, status, total_cents FROM purchase_orders").fetch_one(&pool).await.unwrap();
        assert_eq!((po_number, po_status.as_str(), po_total), (1, "CLOSED", 3_000), "PO-5 is not a number, so it gets the next one");
        let (from, to, amount): (i64, i64, i64) = sqlx::query_as("SELECT from_account_id, to_account_id, amount_cents FROM transfers").fetch_one(&pool).await.unwrap();

        let summary = batch_summary(&pool, batch).await.unwrap();
        let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
        assert_eq!((from, to, amount), (mapped("Riverside Bank"), mapped("Savings"), 20_000));
        assert_eq!(summary.postable_count, 0);
        let reasons: Vec<&str> = summary.fallbacks.iter().map(|f| f.reason.as_str()).collect();
        assert_eq!(reasons, ["not brought in: its customer was not imported", "LocalBooks transfers cannot use one of its accounts (accounts receivable)"]);
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
        let lists = import_lists(&pool, batch).await.unwrap();
        println!("lists: {lists:?}");
        let (customers, active_customers, vendors, items): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM customers), (SELECT COUNT(*) FROM customers WHERE is_active = 1),
                    (SELECT COUNT(*) FROM vendors), (SELECT COUNT(*) FROM products_services)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        println!("LocalBooks now has {customers} customers ({active_customers} active), {vendors} vendors, {items} items");
        let mode = if std::env::var("LOCALBOOKS_QBW_MODE").as_deref() == Ok("full") { ImportMode::Full } else { ImportMode::Journal };
        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode }).await.unwrap();
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
        if mode == ImportMode::Full {
            let fallbacks: Vec<(String, String, i64)> = sqlx::query_as(
                "SELECT kind, fallback_reason, COUNT(*) FROM ledger_import_transactions
                 WHERE batch_id = ?1 AND fallback_reason IS NOT NULL GROUP BY kind, fallback_reason ORDER BY 3 DESC LIMIT 25",
            )
            .bind(batch)
            .fetch_all(&pool)
            .await
            .unwrap();
            for (kind, reason, n) in fallbacks {
                println!("  fell back: {n:>5} {kind}: {reason}");
            }
            let (open, open_cents): (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM invoices WHERE status IN ('UNPAID', 'PARTIAL')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let (linked, deposit_lines): (i64, i64) =
                sqlx::query_as("SELECT COUNT(invoice_payment_id) + COUNT(sales_receipt_id), COUNT(*) FROM deposit_lines").fetch_one(&pool).await.unwrap();
            println!("open invoices: {open}, {}; deposit lines linked to a payment or receipt: {linked}/{deposit_lines}", crate::ledger::format_cents(open_cents));
            let (bills, bills_cents): (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM bills WHERE status IN ('UNPAID', 'PARTIAL')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            println!("open bills: {bills}, {}", crate::ledger::format_cents(bills_cents));
            let waiting = crate::banking::list_undeposited(&pool).await.unwrap();
            let uf: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN chart_of_accounts a ON a.id = l.account_id
                 WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            println!(
                "waiting to be deposited: {} items, {}; Undeposited Funds balance {}",
                waiting.len(),
                crate::ledger::format_cents(waiting.iter().map(|w| w.amount_cents).sum()),
                crate::ledger::format_cents(uf)
            );
        }
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
        post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
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
        let reposted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
        assert_eq!(reposted.entries_posted, 2);

        // A recorded copy that has gone missing is reported, not restored.
        set_pre_post_backup(&pool, batch, &dir.join("gone.db")).await.unwrap();
        assert!(pre_post_backup(&pool, batch).await.unwrap_err().contains("no longer in the backups folder"));
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reimporting_after_a_reverse_reuses_the_accounts_it_created() {
        let pool = test_pool().await;
        // A LocalBooks account of another type already holds the name, so the first import
        // creates "Equipment (imported)".
        sqlx::query("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Equipment', 'FIXED_ASSET')").execute(&pool).await.ok();
        let books = || Ledger {
            accounts: vec![account(1, "Riverside Bank", "BANK", None), account(2, "Equipment", "EXPENSE", None)],
            transactions: vec![txn(10, "2023-03-01", &[(2, -12_000), (1, 12_000)])],
            unbalanced: vec![],
            unreadable: vec![],
            customers: vec![],
            vendors: vec![],
            items: vec![],
            quality: LedgerQuality::default(),
        };
        let first = stage(&pool, Path::new("/books/Books.qbw"), 1, &books()).await.unwrap();
        auto_map_accounts(&pool, first).await.unwrap();
        let mapped = |batch: i64| {
            let pool = pool.clone();
            async move {
                batch_summary(&pool, batch).await.unwrap().accounts.into_iter().find(|a| a.full_name == "Equipment").unwrap().account_id.unwrap()
            }
        };
        let equipment = mapped(first).await;
        let name: String = sqlx::query_scalar("SELECT name FROM chart_of_accounts WHERE id = ?1").bind(equipment).fetch_one(&pool).await.unwrap();
        assert_eq!(name, "Equipment (imported)");
        post_batch(&pool, &PostLedgerInput { batch_id: first, mode: ImportMode::Journal }).await.unwrap();
        reverse_batch(&pool, first).await.unwrap();
        discard_batch(&pool, first).await.unwrap();

        // Importing the same books again must not trip the sibling-name rule: the deactivated
        // account is reused and reactivated.
        let second = stage(&pool, Path::new("/books/Books.qbw"), 1, &books()).await.unwrap();
        auto_map_accounts(&pool, second).await.unwrap();
        assert_eq!(mapped(second).await, equipment);
        let active: i64 = sqlx::query_scalar("SELECT is_active FROM chart_of_accounts WHERE id = ?1").bind(equipment).fetch_one(&pool).await.unwrap();
        assert_eq!(active, 1);
        let posted = post_batch(&pool, &PostLedgerInput { batch_id: second, mode: ImportMode::Journal }).await.unwrap();
        assert_eq!(posted.entries_posted, 1);

        // A taken "(imported)" name that is not the importer's own gets the next free suffix.
        sqlx::query("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Tools', 'FIXED_ASSET'), ('Tools (imported)', 'OTHER_ASSET')")
            .execute(&pool)
            .await
            .unwrap();
        let tools = Ledger {
            accounts: vec![account(1, "Riverside Bank", "BANK", None), account(3, "Tools", "EXPENSE", None)],
            transactions: vec![txn(11, "2023-03-02", &[(3, -500), (1, 500)])],
            unbalanced: vec![],
            unreadable: vec![],
            customers: vec![],
            vendors: vec![],
            items: vec![],
            quality: LedgerQuality::default(),
        };
        let third = stage(&pool, Path::new("/books/Other.qbw"), 1, &tools).await.unwrap();
        auto_map_accounts(&pool, third).await.unwrap();
        let id = batch_summary(&pool, third).await.unwrap().accounts.into_iter().find(|a| a.full_name == "Tools").unwrap().account_id.unwrap();
        let name: String = sqlx::query_scalar("SELECT name FROM chart_of_accounts WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(name, "Tools (imported 2)");
    }

    fn party(id: i64, name: &str, parent: Option<i64>, email: Option<&str>) -> SourceParty {
        SourceParty {
            id,
            name: name.into(),
            parent_id: parent,
            company_name: None,
            email: email.map(Into::into),
            phone: Some("555-0100".into()),
            address: Some("1 Main St\nSpringfield".into()),
            terms_days: Some(30),
            tax_id: None,
            is_1099: false,
            active: true,
            balance_cents: None,
            notes: None,
        }
    }

    #[tokio::test]
    async fn lists_are_matched_by_name_or_created_with_their_details() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO customers (name) VALUES ('Acme')").execute(&pool).await.unwrap();
        let mut books = ledger();
        books.customers = vec![
            party(1, "Acme", None, None),
            party(2, "Beta Co", None, Some("ap@beta.com")),
            party(3, "Beta Co:Roof job", Some(2), Some("not an email")),
        ];
        books.vendors = vec![party(9, "Hosting Inc", None, Some("billing@hosting.example"))];
        let item = |id, name: &str, account| SourceItem {
            id,
            name: name.into(),
            description: Some("desc".into()),
            rate_cents: 8_500,
            income_account_id: account,
            kind: "Service",
            taxable: false,
            active: true,
        };
        books.accounts.push(account(6, "Consulting Income", "INCOME", None));
        books.transactions.push(txn(14, "2023-03-05", &[(4, -9_000), (6, 9_000)]));
        books.items = vec![item(20, "Business Labor", Some(6)), item(21, "Mystery", Some(999)), item(22, "Parts kit", Some(2))];
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &books).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();

        let r = import_lists(&pool, batch).await.unwrap();
        assert_eq!((r.customers_matched, r.customers_created, r.vendors_created, r.items_created), (1, 2, 1, 3));
        assert_eq!(r.items_on_fallback_account, 2, "the unmapped item and the one posting to an expense account");
        let kit: String = sqlx::query_scalar("SELECT description FROM products_services WHERE name = 'Parts kit'").fetch_one(&pool).await.unwrap();
        assert_eq!(kit, "desc (posted to Hosting in the original file)");
        let (parent, notes, email): (Option<i64>, Option<String>, Option<String>) =
            sqlx::query_as("SELECT parent_id, notes, email FROM customers WHERE name = 'Beta Co:Roof job'").fetch_one(&pool).await.unwrap();
        let beta: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Beta Co'").fetch_one(&pool).await.unwrap();
        assert_eq!(parent, Some(beta));
        assert_eq!((email, notes.as_deref()), (None, Some("Email: not an email")));
        let (terms, address): (Option<i64>, Option<String>) =
            sqlx::query_as("SELECT terms_days, billing_address FROM customers WHERE id = ?1").bind(beta).fetch_one(&pool).await.unwrap();
        assert_eq!((terms, address.as_deref()), (Some(30), Some("1 Main St\nSpringfield")));
        let summary = batch_summary(&pool, batch).await.unwrap();
        let consulting = summary.accounts.iter().find(|a| a.full_name == "Consulting Income").unwrap().account_id.unwrap();
        let labor_account: i64 = sqlx::query_scalar("SELECT income_account_id FROM products_services WHERE name = 'Business Labor'").fetch_one(&pool).await.unwrap();
        assert_eq!(labor_account, consulting, "posts to the account its source account maps to");
        assert_eq!((summary.lists.customers_done, summary.lists.vendors_done, summary.lists.items_done), (3, 1, 3));

        // Again: nothing new.
        let again = import_lists(&pool, batch).await.unwrap();
        assert_eq!(again, ImportListsResult::default());

        post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
        let reversed = reverse_batch(&pool, batch).await.unwrap();
        assert_eq!(reversed.list_records_deactivated, 6, "2 customers, 1 vendor, 3 items created by the import");
        let acme: i64 = sqlx::query_scalar("SELECT is_active FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
        assert_eq!(acme, 1, "a matched customer is not the import's to deactivate");
    }

    #[tokio::test]
    async fn a_ledger_maps_posts_once_and_reverses() {
        let pool = test_pool().await;
        let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger()).await.unwrap();
        let summary = batch_summary(&pool, batch).await.unwrap();
        assert_eq!((summary.transaction_count, summary.postable_count, summary.unbalanced.len()), (3, 2, 1));
        let u = &summary.unreadable[0];
        assert_eq!((u.outcome.as_str(), u.account_name.as_deref(), u.cents), ("RECOVERED", Some("Domain Names"), Some(-250)));

        let err = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap_err();
        assert!(err.starts_with("QBW_UNMAPPED"), "{err}");

        let mapped = auto_map_accounts(&pool, batch).await.unwrap();
        // Receivables fold into the system account; the bank, the expense and its subaccount are
        // created; the unused account is left alone.
        assert_eq!(mapped, AutoMapResult { matched: 1, created: 3, reactivated: 0 });
        let accounts = batch_summary(&pool, batch).await.unwrap().accounts;
        let id = |name: &str| accounts.iter().find(|a| a.full_name == name).unwrap().account_id;
        assert!(id("Unused").is_none());
        let domains = id("Domain Names").unwrap();
        let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM chart_of_accounts WHERE id = ?1").bind(domains).fetch_one(&pool).await.unwrap();
        assert_eq!(parent, id("Hosting"));

        let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
        assert_eq!(posted, PostLedgerResult { entries_posted: 2, lines_posted: 4, skipped_unbalanced: 1, skipped_already_posted: 0, full: None });
        assert_eq!(balance(&pool, domains).await, 12_000);
        assert_eq!(balance(&pool, id("Riverside Bank").unwrap()).await, -12_500);
        let memo: String = sqlx::query_scalar("SELECT memo FROM journal_entries WHERE id = (SELECT journal_entry_id FROM ledger_import_transactions WHERE source_id = 10)")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(memo, "Check #10 · Vendor");

        // Posting again posts nothing new; a second import of the same books is refused.
        let again = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
        assert_eq!(again.entries_posted, 0);
        let second = stage(&pool, Path::new("/books/Books copy.qbw"), 1, &ledger()).await.unwrap();
        auto_map_accounts(&pool, second).await.unwrap();
        let err = post_batch(&pool, &PostLedgerInput { batch_id: second, mode: ImportMode::Journal }).await.unwrap_err();
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
