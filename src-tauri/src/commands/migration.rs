//! Desktop Migration Wizard: importing other programs' books through a staging sandbox.

use super::prelude::*;

handlers![
    ingest_raw_qbw,
    ingest_report_exports,
    list_qbw_batches,
    get_qbw_batch,
    set_qbw_mapping,
    auto_create_qbw_accounts,
    post_qbw_batch,
    reverse_qbw_batch,
    discard_qbw_batch,
    list_ledger_batches,
    get_ledger_batch,
    set_ledger_account,
    auto_map_ledger_accounts,
    import_ledger_lists,
    post_ledger_batch,
    start_over_ledger_batch,
    reverse_ledger_batch,
    discard_ledger_batch,
];

pub const QBW_PROGRESS: &str = "qbw-import-progress";

/// What a company file was read into: invoice lines (files openqbw reads) or the full ledger
/// (newer files, read by the record-layer decoder).
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant, reason = "built once per import and sent straight to the screen")]
pub enum IngestResult {
    Invoices { batch: BatchSummary },
    /// `sources` says what each report export was read as; it is empty for other files.
    Ledger { batch: LedgerBatchSummary, sources: Vec<String> },
}

/// Reads a .qbw company file, the company file inside a .qbb backup, a QuickBooks for Mac company
/// file (.qb2012 and earlier), an .iif (QuickBooks) or .qif (Quicken) export, or a GnuCash book
/// (.gnucash, XML or SQLite) into a new staging batch,
/// reporting progress as `qbw-import-progress` events. Nothing is posted to the ledger.
///
/// The invoice-line reader runs first, so files it supports import as before; when it finds
/// nothing (newer file versions), the record-layer decoder reads the whole ledger instead.
#[tauri::command]
pub async fn ingest_raw_qbw(app: AppHandle, db: State<'_, Db>, file_path: String) -> Result<IngestResult, String> {
    let path = PathBuf::from(&file_path);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mac = crate::qbmac_import::is_mac_path(&path);
    if !mac && !qbw_import::ACCEPTED_EXTENSIONS.contains(&ext.as_str()) && !matches!(ext.as_str(), "iif" | "qif" | "gnucash" | "gnc") {
        return Err("QBW_BAD_EXTENSION: choose a .qbw company file, a .qbb backup, a QuickBooks for Mac company file (.qb2012), an .iif or .qif export, a .gnucash book, or QuickBooks report exports (.csv, .xlsx)".into());
    }
    let file_size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
    let progress = |stage: &'static str, done: u64, total: u64| {
        let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage, done, total });
    };

    // IIF and QIF exports, GnuCash books and QuickBooks for Mac company files read straight into a
    // ledger, staged like a company file.
    if mac || matches!(ext.as_str(), "iif" | "qif" | "gnucash" | "gnc") {
        progress("Reading file", 0, 1);
        let ledger = if mac {
            crate::qbmac_import::read(&path).await?
        } else if matches!(ext.as_str(), "gnucash" | "gnc") {
            gnucash_import::read(&path).await?
        } else {
            let bytes = std::fs::read(&path).map_err(|e| format!("QBW_UNREADABLE: {}: {e}", path.display()))?;
            let account = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported account".into());
            let is_iif = ext == "iif";
            tauri::async_runtime::spawn_blocking(move || if is_iif { iif_import::read(&bytes) } else { qif_import::read(&bytes, &account) })
                .await
                .map_err(|e| format!("QBW_PARSE_FAILED: the file could not be read ({e})"))??
        };
        progress("Reading file", 1, 1);
        // A file of lists alone imports (it adds its lists); one with nothing at all does not.
        if ledger.transactions.is_empty()
            && ledger.unbalanced.is_empty()
            && ledger.accounts.is_empty()
            && ledger.customers.is_empty()
            && ledger.vendors.is_empty()
            && ledger.items.is_empty()
        {
            return Err(format!("{}_EMPTY: this file has no accounts, lists or transactions to import", ext.to_ascii_uppercase()));
        }
        let pool = db.pool().await;
        progress("Staging rows", 0, 1);
        let batch_id = qbw_ledger_import::stage(&pool, &path, file_size, &ledger).await?;
        progress("Staging rows", 1, 1);
        return Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await?, sources: Vec::new() });
    }

    // A backup holds the company file compressed; unpack it next to LocalBooks' own cache.
    let unpacked = if qbw_backup::is_backup(&path) {
        progress("Unpacking backup", 0, 1);
        let dir = app.path().app_cache_dir().map_err(|e| format!("QBW_UNPACK: {e}"))?;
        let source = path.clone();
        let unpacked = tauri::async_runtime::spawn_blocking(move || qbw_backup::unpack(&source, &dir))
            .await
            .map_err(|e| format!("QBW_UNPACK: the backup could not be unpacked ({e})"))??;
        progress("Unpacking backup", 1, 1);
        Some(unpacked)
    } else {
        None
    };
    let company = unpacked.as_ref().map_or(path.clone(), |u| u.path.clone());

    let emitter = app.clone();
    let source = company.clone();
    // A panic inside a parser surfaces here as a join error instead of taking the app down.
    let extraction = tauri::async_runtime::spawn_blocking(move || {
        qbw_import::extract(&source, &mut |p| {
            let _ = emitter.emit(QBW_PROGRESS, p);
        })
    })
    .await
    .map_err(|e| format!("QBW_PARSE_FAILED: the .qbw file could not be read ({e})"))?;
    let pool = db.pool().await;
    match extraction {
        Ok(extraction) => {
            let staged = (extraction.lines.len() as u64).max(1);
            progress("Staging rows", 0, staged);
            let batch_id = qbw_import::stage(&pool, &path, &extraction).await?;
            progress("Staging rows", staged, staged);
            Ok(IngestResult::Invoices { batch: qbw_import::batch_summary(&pool, batch_id).await? })
        }
        Err(err) if err.starts_with("QBW_UNSUPPORTED_VERSION") || err.starts_with("QBW_NO_INVOICES") => {
            progress("Decoding the full ledger", 0, 1);
            let source = company.clone();
            let ledger = tauri::async_runtime::spawn_blocking(move || qbw_company::read_ledger(&source))
                .await
                .map_err(|e| format!("QBW_PARSE_FAILED: the company file could not be decoded ({e})"))??;
            progress("Decoding the full ledger", 1, 1);
            if ledger.transactions.is_empty() {
                return Err(err);
            }
            progress("Staging rows", 0, 1);
            let batch_id = qbw_ledger_import::stage(&pool, &path, file_size, &ledger).await?;
            progress("Staging rows", 1, 1);
            Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await?, sources: Vec::new() })
        }
        Err(err) => Err(err),
    }
}

/// Reads QuickBooks report exports (a Journal or General Ledger, and any of the Account Listing,
/// contact lists and Item List, as .csv or .xlsx) into one new staging batch, named after the
/// report. Nothing is posted to the ledger.
#[tauri::command]
pub async fn ingest_report_exports(app: AppHandle, db: State<'_, Db>, file_paths: Vec<String>) -> Result<IngestResult, String> {
    if file_paths.is_empty() {
        return Err("REPORT_NO_FILES: choose at least one report export".into());
    }
    let progress = |done: u64, total: u64| {
        let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Reading file", done, total });
    };
    let mut files = Vec::with_capacity(file_paths.len());
    let mut file_size = 0;
    for (n, p) in file_paths.iter().enumerate() {
        let path = PathBuf::from(p);
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !report_import::EXTENSIONS.contains(&ext.as_str()) {
            return Err(format!("REPORT_BAD_EXTENSION: {} is not a report export: choose .csv or .xlsx files", path.display()));
        }
        progress(n as u64, file_paths.len() as u64 + 1);
        let bytes = std::fs::read(&path).map_err(|e| format!("QBW_UNREADABLE: {}: {e}", path.display()))?;
        file_size += bytes.len() as i64;
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
        files.push((name, bytes));
    }
    let read = tauri::async_runtime::spawn_blocking(move || report_import::read_files(&files))
        .await
        .map_err(|e| format!("QBW_PARSE_FAILED: the files could not be read ({e})"))??;
    progress(1, 1);
    let ledger = &read.ledger;
    if ledger.transactions.is_empty()
        && ledger.unbalanced.is_empty()
        && ledger.accounts.is_empty()
        && ledger.customers.is_empty()
        && ledger.vendors.is_empty()
        && ledger.items.is_empty()
    {
        return Err("REPORT_EMPTY: these reports have no accounts, lists or transactions to import".into());
    }
    let pool = db.pool().await;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 0, total: 1 });
    let batch_id = qbw_ledger_import::stage(&pool, Path::new(&file_paths[read.main]), file_size, ledger).await?;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 1, total: 1 });
    Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await?, sources: read.sources })
}

#[tauri::command]
pub async fn list_qbw_batches(db: State<'_, Db>) -> Result<Vec<BatchListItem>, String> {
    qbw_import::list_batches(&db.pool().await).await
}

#[tauri::command]
pub async fn get_qbw_batch(db: State<'_, Db>, batch_id: i64) -> Result<BatchSummary, String> {
    qbw_import::batch_summary(&db.pool().await, batch_id).await
}

#[tauri::command]
pub async fn set_qbw_mapping(db: State<'_, Db>, batch_id: i64, raw_label: String, account_id: Option<i64>) -> Result<(), String> {
    qbw_import::set_mapping(&db.pool().await, batch_id, &raw_label, account_id).await
}

/// Creates income accounts for unmapped labels; emits `ledger-changed` so account lists refresh.
#[tauri::command]
pub async fn auto_create_qbw_accounts(app: AppHandle, db: State<'_, Db>, batch_id: i64) -> Result<i64, String> {
    let created = qbw_import::auto_create_accounts(&db.pool().await, batch_id).await?;
    notify(&app);
    Ok(created)
}

#[tauri::command]
pub async fn post_qbw_batch(app: AppHandle, db: State<'_, Db>, input: PostBatchInput) -> Result<PostBatchResult, String> {
    let result = qbw_import::post_batch(&db.pool().await, &input).await?;
    notify(&app);
    Ok(result)
}

/// Undoes a posted import by voiding everything it posted.
#[tauri::command]
pub async fn reverse_qbw_batch(app: AppHandle, db: State<'_, Db>, batch_id: i64) -> Result<ReverseBatchResult, String> {
    let result = qbw_import::reverse_batch(&db.pool().await, batch_id).await?;
    notify(&app);
    Ok(result)
}

#[tauri::command]
pub async fn discard_qbw_batch(db: State<'_, Db>, batch_id: i64) -> Result<(), String> {
    qbw_import::discard_batch(&db.pool().await, batch_id).await
}

#[tauri::command]
pub async fn list_ledger_batches(db: State<'_, Db>) -> Result<Vec<LedgerBatchSummary>, String> {
    qbw_ledger_import::list_batches(&db.pool().await).await
}

#[tauri::command]
pub async fn get_ledger_batch(db: State<'_, Db>, batch_id: i64) -> Result<LedgerBatchSummary, String> {
    qbw_ledger_import::batch_summary(&db.pool().await, batch_id).await
}

#[tauri::command]
pub async fn set_ledger_account(db: State<'_, Db>, batch_id: i64, source_id: i64, account_id: Option<i64>) -> Result<(), String> {
    qbw_ledger_import::set_account(&db.pool().await, batch_id, source_id, account_id).await
}

/// Matches or creates an account for every used source account; emits `ledger-changed`.
#[tauri::command]
pub async fn auto_map_ledger_accounts(app: AppHandle, db: State<'_, Db>, batch_id: i64) -> Result<AutoMapResult, String> {
    let result = qbw_ledger_import::auto_map_accounts(&db.pool().await, batch_id).await?;
    notify(&app);
    Ok(result)
}

/// Brings the import's customers, vendors and items into LocalBooks; emits `ledger-changed`.
#[tauri::command]
pub async fn import_ledger_lists(app: AppHandle, db: State<'_, Db>, batch_id: i64) -> Result<ImportListsResult, String> {
    let pool = db.pool().await;
    let result = qbw_ledger_import::import_lists(&pool, batch_id).await?;
    protect_imported(&pool, &db).await;
    notify(&app);
    Ok(result)
}

#[tauri::command]
pub async fn post_ledger_batch(app: AppHandle, db: State<'_, Db>, input: PostLedgerInput) -> Result<PostLedgerResult, String> {
    // Keep a copy of the books as they are now, so the import can be undone exactly ("Start over").
    let snapshot = backup::pre_import_snapshot(&db, input.batch_id).await?;
    let pool = db.pool().await;
    let posted = qbw_ledger_import::post_batch(&pool, &input).await;
    protect_imported(&pool, &db).await;
    match &posted {
        Ok(result) if result.entries_posted > 0 || result.lists.is_some() => {
            if let Err(e) = qbw_ledger_import::set_pre_post_backup(&pool, input.batch_id, &snapshot).await {
                let _ = std::fs::remove_file(&snapshot);
                notify(&app);
                return Err(e);
            }
        }
        _ => {
            let _ = std::fs::remove_file(&snapshot);
        }
    }
    notify(&app);
    posted
}

/// Puts back the copy of the company file taken just before the import posted. Everything recorded
/// since that moment is undone; the books being replaced are kept as a pre-restore backup.
#[tauri::command]
pub async fn start_over_ledger_batch(app_handle: AppHandle, batch_id: i64) -> Result<RestoreResult, String> {
    let db = app_handle.state::<Db>();
    let source = qbw_ledger_import::pre_post_backup(&db.pool().await, batch_id).await?;
    let result = backup::restore(&db, &source).await?;
    notify(&app_handle);
    Ok(result)
}

#[tauri::command]
pub async fn reverse_ledger_batch(app: AppHandle, db: State<'_, Db>, batch_id: i64) -> Result<ReverseLedgerResult, String> {
    let result = qbw_ledger_import::reverse_batch(&db.pool().await, batch_id).await?;
    notify(&app);
    Ok(result)
}

#[tauri::command]
pub async fn discard_ledger_batch(db: State<'_, Db>, batch_id: i64) -> Result<(), String> {
    qbw_ledger_import::discard_batch(&db.pool().await, batch_id).await
}
