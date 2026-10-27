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

use crate::custom_fields::FieldList;
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
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
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
                (batch_id, source_id, name, full_name, account_type, parent_source_id, is_active, stored_balance_cents, tax_line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(batch_id)
        .bind(a.id)
        .bind(&a.name)
        .bind(&a.full_name)
        .bind(a.account_type)
        .bind(a.parent_id)
        .bind(a.active as i64)
        .bind(a.stored_balance_cents)
        .bind(&a.tax_line)
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
                     tax_id, is_1099, is_active, balance_cents, notes, sales_rep, custom_fields)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
            .bind(&p.sales_rep)
            .bind(custom_json(&p.custom_fields))
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    }
    for item in &ledger.items {
        sqlx::query(
            "INSERT INTO ledger_import_items
                (batch_id, source_id, name, description, rate_cents, income_source_account_id, item_kind, is_taxable, is_active, custom_fields)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
        .bind(custom_json(&item.custom_fields))
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    for (kind, names) in [("REP", &ledger.sales_lists.reps), ("SHIP_METHOD", &ledger.sales_lists.ship_methods)] {
        for (name, active) in names {
            sqlx::query("INSERT OR IGNORE INTO ledger_import_sales_lists (batch_id, kind, name, is_active) VALUES (?1, ?2, ?3, ?4)")
                .bind(batch_id)
                .bind(kind)
                .bind(name)
                .bind(*active as i64)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
    }
    for level in &ledger.sales_lists.price_levels {
        sqlx::query(
            "INSERT OR IGNORE INTO ledger_import_price_levels (batch_id, name, kind, percent_milli, is_active, items) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(batch_id)
        .bind(level.name.trim())
        .bind(serde_json::to_value(level.kind).expect("kinds serialize").as_str().unwrap_or_default().to_string())
        .bind(level.percent_milli)
        .bind(level.active as i64)
        .bind(serde_json::to_string(&level.items).expect("items serialize"))
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    for (customer, level) in &ledger.sales_lists.customer_price_levels {
        sqlx::query("UPDATE ledger_import_parties SET price_level = ?3 WHERE batch_id = ?1 AND kind = 'CUSTOMER' AND source_id = ?2")
            .bind(batch_id)
            .bind(customer)
            .bind(level.trim())
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
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
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
    /// `QBW` (a company file or backup), `QBMAC` (a QuickBooks for Mac company file), `IIF`, `QIF` or
    /// `GNUCASH`, from the file's extension.
    pub source_format: String,
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
    /// The file has lists but no transactions: posting it adds its accounts, customers, vendors
    /// and items, and nothing else.
    pub lists_only: bool,
    /// A lists-only batch whose lists were added and not reversed since.
    pub lists_added: bool,
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
        lists_added: i64,
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
        lists_only: transaction_count == 0,
        lists_added: head.lists_added != 0,
        mode: head.mode,
        documents,
        posted_as,
        fallbacks,
        id: batch_id,
        source_format: source_format(&head.source_path).into(),
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

/// Maps every unmapped source account that has lines (every account, when the file has no
/// transactions): to an existing active account with the same full name and type, receivables and
/// payables to the system accounts, and otherwise to a new account of the same type under the
/// (already mapped) parent. Parents are handled first.
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
                              AND json_extract(j.value, '$.accountId') = a.source_id)
                 -- A file of lists only brings its whole chart of accounts.
                 OR NOT EXISTS (SELECT 1 FROM ledger_import_transactions t WHERE t.batch_id = a.batch_id)) AS used
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
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
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
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
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
    /// Custom field values left out because their list already had 15 fields.
    pub custom_values_without_room: i64,
    /// Price levels: created, or matched by name to one already in LocalBooks (left as it is).
    pub price_levels_created: i64,
    pub price_levels_matched: i64,
}

/// Custom field values as staged: a JSON array of [label, value] pairs, or nothing.
fn custom_json(values: &[(String, String)]) -> Option<String> {
    (!values.is_empty()).then(|| serde_json::to_string(values).expect("pairs serialize"))
}

fn custom_from_json(json: Option<&str>) -> Vec<(String, String)> {
    json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default()
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
        sales_rep: Option<String>,
        custom_fields: Option<String>,
    }
    let parties: Vec<Party> = sqlx::query_as(
        "SELECT kind, source_id, name, parent_source_id, company_name, email, phone, address, terms_days, tax_id, is_1099,
                is_active, notes, sales_rep, custom_fields
         FROM ledger_import_parties WHERE batch_id = ?1 AND party_id IS NULL
         ORDER BY kind, length(name) - length(replace(name, ':', '')), name",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut result = ImportListsResult::default();
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    // Reps and shipping methods first, so customers can name their rep. Existing ones are matched
    // by initials or name; ones the file has hidden come in inactive.
    let sales_lists: Vec<(String, String, i64)> = sqlx::query_as("SELECT kind, name, is_active FROM ledger_import_sales_lists WHERE batch_id = ?1 ORDER BY kind, name")
        .bind(batch_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    for (kind, name, active) in sales_lists {
        let (table, found) = if kind == "REP" {
            ("sales_reps", crate::sales_lists::find_or_create_rep(&mut tx, &name).await)
        } else {
            ("ship_methods", crate::sales_lists::find_or_create_ship_method(&mut tx, &name).await)
        };
        let id = match found {
            Ok(id) => id,
            Err(e) => return Err(ledger::rollback(tx, e).await),
        };
        if let (Some(id), 0) = (id, active) {
            // Only a rep or method nothing uses yet is made inactive: it was just created.
            let unused = format!(
                "UPDATE {table} SET is_active = 0 WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM invoices WHERE {col} = ?1)
                   AND NOT EXISTS (SELECT 1 FROM sales_receipts WHERE {col} = ?1) AND NOT EXISTS (SELECT 1 FROM estimates WHERE {col} = ?1)",
                col = if kind == "REP" { "sales_rep_id" } else { "ship_method_id" }
            );
            if let Err(e) = sqlx::query(&unused).bind(id).execute(&mut *tx).await {
                return Err(ledger::rollback(tx, db_err(e)).await);
            }
        }
    }
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
        if created {
            let list = if p.kind == "CUSTOMER" { FieldList::Customer } else { FieldList::Vendor };
            let extras = async {
                if let Some(rep) = p.sales_rep.as_deref().filter(|_| p.kind == "CUSTOMER") {
                    let rep = crate::sales_lists::find_or_create_rep(&mut tx, rep).await?;
                    sqlx::query("UPDATE customers SET sales_rep_id = ?1 WHERE id = ?2").bind(rep).bind(id).execute(&mut *tx).await.map_err(db_err)?;
                }
                crate::custom_fields::set_values_by_label(&mut tx, list, id, &custom_from_json(p.custom_fields.as_deref())).await
            }
            .await;
            match extras {
                Ok(dropped) => result.custom_values_without_room += dropped as i64,
                Err(e) => return Err(ledger::rollback(tx, format!("{e} ({} {})", p.kind.to_lowercase(), p.name)).await),
            }
        }
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
        custom_fields: Option<String>,
    }
    // LocalBooks items post to income accounts only; the desktop program also allows expense and
    // cost accounts (for example a "Parts" item). Those items use the fallback income account,
    // and their description says which account they used in the file.
    let items: Vec<Item> = sqlx::query_as(
        "SELECT i.source_id, i.name, i.description, i.rate_cents,
                CASE WHEN t.classification = 'INCOME' THEN a.account_id END AS mapped_account,
                CASE WHEN a.account_id IS NULL OR t.classification <> 'INCOME' THEN a.full_name END AS other_account,
                i.is_taxable, i.is_active, i.custom_fields
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
                let inserted = match inserted {
                    Ok(id) => crate::custom_fields::set_values_by_label(&mut tx, FieldList::Item, id, &custom_from_json(item.custom_fields.as_deref()))
                        .await
                        .map(|dropped| (id, dropped))
                        .map_err(|e| format!("{e} (item {})", item.name)),
                    Err(e) => Err(format!("{} (item {})", db_err(e), item.name)),
                };
                match inserted {
                    Ok((id, dropped)) => {
                        result.items_created += 1;
                        result.custom_values_without_room += dropped as i64;
                        (id, true)
                    }
                    Err(e) => return Err(ledger::rollback(tx, e).await),
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
    // Price levels last, so per-item levels can name the items just added; then the customers this
    // import created get their default level.
    if let Err(e) = import_price_levels(&mut tx, batch_id, &mut result).await {
        return Err(ledger::rollback(tx, e).await);
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(result)
}

/// Creates the staged price levels that aren't in LocalBooks yet (matched by name), recording each
/// in the audit trail, and sets the default level of the customers the import created.
async fn import_price_levels(tx: &mut sqlx::SqliteConnection, batch_id: i64, result: &mut ImportListsResult) -> Result<(), String> {
    use crate::price_levels::{create_in, LevelItem, LevelKind, PriceLevelInput};
    let staged: Vec<(String, String, i64, i64, String)> = sqlx::query_as(
        "SELECT name, kind, percent_milli, is_active, items FROM ledger_import_price_levels WHERE batch_id = ?1 AND price_level_id IS NULL ORDER BY name",
    )
    .bind(batch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    for (name, kind, percent_milli, active, items) in staged {
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM price_levels WHERE name = ?1 COLLATE NOCASE").bind(&name).fetch_optional(&mut *tx).await.map_err(db_err)?;
        let (id, created) = match existing {
            Some(id) => {
                result.price_levels_matched += 1;
                (id, false)
            }
            None => {
                let mut level_items = Vec::new();
                for item in serde_json::from_str::<Vec<LevelItem>>(&items).unwrap_or_default() {
                    let product: Option<i64> = sqlx::query_scalar("SELECT product_id FROM ledger_import_items WHERE batch_id = ?1 AND source_id = ?2")
                        .bind(batch_id)
                        .bind(item.product_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(db_err)?
                        .flatten();
                    if let Some(product_id) = product.filter(|p| !level_items.iter().any(|i: &LevelItem| i.product_id == *p)) {
                        level_items.push(LevelItem { product_id, ..item });
                    }
                }
                let input = PriceLevelInput {
                    name: name.clone(),
                    kind: if kind == "PER_ITEM" { LevelKind::PerItem } else { LevelKind::FixedPercent },
                    percent_milli: percent_milli.clamp(crate::price_levels::MIN_PERCENT_MILLI, crate::price_levels::MAX_PERCENT_MILLI),
                    rounding: crate::item_prices::Rounding::None,
                    items: level_items,
                    is_active: active == 1,
                };
                let id = create_in(tx, &input, "PRICE_LEVEL_IMPORTED").await.map_err(|e| format!("{e} (price level {name})"))?;
                result.price_levels_created += 1;
                (id, true)
            }
        };
        sqlx::query("UPDATE ledger_import_price_levels SET price_level_id = ?3, created_by_import = ?4 WHERE batch_id = ?1 AND name = ?2")
            .bind(batch_id)
            .bind(&name)
            .bind(id)
            .bind(created as i64)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    sqlx::query(
        "UPDATE customers SET price_level_id = (
             SELECT l.price_level_id FROM ledger_import_parties p JOIN ledger_import_price_levels l ON l.batch_id = p.batch_id AND l.name = p.price_level COLLATE NOCASE
             WHERE p.batch_id = ?1 AND p.kind = 'CUSTOMER' AND p.party_id = customers.id)
         WHERE price_level_id IS NULL AND id IN (
             SELECT p.party_id FROM ledger_import_parties p JOIN ledger_import_price_levels l ON l.batch_id = p.batch_id AND l.name = p.price_level COLLATE NOCASE
             WHERE p.batch_id = ?1 AND p.kind = 'CUSTOMER' AND p.created_by_import = 1 AND l.price_level_id IS NOT NULL)",
    )
    .bind(batch_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    Ok(())
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
    /// For a file of lists only: the accounts, and the customers, vendors and items, it added.
    pub accounts: Option<AutoMapResult>,
    pub lists: Option<ImportListsResult>,
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

/// The kind of file a batch was read from, by its path's extension (or, for a file inside a
/// QuickBooks for Mac bundle, the bundle's).
fn source_format(path: &str) -> &'static str {
    match std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("iif") => "IIF",
        Some("qif") => "QIF",
        Some("csv" | "xlsx" | "xls") => "REPORT",
        Some("gnucash" | "gnc") => "GNUCASH",
        Some("qbxml") => "QBXML",
        _ if crate::qbmac_import::is_mac_path(std::path::Path::new(path)) => "QBMAC",
        _ => "QBW",
    }
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
/// are already posted from another import. A batch with no transactions adds its lists instead
/// (see `add_lists`).
/// Gives each mapped account the tax line the file assigns it, unless it already has one.
async fn copy_tax_lines(pool: &SqlitePool, batch_id: i64) -> Result<(), String> {
    sqlx::query(
        "UPDATE chart_of_accounts
         SET tax_line = (SELECT a.tax_line FROM ledger_import_accounts a
                         WHERE a.batch_id = ?1 AND a.account_id = chart_of_accounts.id AND a.tax_line IS NOT NULL
                         ORDER BY a.source_id LIMIT 1)
         WHERE tax_line IS NULL
           AND id IN (SELECT account_id FROM ledger_import_accounts WHERE batch_id = ?1 AND account_id IS NOT NULL AND tax_line IS NOT NULL)",
    )
    .bind(batch_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn post_batch(pool: &SqlitePool, input: &PostLedgerInput) -> Result<PostLedgerResult, String> {
    let batch_id = input.batch_id;
    require_batch(pool, batch_id).await?;
    copy_tax_lines(pool, batch_id).await?;
    if is_lists_only(pool, batch_id).await? {
        return add_lists(pool, batch_id).await;
    }
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

    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
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
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(result)
}

async fn is_lists_only(pool: &SqlitePool, batch_id: i64) -> Result<bool, String> {
    sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM ledger_import_transactions WHERE batch_id = ?1)")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// "Posts" a batch with lists but no transactions: matches or creates every account, then adds
/// the customers, vendors and items, the way a full import adds lists first. Posts no entries.
/// Does nothing while the lists are already added; after a Reverse it puts back what that
/// deactivated (records inactive in the file stay inactive).
async fn add_lists(pool: &SqlitePool, batch_id: i64) -> Result<PostLedgerResult, String> {
    let added: i64 = sqlx::query_scalar("SELECT lists_added FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if added != 0 {
        return Ok(PostLedgerResult::default());
    }
    let reactivated = sqlx::query(
        "UPDATE chart_of_accounts SET is_active = 1 WHERE is_active = 0 AND id IN
             (SELECT account_id FROM ledger_import_accounts WHERE batch_id = ?1 AND created_by_import = 1)",
    )
    .bind(batch_id)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected() as i64;
    for sql in [
        "UPDATE customers SET is_active = 1 WHERE is_active = 0 AND id IN
             (SELECT party_id FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'CUSTOMER' AND created_by_import = 1 AND is_active = 1)",
        "UPDATE vendors SET is_active = 1 WHERE is_active = 0 AND id IN
             (SELECT party_id FROM ledger_import_parties WHERE batch_id = ?1 AND kind = 'VENDOR' AND created_by_import = 1 AND is_active = 1)",
        "UPDATE products_services SET is_active = 1 WHERE is_active = 0 AND id IN
             (SELECT product_id FROM ledger_import_items WHERE batch_id = ?1 AND created_by_import = 1 AND is_active = 1)",
    ] {
        sqlx::query(sql).bind(batch_id).execute(pool).await.map_err(db_err)?;
    }
    let mut accounts = auto_map_accounts(pool, batch_id).await?;
    accounts.reactivated += reactivated;
    let lists = import_lists(pool, batch_id).await?;
    sqlx::query("UPDATE ledger_import_batches SET lists_added = 1, posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(batch_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(PostLedgerResult { accounts: Some(accounts), lists: Some(lists), ..Default::default() })
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
/// once they have no other activity. A lists-only import has no entries: its accounts, customers,
/// vendors and items are deactivated.
pub async fn reverse_batch(pool: &SqlitePool, batch_id: i64) -> Result<ReverseLedgerResult, String> {
    require_batch(pool, batch_id).await?;
    let (mode, lists_added): (String, i64) = sqlx::query_as("SELECT mode, lists_added FROM ledger_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let lists_only = is_lists_only(pool, batch_id).await?;
    if lists_only && lists_added == 0 {
        return Err("QBW_NOTHING_TO_REVERSE: this import's lists are not in your books".into());
    }
    if mode == "FULL" {
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
    if entries.is_empty() && !lists_only {
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
    if lists_only {
        sqlx::query("UPDATE ledger_import_batches SET lists_added = 0 WHERE id = ?1").bind(batch_id).execute(pool).await.map_err(db_err)?;
    }
    Ok(result)
}

/// Deletes a batch's staged rows. Refuses while it has posted entries (or added lists) in effect.
pub async fn discard_batch(pool: &SqlitePool, batch_id: i64) -> Result<(), String> {
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) + COALESCE((SELECT lists_added FROM ledger_import_batches WHERE id = ?1), 0)
         FROM ledger_import_transactions t JOIN journal_entries e ON e.id = t.journal_entry_id
         WHERE t.batch_id = ?1 AND e.is_voided = 0",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if live > 0 {
        return Err("QBW_POSTED: reverse this import's posted entries before deleting it".into());
    }
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    for sql in [
        "DELETE FROM ledger_import_lines WHERE batch_id = ?1",
        "DELETE FROM ledger_import_unreadable WHERE batch_id = ?1",
        "DELETE FROM ledger_import_parties WHERE batch_id = ?1",
        "DELETE FROM ledger_import_items WHERE batch_id = ?1",
        "DELETE FROM ledger_import_sales_lists WHERE batch_id = ?1",
        "DELETE FROM ledger_import_price_levels WHERE batch_id = ?1",
        "DELETE FROM ledger_import_transactions WHERE batch_id = ?1",
        "DELETE FROM ledger_import_accounts WHERE batch_id = ?1",
        "DELETE FROM ledger_import_batches WHERE id = ?1",
    ] {
        sqlx::query(sql).bind(batch_id).execute(&mut *tx).await.map_err(db_err)?;
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

#[cfg(test)]
mod tests;
