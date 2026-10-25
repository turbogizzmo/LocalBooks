//! Desktop Migration Wizard: .qbw company-file import through a staging sandbox.
//!
//! 1. [`extract`] reads the company file with the `openqbw` parser (read-only) and returns flat
//!    invoice line items: the parent invoice's id in the file, a date, signed cents, and the internal
//!    table the line came from. openqbw 0.1 does not decode account names, customers, or memos, so
//!    the source table is the only label the file offers for mapping.
//! 2. [`stage`] stores those lines in `staging_raw_transactions`, outside the ledger.
//! 3. The user maps each label to a LocalBooks income account ([`set_mapping`]), or lets
//!    [`auto_create_accounts`] add one per unmapped label.
//! 4. [`post_batch`] turns each source invoice into one balanced journal entry — credits to the
//!    mapped income accounts, offset to an account the user chooses — and posts every entry through
//!    [`ledger::post_entry_in_tx`] in a single transaction, so either the whole batch lands with every
//!    ledger rule enforced, or nothing does.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use chrono::{Days, NaiveDate};
use openqbw::{iter_lineitems_with_attribution, iter_transaction_headers, sa_day_to_unix_day, AmountType, PageAttribution};
use opensqlany::{ApModel, PageStore};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{self, db_err, EntrySource, JournalEntry, JournalLine};

/// Label for line items whose page the parser could not attribute to a table.
pub const UNATTRIBUTED: &str = "(unattributed)";

/// The .qbw tables that hold invoice line items. openqbw's line-item pattern also matches bytes
/// on pages of many unrelated tables (payroll, audit, budgets, internal catalogs), so only lines
/// attributed to these tables are imported; everything else is counted as ignored.
pub const INVOICE_LINE_TABLES: [&str; 2] = ["abmc_invoice_lineitem", "abmc_invoice_inventory_lineitem"];

/// Description given to income accounts the importer creates (also how a reversal finds them).
const IMPORTER_ACCOUNT_NOTE: &str = "Created by the desktop migration wizard for";

/// Extensions the importer accepts from the file picker and drop zone.
pub const ACCEPTED_EXTENSIONS: [&str; 3] = ["qbw", "qbb", "qbm"];

/// One invoice line item as read from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    pub qb_invoice_id: String,
    pub qb_item_id: Option<String>,
    pub date: Option<NaiveDate>,
    pub amount_cents: i64,
    pub label: String,
    pub amount_type: &'static str,
    pub page_number: i64,
    pub page_offset: i64,
}

/// Everything [`extract`] learned about a file.
#[derive(Debug, Clone)]
pub struct Extraction {
    pub file_size_bytes: i64,
    pub page_count: i64,
    pub attributed_tables: i64,
    pub header_count: i64,
    pub skipped_line_count: i64,
    /// Line-item matches on pages that do not belong to an invoice line table.
    pub ignored_line_count: i64,
    pub lines: Vec<RawLine>,
}

/// A progress report while a file is read: `stage` names the step, `done`/`total` measure it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
}

fn unix_day_to_date(unix_day: i64) -> Option<NaiveDate> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    if unix_day >= 0 {
        epoch.checked_add_days(Days::new(unix_day as u64))
    } else {
        epoch.checked_sub_days(Days::new(unix_day.unsigned_abs()))
    }
}

fn amount_type_code(t: AmountType) -> &'static str {
    match t {
        AmountType::None => "NONE",
        AmountType::OneByteOne => "TYPE_01",
        AmountType::Standard => "TYPE_02",
        AmountType::Deferred => "DEFERRED",
        AmountType::Other(_) => "OTHER",
    }
}

/// Reads invoice line items out of a .qbw company file. Blocking and CPU-bound: call it from
/// a blocking thread. The file is only read, never written.
pub fn extract(path: &Path, progress: &mut dyn FnMut(Progress)) -> Result<Extraction, String> {
    progress(Progress { stage: "Reading file", done: 0, total: 1 });
    let store = PageStore::open(path).map_err(|e| match e {
        opensqlany::Error::Io(io) => format!("QBW_READ: could not read {}: {io}", path.display()),
        _ => not_a_company_file(path),
    })?;
    store.superblock().map_err(|_| not_a_company_file(path))?;
    let pages = store.page_count();
    progress(Progress { stage: "Reading file", done: 1, total: 1 });

    progress(Progress { stage: "Decoding pages", done: 0, total: 1 });
    let model = ApModel::learn(&store);
    progress(Progress { stage: "Decoding pages", done: 1, total: 1 });

    progress(Progress { stage: "Reading table catalog", done: 0, total: 1 });
    let attribution = PageAttribution::build(&store, &model);
    progress(Progress { stage: "Reading table catalog", done: 1, total: 1 });

    // Header dates fill in line items whose own date field was not found.
    let mut header_dates: HashMap<String, NaiveDate> = HashMap::new();
    let mut header_count = 0i64;
    for header in iter_transaction_headers(&store, &model, &attribution) {
        header_count += 1;
        if let Some(date) = header.txn_date_raw.and_then(sa_day_to_unix_day).and_then(unix_day_to_date) {
            header_dates.entry(header.qb_id).or_insert(date);
        }
    }

    let mut lines = Vec::new();
    let mut skipped = 0i64;
    let mut ignored = 0i64;
    let mut last_report = 0u64;
    for mut item in iter_lineitems_with_attribution(&store, &model, &attribution) {
        if item.page_number >= last_report + 64 {
            last_report = item.page_number;
            progress(Progress { stage: "Extracting line items", done: item.page_number, total: pages });
        }
        let label = match item.source_table.take() {
            Some(table) if INVOICE_LINE_TABLES.contains(&table.as_str()) => table,
            _ => {
                ignored += 1;
                continue;
            }
        };
        let cents = match item.amount_cents_signed {
            Some(c) if c != 0 => i64::from(c),
            _ => {
                skipped += 1;
                continue;
            }
        };
        let date = item
            .txn_date_days_since_unix()
            .and_then(unix_day_to_date)
            .or_else(|| header_dates.get(&item.invoice_id).copied());
        lines.push(RawLine {
            qb_invoice_id: item.invoice_id,
            qb_item_id: item.item_qb_id,
            date,
            amount_cents: cents,
            label,
            amount_type: amount_type_code(item.amount_type),
            page_number: item.page_number as i64,
            page_offset: item.page_offset as i64,
        });
    }
    progress(Progress { stage: "Extracting line items", done: pages, total: pages });

    let extraction = Extraction {
        file_size_bytes: store.size_bytes() as i64,
        page_count: pages as i64,
        attributed_tables: attribution.len() as i64,
        header_count,
        skipped_line_count: skipped,
        ignored_line_count: ignored,
        lines,
    };
    ensure_found(path, &extraction)?;
    Ok(extraction)
}

/// Refuses an extraction with no invoice lines, so an unreadable file never stages as an empty
/// import. No transaction headers at all means the parser could not find the file's tables,
/// which happens on file versions it does not support yet.
fn ensure_found(path: &Path, extraction: &Extraction) -> Result<(), String> {
    if !extraction.lines.is_empty() {
        return Ok(());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if extraction.header_count == 0 {
        Err(format!(
            "QBW_UNSUPPORTED_VERSION: no transactions could be read from {name}. It was most likely saved by a version of the desktop program this importer cannot read yet. Nothing was imported"
        ))
    } else {
        Err(format!(
            "QBW_NO_INVOICES: {name} has {} transactions but no invoice lines this importer can read. Nothing was imported",
            extraction.header_count
        ))
    }
}

fn not_a_company_file(path: &Path) -> String {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "qbb" || ext == "qbm" {
        format!(
            "QBW_COMPRESSED_BACKUP: {} is a compressed backup, not a company file. Restore it in the program that created it, then import the restored .qbw file",
            path.display()
        )
    } else {
        format!("QBW_NOT_COMPANY_FILE: {} is not a .qbw company file", path.display())
    }
}

/// Turns an internal table name into a readable label: `abmc_invoice_inventory_lineitem` →
/// `Invoice inventory line items`.
pub fn display_label(raw: &str) -> String {
    if raw == UNATTRIBUTED {
        return "Unattributed invoice lines".into();
    }
    let core = raw.strip_prefix("abmc_").unwrap_or(raw);
    let words: Vec<String> = core
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| if w.eq_ignore_ascii_case("lineitem") { "line items".to_string() } else { w.to_ascii_lowercase() })
        .collect();
    let joined = words.join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => raw.to_string(),
    }
}

/// Stores an extraction as a new staging batch and returns its id.
pub async fn stage(pool: &SqlitePool, source_path: &Path, extraction: &Extraction) -> Result<i64, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let file_name = source_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let batch_id: i64 = sqlx::query_scalar(
        "INSERT INTO staging_import_batches
            (source_path, file_name, file_size_bytes, page_count, attributed_tables, header_count, skipped_line_count, ignored_line_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(source_path.display().to_string())
    .bind(&file_name)
    .bind(extraction.file_size_bytes)
    .bind(extraction.page_count)
    .bind(extraction.attributed_tables)
    .bind(extraction.header_count)
    .bind(extraction.skipped_line_count)
    .bind(extraction.ignored_line_count)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;

    for line in &extraction.lines {
        let description = match &line.qb_item_id {
            Some(item) => format!("Imported invoice {} · item {item}", line.qb_invoice_id),
            None => format!("Imported invoice {}", line.qb_invoice_id),
        };
        sqlx::query(
            "INSERT INTO staging_raw_transactions
                (batch_id, date, raw_description, raw_amount_cents, raw_account_label,
                 qb_invoice_id, qb_item_id, amount_type, page_number, page_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (batch_id, page_number, page_offset) DO NOTHING",
        )
        .bind(batch_id)
        .bind(line.date.map(|d| d.to_string()))
        .bind(description)
        .bind(line.amount_cents)
        .bind(&line.label)
        .bind(&line.qb_invoice_id)
        .bind(&line.qb_item_id)
        .bind(line.amount_type)
        .bind(line.page_number)
        .bind(line.page_offset)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }

    sqlx::query(
        "INSERT INTO staging_account_map (batch_id, raw_account_label)
         SELECT DISTINCT ?1, raw_account_label FROM staging_raw_transactions WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(batch_id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub ignored_line_count: i64,
    /// Entries from this import still in effect (posted and not voided).
    pub live_entry_count: i64,
    pub total_cents: i64,
    pub id: i64,
    pub file_name: String,
    pub source_path: String,
    pub file_size_bytes: i64,
    pub page_count: i64,
    pub attributed_tables: i64,
    pub header_count: i64,
    pub skipped_line_count: i64,
    pub imported_at: String,
    pub posted_at: Option<String>,
    pub line_count: i64,
    pub invoice_count: i64,
    pub undated_invoice_count: i64,
    pub already_posted_invoice_count: i64,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub labels: Vec<LabelMapping>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LabelMapping {
    pub raw_label: String,
    #[sqlx(skip)]
    pub display_label: String,
    pub line_count: i64,
    pub total_cents: i64,
    pub account_id: Option<i64>,
    pub account_name: Option<String>,
}

/// Live postings already made for a source invoice id. A voided entry stays POSTED with
/// `is_voided = 1` (its reversal offsets it), which frees the invoice to be posted again.
const LIVE_POSTED: &str = "SELECT p.qb_invoice_id FROM qbw_posted_invoices p
                           JOIN journal_entries e ON e.id = p.journal_entry_id
                           WHERE e.is_voided = 0";

pub async fn batch_summary(pool: &SqlitePool, batch_id: i64) -> Result<BatchSummary, String> {
    #[derive(sqlx::FromRow)]
    struct Head {
        id: i64,
        file_name: String,
        source_path: String,
        file_size_bytes: i64,
        page_count: i64,
        attributed_tables: i64,
        header_count: i64,
        skipped_line_count: i64,
        ignored_line_count: i64,
        imported_at: String,
        posted_at: Option<String>,
    }
    let head: Head = sqlx::query_as(
        "SELECT id, file_name, source_path, file_size_bytes, page_count, attributed_tables, header_count,
                skipped_line_count, ignored_line_count, imported_at, posted_at
         FROM staging_import_batches WHERE id = ?1",
    )
    .bind(batch_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("QBW_NO_BATCH: import #{batch_id} does not exist"))?;

    let (line_count, invoice_count, first_date, last_date, total_cents): (i64, i64, Option<String>, Option<String>, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(DISTINCT qb_invoice_id), MIN(date), MAX(date), COALESCE(SUM(raw_amount_cents), 0)
         FROM staging_raw_transactions WHERE batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let undated_invoice_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT qb_invoice_id FROM staging_raw_transactions WHERE batch_id = ?1
                               GROUP BY qb_invoice_id HAVING MAX(date) IS NULL)",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let already_posted_invoice_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT qb_invoice_id) FROM staging_raw_transactions
         WHERE batch_id = ?1 AND qb_invoice_id IN ({LIVE_POSTED})"
    ))
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    let live_entry_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM qbw_posted_invoices p JOIN journal_entries e ON e.id = p.journal_entry_id
         WHERE p.batch_id = ?1 AND e.is_voided = 0",
    )
    .bind(batch_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    let mut labels: Vec<LabelMapping> = sqlx::query_as(
        "SELECT m.raw_account_label AS raw_label,
                COUNT(r.id) AS line_count,
                COALESCE(SUM(r.raw_amount_cents), 0) AS total_cents,
                a.id AS account_id,
                a.name AS account_name
         FROM staging_account_map m
         LEFT JOIN staging_raw_transactions r ON r.batch_id = m.batch_id AND r.raw_account_label = m.raw_account_label
         LEFT JOIN chart_of_accounts a ON a.id = m.account_id
         WHERE m.batch_id = ?1
         GROUP BY m.raw_account_label
         ORDER BY line_count DESC, m.raw_account_label",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for label in &mut labels {
        label.display_label = display_label(&label.raw_label);
    }

    Ok(BatchSummary {
        ignored_line_count: head.ignored_line_count,
        live_entry_count,
        total_cents,
        id: head.id,
        file_name: head.file_name,
        source_path: head.source_path,
        file_size_bytes: head.file_size_bytes,
        page_count: head.page_count,
        attributed_tables: head.attributed_tables,
        header_count: head.header_count,
        skipped_line_count: head.skipped_line_count,
        imported_at: head.imported_at,
        posted_at: head.posted_at,
        line_count,
        invoice_count,
        undated_invoice_count,
        already_posted_invoice_count,
        first_date,
        last_date,
        labels,
    })
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BatchListItem {
    pub id: i64,
    pub file_name: String,
    pub imported_at: String,
    pub posted_at: Option<String>,
    pub line_count: i64,
    pub live_entry_count: i64,
}

pub async fn list_batches(pool: &SqlitePool) -> Result<Vec<BatchListItem>, String> {
    sqlx::query_as(
        "SELECT b.id, b.file_name, b.imported_at, b.posted_at,
                (SELECT COUNT(*) FROM staging_raw_transactions r WHERE r.batch_id = b.id) AS line_count,
                (SELECT COUNT(*) FROM qbw_posted_invoices p JOIN journal_entries e ON e.id = p.journal_entry_id
                 WHERE p.batch_id = b.id AND e.is_voided = 0) AS live_entry_count
         FROM staging_import_batches b ORDER BY b.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Income accounts are the only valid targets: every extracted line is an invoice line.
async fn require_income_account(pool: &SqlitePool, account_id: i64) -> Result<(), String> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT t.classification, a.is_active FROM chart_of_accounts a
         JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match row {
        None => Err(format!("QBW_BAD_ACCOUNT: account #{account_id} does not exist")),
        Some((class, _)) if class != "INCOME" => {
            Err("QBW_BAD_ACCOUNT: imported invoice lines can only map to an Income or Other Income account".into())
        }
        Some((_, 0)) => Err("QBW_BAD_ACCOUNT: that account is inactive".into()),
        Some(_) => Ok(()),
    }
}

/// Maps (or with `None`, unmaps) one raw label in a batch.
pub async fn set_mapping(pool: &SqlitePool, batch_id: i64, raw_label: &str, account_id: Option<i64>) -> Result<(), String> {
    if let Some(id) = account_id {
        require_income_account(pool, id).await?;
    }
    let updated = sqlx::query("UPDATE staging_account_map SET account_id = ?3 WHERE batch_id = ?1 AND raw_account_label = ?2")
        .bind(batch_id)
        .bind(raw_label)
        .bind(account_id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if updated == 0 {
        return Err(format!("QBW_NO_LABEL: import #{batch_id} has no label '{raw_label}'"));
    }
    Ok(())
}

/// Creates an Income account for every unmapped label (reusing a top-level account that already
/// has the generated name) and maps the label to it. Returns how many accounts were created.
pub async fn auto_create_accounts(pool: &SqlitePool, batch_id: i64) -> Result<i64, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let unmapped: Vec<String> = sqlx::query_scalar(
        "SELECT raw_account_label FROM staging_account_map WHERE batch_id = ?1 AND account_id IS NULL ORDER BY raw_account_label",
    )
    .bind(batch_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;

    let mut created = 0i64;
    for raw in unmapped {
        let name = format!("Imported {}", display_label(&raw).to_lowercase());
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT a.id FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
             WHERE a.parent_id IS NULL AND a.name = ?1 COLLATE NOCASE AND t.classification = 'INCOME' AND a.is_active = 1",
        )
        .bind(&name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let account_id = match existing {
            Some(id) => id,
            None => {
                // Next free number in 4500-4899, the import block of the income range.
                let number: Option<String> = sqlx::query_scalar(
                    "WITH RECURSIVE n(v) AS (SELECT 4500 UNION ALL SELECT v + 1 FROM n WHERE v < 4899)
                     SELECT CAST(v AS TEXT) FROM n
                     WHERE CAST(v AS TEXT) NOT IN (SELECT account_number FROM chart_of_accounts WHERE account_number IS NOT NULL)
                     ORDER BY v LIMIT 1",
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?;
                created += 1;
                sqlx::query_scalar(
                    "INSERT INTO chart_of_accounts (account_number, name, account_type, description)
                     VALUES (?1, ?2, 'INCOME', ?3) RETURNING id",
                )
                .bind(number)
                .bind(&name)
                .bind(format!("{IMPORTER_ACCOUNT_NOTE} {raw}"))
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err)?
            }
        };
        sqlx::query("UPDATE staging_account_map SET account_id = ?3 WHERE batch_id = ?1 AND raw_account_label = ?2")
            .bind(batch_id)
            .bind(&raw)
            .bind(account_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(created)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostBatchInput {
    pub batch_id: i64,
    /// The balancing side of every imported invoice (e.g. Opening Balance Equity).
    pub offset_account_id: i64,
    /// Date for invoices whose date the parser could not recover; without it they are skipped.
    #[serde(default)]
    pub undated_fallback: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostBatchResult {
    pub entries_posted: i64,
    pub lines_posted: i64,
    pub total_cents: i64,
    pub skipped_already_posted: i64,
    pub skipped_undated: i64,
    pub skipped_zero_total: i64,
}

/// Posts every unposted invoice in the batch as one balanced journal entry each, all inside a
/// single transaction. Refuses to start while any label is unmapped.
pub async fn post_batch(pool: &SqlitePool, input: &PostBatchInput) -> Result<PostBatchResult, String> {
    let batch_id = input.batch_id;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM staging_import_batches WHERE id = ?1")
        .bind(batch_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if exists.is_none() {
        return Err(format!("QBW_NO_BATCH: import #{batch_id} does not exist"));
    }

    let offset: Option<(String, i64)> = sqlx::query_as(
        "SELECT t.classification, a.is_active FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(input.offset_account_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    match offset {
        None => return Err("QBW_BAD_OFFSET: choose an offset account".into()),
        Some((class, _)) if class == "INCOME" || class == "EXPENSE" => {
            return Err("QBW_BAD_OFFSET: the offset must be a balance sheet account, such as Opening Balance Equity".into())
        }
        Some((_, 0)) => return Err("QBW_BAD_OFFSET: the offset account is inactive".into()),
        Some(_) => {}
    }

    let map: Vec<(String, Option<i64>)> =
        sqlx::query_as("SELECT raw_account_label, account_id FROM staging_account_map WHERE batch_id = ?1")
            .bind(batch_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let unmapped: Vec<String> = map.iter().filter(|(_, a)| a.is_none()).map(|(l, _)| display_label(l)).collect();
    if !unmapped.is_empty() {
        return Err(format!("QBW_UNMAPPED: map every label before posting ({})", unmapped.join(", ")));
    }
    let map: HashMap<String, i64> = map.into_iter().map(|(l, a)| (l, a.expect("checked above"))).collect();
    for account_id in map.values() {
        require_income_account(pool, *account_id).await?;
    }

    #[derive(sqlx::FromRow)]
    struct Row {
        qb_invoice_id: String,
        date: Option<String>,
        raw_amount_cents: i64,
        raw_account_label: String,
        already_posted: i64,
    }
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT qb_invoice_id, date, raw_amount_cents, raw_account_label,
                qb_invoice_id IN ({LIVE_POSTED}) AS already_posted
         FROM staging_raw_transactions WHERE batch_id = ?1 ORDER BY qb_invoice_id, id"
    ))
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    struct Invoice {
        date: Option<NaiveDate>,
        lines: usize,
        by_account: BTreeMap<i64, i64>,
        already_posted: bool,
    }
    let mut invoices: BTreeMap<String, Invoice> = BTreeMap::new();
    for row in rows {
        let date = row.date.as_deref().and_then(|d| d.parse::<NaiveDate>().ok());
        let inv = invoices.entry(row.qb_invoice_id).or_insert(Invoice {
            date: None,
            lines: 0,
            by_account: BTreeMap::new(),
            already_posted: row.already_posted != 0,
        });
        // An invoice dates from its earliest dated line.
        inv.date = match (inv.date, date) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        inv.lines += 1;
        let account = map[&row.raw_account_label];
        let slot = inv.by_account.entry(account).or_insert(0);
        *slot = slot.checked_add(row.raw_amount_cents).ok_or("QBW_OVERFLOW: invoice total out of range")?;
    }

    let mut result = PostBatchResult::default();
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    for (qb_id, inv) in invoices {
        if inv.already_posted {
            result.skipped_already_posted += 1;
            continue;
        }
        let Some(date) = inv.date.or(input.undated_fallback) else {
            result.skipped_undated += 1;
            continue;
        };
        let mut lines = Vec::new();
        let mut total = 0i64;
        for (account, cents) in &inv.by_account {
            match cents.signum() {
                1 => lines.push(JournalLine::credit(*account, *cents)),
                -1 => lines.push(JournalLine::debit(*account, -cents)),
                _ => continue,
            }
            total = total.checked_add(*cents).ok_or("QBW_OVERFLOW: invoice total out of range")?;
        }
        if total == 0 {
            result.skipped_zero_total += 1;
            continue;
        }
        let offset = if total > 0 { JournalLine::debit(input.offset_account_id, total) } else { JournalLine::credit(input.offset_account_id, -total) };
        lines.push(offset);
        let entry = JournalEntry {
            id: None,
            date,
            description: format!("Desktop migration · invoice {qb_id}"),
            lines,
            currency: None,
        };
        let entry_id = match ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
            Ok(id) => id,
            Err(err) => return Err(ledger::rollback(tx, format!("{err} (imported invoice {qb_id})")).await),
        };
        // A register row left by a voided posting is replaced; a live one cannot exist here.
        if let Err(e) = sqlx::query(
            "INSERT INTO qbw_posted_invoices (qb_invoice_id, journal_entry_id, batch_id) VALUES (?1, ?2, ?3)
             ON CONFLICT (qb_invoice_id) DO UPDATE SET journal_entry_id = excluded.journal_entry_id,
                 batch_id = excluded.batch_id, posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(&qb_id)
        .bind(entry_id)
        .bind(batch_id)
        .execute(&mut *tx)
        .await
        {
            return Err(ledger::rollback(tx, db_err(e)).await);
        }
        result.entries_posted += 1;
        result.lines_posted += inv.lines as i64;
        result.total_cents = result.total_cents.checked_add(total).ok_or("QBW_OVERFLOW: batch total out of range")?;
    }
    if result.entries_posted > 0 {
        if let Err(e) = sqlx::query("UPDATE staging_import_batches SET posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseBatchResult {
    pub entries_voided: i64,
    pub accounts_deactivated: i64,
}

/// Undoes a posted import: voids every entry it posted (each gets a reversing entry, as with any
/// void) and deactivates the income accounts the importer created for it once they are back to a
/// zero balance with no other activity. Posted history cannot be deleted; this is the undo.
pub async fn reverse_batch(pool: &SqlitePool, batch_id: i64) -> Result<ReverseBatchResult, String> {
    let entries: Vec<i64> = sqlx::query_scalar(
        "SELECT p.journal_entry_id FROM qbw_posted_invoices p JOIN journal_entries e ON e.id = p.journal_entry_id
         WHERE p.batch_id = ?1 AND e.is_voided = 0 ORDER BY p.journal_entry_id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if entries.is_empty() {
        return Err("QBW_NOTHING_TO_REVERSE: this import has no posted entries in effect".into());
    }
    let mut result = ReverseBatchResult::default();
    for entry in entries {
        ledger::void_transaction(pool, entry).await?;
        result.entries_voided += 1;
    }
    // Importer-created accounts mapped in this batch whose only activity is entries now voided.
    let accounts: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT a.id FROM staging_account_map m JOIN chart_of_accounts a ON a.id = m.account_id
         WHERE m.batch_id = ?1 AND a.is_active = 1 AND a.is_system = 0
           AND (a.description LIKE ?2 || '%'
                OR a.description LIKE 'Created by the QuickBooks Desktop importer%') -- wording used before 2026-10-14
           AND NOT EXISTS (
               SELECT 1 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
               WHERE l.account_id = a.id AND e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL)",
    )
    .bind(batch_id)
    .bind(IMPORTER_ACCOUNT_NOTE)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for account in accounts {
        sqlx::query("UPDATE chart_of_accounts SET is_active = 0 WHERE id = ?1").bind(account).execute(pool).await.map_err(db_err)?;
        result.accounts_deactivated += 1;
    }
    Ok(result)
}

/// Deletes a batch's staged rows. Posted journal entries are unaffected (void them in the ledger).
pub async fn discard_batch(pool: &SqlitePool, batch_id: i64) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    for sql in [
        "DELETE FROM staging_raw_transactions WHERE batch_id = ?1",
        "DELETE FROM staging_account_map WHERE batch_id = ?1",
        "DELETE FROM staging_import_batches WHERE id = ?1",
    ] {
        sqlx::query(sql).bind(batch_id).execute(&mut *tx).await.map_err(db_err)?;
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

#[cfg(test)]
mod tests;
