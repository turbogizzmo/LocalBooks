//! Tauri command surface. Every successful write emits `ledger-changed` so open views refresh live.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::backup::{self, BackupFile, CompanyFileInfo, RestoreResult};
use crate::bank::{self, BankFileInspection, BankTransactionView, ColumnMapping, ImportSummary};
use crate::closing::{self, ClosingInput, ClosingStatus};
use crate::forensic::{self, AppliedCorrection, ApplyCorrectionInput, CorrectionProposal};
use crate::db::{Db, RelocateMode, StorageStatus};
use crate::storage::{self, StorageInfo};
use crate::export_engine;
use crate::preferences::{self, FiscalPeriods, TaxDeadline};
use crate::banking::{self, BankingActivity, DepositInput, TransferInput, UndepositedItem};
use crate::estimates::{self, EstimateDetail, EstimateInput, EstimateSummary};
use crate::inventory::{self, AdjustInput, InventoryItemView, StockMovementView, TrackInput};
use crate::revaluation::{self, RevaluationPreview, RevaluationView};
use crate::expenses::{self, BillInput, BillPaymentInput, BillPaymentView, BillSummary, ExpenseInput, ExpenseView};
use crate::invoices::{self, ArAging, InvoiceDetail, InvoiceInput, InvoiceSummary, PaymentInput, PaymentView};
use crate::ledger::{self, AuditEntry, JournalEntry, JournalEntryView, JournalLineView, LedgerVerification};
use crate::mcp_server::{McpService, McpStatus};
use crate::qbw_import::{self, BatchListItem, BatchSummary, PostBatchInput, PostBatchResult, ReverseBatchResult};
use crate::timesheet::{self, TimeActivityInput, TimeActivityView};
use crate::reconcile::{self, ReconcileAccountState, ReconcileProgress, Reconciliation, UnreconciledLine};
use crate::reports::{self, AccountView, Dashboard};
use crate::simplefin::{self, ConnectionView, SimpleFin, SyncSummary};
use crate::sales::{self, PartyView, ProductInput, ProductView, SalesReceiptInput, SalesReceiptSummary, SalesTransaction};
use crate::statements::{self, BalanceSheet, ProfitAndLoss};

pub const LEDGER_CHANGED: &str = "ledger-changed";

fn notify(app: &AppHandle) {
    // A failed emit only means no window is listening; the write itself already committed.
    let _ = app.emit(LEDGER_CHANGED, ());
}

#[tauri::command]
pub async fn list_accounts(db: State<'_, Db>) -> Result<Vec<AccountView>, String> {
    reports::list_accounts(&db.pool().await).await
}

#[tauri::command]
pub async fn get_dashboard(db: State<'_, Db>) -> Result<Dashboard, String> {
    reports::dashboard(&db.pool().await, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn post_journal_entry(
    app: AppHandle,
    db: State<'_, Db>,
    entry: JournalEntry,
) -> Result<i64, String> {
    let id = ledger::post_journal_entry(&db.pool().await, entry).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_invoices(db: State<'_, Db>) -> Result<Vec<InvoiceSummary>, String> {
    invoices::list_invoices(&db.pool().await).await
}

#[tauri::command]
pub async fn create_invoice(
    app: AppHandle,
    db: State<'_, Db>,
    input: InvoiceInput,
) -> Result<InvoiceSummary, String> {
    let invoice = invoices::create_and_post_invoice(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoice)
}

#[tauri::command]
pub async fn record_invoice_payment(
    app: AppHandle,
    db: State<'_, Db>,
    input: PaymentInput,
) -> Result<InvoiceSummary, String> {
    let invoice = invoices::record_payment(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoice)
}

#[tauri::command]
pub async fn list_invoice_payments(db: State<'_, Db>, invoice_id: i64) -> Result<Vec<PaymentView>, String> {
    invoices::list_payments(&db.pool().await, invoice_id).await
}

#[tauri::command]
pub async fn get_ar_aging(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<ArAging, String> {
    invoices::ar_aging(&db.pool().await, as_of.unwrap_or_else(|| Local::now().date_naive())).await
}

/// Reads a bank file (CSV, OFX, QFX) and reports how it would be imported, importing nothing.
#[tauri::command]
pub async fn inspect_bank_file(db: State<'_, Db>, path: PathBuf, mapping: Option<ColumnMapping>) -> Result<BankFileInspection, String> {
    bank::inspect_file(&db.pool().await, &path, mapping).await
}

#[tauri::command]
pub async fn import_bank_csv(
    app: AppHandle,
    db: State<'_, Db>,
    path: PathBuf,
    bank_account_id: i64,
    mapping: Option<ColumnMapping>,
) -> Result<ImportSummary, String> {
    let summary = bank::import_file(&db.pool().await, &path, bank_account_id, mapping).await?;
    notify(&app);
    Ok(summary)
}

// ------------------------------------------------------------------------------------------------
// SimpleFIN bank connections
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn simplefin_list(db: State<'_, Db>, sf: State<'_, SimpleFin>) -> Result<Vec<ConnectionView>, String> {
    sf.list(&db.pool().await).await
}

/// Claims a setup token (it works once) and lists the connection's accounts.
#[tauri::command]
pub async fn simplefin_connect(db: State<'_, Db>, sf: State<'_, SimpleFin>, token: String) -> Result<ConnectionView, String> {
    sf.connect(&db.pool().await, &token).await
}

#[tauri::command]
pub async fn simplefin_reconnect(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64, token: String) -> Result<ConnectionView, String> {
    sf.reconnect(&db.pool().await, connection_id, &token).await
}

#[tauri::command]
pub async fn simplefin_refresh_accounts(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<ConnectionView, String> {
    sf.refresh_accounts(&db.pool().await, connection_id).await
}

#[tauri::command]
pub async fn simplefin_link(db: State<'_, Db>, account_id: i64, bank_account_id: i64, sync_from: NaiveDate) -> Result<(), String> {
    simplefin::link_account(&db.pool().await, account_id, bank_account_id, sync_from, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn simplefin_unlink(db: State<'_, Db>, account_id: i64) -> Result<(), String> {
    simplefin::unlink_account(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn simplefin_suggested_start(db: State<'_, Db>, bank_account_id: i64) -> Result<NaiveDate, String> {
    simplefin::suggested_start(&db.pool().await, bank_account_id, Local::now().date_naive()).await
}

/// Fetches new posted transactions for the connection's linked accounts into Bank Feeds.
#[tauri::command]
pub async fn simplefin_sync(app: AppHandle, db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<SyncSummary, String> {
    let summary = sf.sync(&db.pool().await, connection_id, chrono::Utc::now()).await?;
    notify(&app);
    Ok(summary)
}

/// Removes the connection and the access saved on this computer; imported lines stay.
#[tauri::command]
pub async fn simplefin_disconnect(db: State<'_, Db>, sf: State<'_, SimpleFin>, connection_id: i64) -> Result<(), String> {
    sf.disconnect(&db.pool().await, connection_id).await
}

#[tauri::command]
pub async fn list_bank_transactions(db: State<'_, Db>) -> Result<Vec<BankTransactionView>, String> {
    bank::list_pending(&db.pool().await).await
}

#[tauri::command]
pub async fn approve_bank_transaction(
    app: AppHandle,
    db: State<'_, Db>,
    transaction_id: i64,
    category_account_id: i64,
) -> Result<i64, String> {
    let entry_id = bank::approve(&db.pool().await, transaction_id, category_account_id).await?;
    notify(&app);
    Ok(entry_id)
}

#[tauri::command]
pub async fn exclude_bank_transaction(
    app: AppHandle,
    db: State<'_, Db>,
    transaction_id: i64,
) -> Result<(), String> {
    bank::exclude(&db.pool().await, transaction_id).await?;
    notify(&app);
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Reconciliation
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_reconcile_state(db: State<'_, Db>, account_id: i64) -> Result<ReconcileAccountState, String> {
    reconcile::account_state(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn get_unreconciled_lines(db: State<'_, Db>, account_id: i64) -> Result<Vec<UnreconciledLine>, String> {
    reconcile::unreconciled_lines(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn list_reconciliations(db: State<'_, Db>, account_id: i64) -> Result<Vec<Reconciliation>, String> {
    reconcile::list_reconciliations(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn start_reconciliation(
    db: State<'_, Db>,
    account_id: i64,
    statement_end_date: NaiveDate,
    statement_ending_balance_cents: i64,
) -> Result<Reconciliation, String> {
    reconcile::start(&db.pool().await, account_id, statement_end_date, statement_ending_balance_cents).await
}

#[tauri::command]
pub async fn set_lines_cleared(
    db: State<'_, Db>,
    reconciliation_id: i64,
    line_ids: Vec<i64>,
    cleared: bool,
) -> Result<ReconcileProgress, String> {
    reconcile::set_lines_cleared(&db.pool().await, reconciliation_id, &line_ids, cleared).await
}

#[tauri::command]
pub async fn lock_reconciliation(
    app: AppHandle,
    db: State<'_, Db>,
    reconciliation_id: i64,
) -> Result<Reconciliation, String> {
    let rec = reconcile::lock(&db.pool().await, reconciliation_id).await?;
    notify(&app);
    Ok(rec)
}

#[tauri::command]
pub async fn cancel_reconciliation(db: State<'_, Db>, reconciliation_id: i64) -> Result<(), String> {
    reconcile::cancel(&db.pool().await, reconciliation_id).await
}

// ------------------------------------------------------------------------------------------------
// Financial statements
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_profit_and_loss(
    db: State<'_, Db>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<ProfitAndLoss, String> {
    statements::profit_and_loss(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_balance_sheet(db: State<'_, Db>, as_of: NaiveDate) -> Result<BalanceSheet, String> {
    statements::balance_sheet(&db.pool().await, as_of).await
}

// ------------------------------------------------------------------------------------------------
// Sales
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_customers(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    sales::list_customers(&db.pool().await).await
}

#[tauri::command]
pub async fn create_customer(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let customer = sales::create_customer(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(customer)
}

#[tauri::command]
pub async fn list_products(db: State<'_, Db>) -> Result<Vec<ProductView>, String> {
    sales::list_products(&db.pool().await).await
}

#[tauri::command]
pub async fn create_product(app: AppHandle, db: State<'_, Db>, input: ProductInput) -> Result<ProductView, String> {
    let product = sales::create_product(&db.pool().await, input).await?;
    notify(&app);
    Ok(product)
}

#[tauri::command]
pub async fn post_sales_receipt(
    app: AppHandle,
    db: State<'_, Db>,
    input: SalesReceiptInput,
) -> Result<SalesReceiptSummary, String> {
    let receipt = sales::post_sales_receipt(&db.pool().await, input).await?;
    notify(&app);
    Ok(receipt)
}

#[tauri::command]
pub async fn list_all_sales(db: State<'_, Db>) -> Result<Vec<SalesTransaction>, String> {
    sales::list_all_sales(&db.pool().await).await
}

// ------------------------------------------------------------------------------------------------
// Expenses
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_vendors(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    expenses::list_vendors(&db.pool().await).await
}

#[tauri::command]
pub async fn create_vendor(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let vendor = expenses::create_vendor(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(vendor)
}

#[tauri::command]
pub async fn list_bills(db: State<'_, Db>) -> Result<Vec<BillSummary>, String> {
    expenses::list_bills(&db.pool().await).await
}

#[tauri::command]
pub async fn post_vendor_bill(app: AppHandle, db: State<'_, Db>, input: BillInput) -> Result<BillSummary, String> {
    let bill = expenses::post_vendor_bill(&db.pool().await, input).await?;
    notify(&app);
    Ok(bill)
}

#[tauri::command]
pub async fn pay_vendor_bill(
    app: AppHandle,
    db: State<'_, Db>,
    input: BillPaymentInput,
) -> Result<BillSummary, String> {
    let bill = expenses::pay_vendor_bill(&db.pool().await, input).await?;
    notify(&app);
    Ok(bill)
}

#[tauri::command]
pub async fn pay_vendor_bills(app: AppHandle, db: State<'_, Db>, inputs: Vec<BillPaymentInput>) -> Result<usize, String> {
    let count = expenses::pay_vendor_bills(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn list_bill_payments(db: State<'_, Db>, bill_id: i64) -> Result<Vec<BillPaymentView>, String> {
    expenses::list_bill_payments(&db.pool().await, bill_id).await
}

#[tauri::command]
pub async fn post_expense(app: AppHandle, db: State<'_, Db>, input: ExpenseInput) -> Result<ExpenseView, String> {
    let expense = expenses::post_expense(&db.pool().await, input).await?;
    notify(&app);
    Ok(expense)
}

#[tauri::command]
pub async fn list_expenses(db: State<'_, Db>) -> Result<Vec<ExpenseView>, String> {
    expenses::list_expenses(&db.pool().await).await
}

#[tauri::command]
pub async fn record_invoice_payments(app: AppHandle, db: State<'_, Db>, inputs: Vec<PaymentInput>) -> Result<usize, String> {
    let count = invoices::record_payments(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

// ------------------------------------------------------------------------------------------------
// Voiding, journal register and audit log
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn void_transaction(app: AppHandle, db: State<'_, Db>, journal_entry_id: i64) -> Result<(), String> {
    ledger::void_transaction(&db.pool().await, journal_entry_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_journal_entries(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<JournalEntryView>, String> {
    ledger::list_journal_entries(&db.pool().await, limit.unwrap_or(500)).await
}

#[tauri::command]
pub async fn get_journal_entry_lines(db: State<'_, Db>, journal_entry_id: i64) -> Result<Vec<JournalLineView>, String> {
    ledger::journal_entry_lines(&db.pool().await, journal_entry_id).await
}

#[tauri::command]
pub async fn list_audit_log(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<AuditEntry>, String> {
    ledger::list_audit_log(&db.pool().await, limit.unwrap_or(1000)).await
}

/// Compares every posted entry with the snapshot recorded when it was posted.
#[tauri::command]
pub async fn verify_ledger_integrity(db: State<'_, Db>) -> Result<LedgerVerification, String> {
    ledger::verify_ledger_audit(&db.pool().await).await
}

/// For each entry that no longer matches its posting snapshot: the correcting entry that would
/// restore it, or why none can.
#[tauri::command]
pub async fn propose_forensic_corrections(db: State<'_, Db>) -> Result<Vec<CorrectionProposal>, String> {
    forensic::propose_corrections(&db.pool().await).await
}

/// Posts one reviewed correction (only if it still matches what was reviewed).
#[tauri::command]
pub async fn apply_forensic_correction(app: AppHandle, db: State<'_, Db>, input: ApplyCorrectionInput) -> Result<AppliedCorrection, String> {
    let applied = forensic::apply_correction(&db.pool().await, &input).await?;
    notify(&app);
    Ok(applied)
}

// ------------------------------------------------------------------------------------------------
// Deposits & transfers
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_undeposited(db: State<'_, Db>) -> Result<Vec<UndepositedItem>, String> {
    banking::list_undeposited(&db.pool().await).await
}

#[tauri::command]
pub async fn post_bank_deposit(app: AppHandle, db: State<'_, Db>, input: DepositInput) -> Result<i64, String> {
    let id = banking::post_bank_deposit(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn post_transfer(app: AppHandle, db: State<'_, Db>, input: TransferInput) -> Result<i64, String> {
    let id = banking::post_transfer(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_banking_activity(db: State<'_, Db>) -> Result<Vec<BankingActivity>, String> {
    banking::list_banking_activity(&db.pool().await).await
}

// ------------------------------------------------------------------------------------------------
// Estimates
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_estimates(db: State<'_, Db>) -> Result<Vec<EstimateSummary>, String> {
    estimates::list_estimates(&db.pool().await).await
}

#[tauri::command]
pub async fn create_estimate(app: AppHandle, db: State<'_, Db>, input: EstimateInput) -> Result<EstimateSummary, String> {
    let estimate = estimates::create_estimate(&db.pool().await, input).await?;
    notify(&app);
    Ok(estimate)
}

#[tauri::command]
pub async fn set_estimate_status(
    app: AppHandle,
    db: State<'_, Db>,
    estimate_id: i64,
    status: String,
) -> Result<EstimateSummary, String> {
    let estimate = estimates::set_estimate_status(&db.pool().await, estimate_id, &status).await?;
    notify(&app);
    Ok(estimate)
}

#[tauri::command]
pub async fn convert_estimate_to_invoice(
    app: AppHandle,
    db: State<'_, Db>,
    estimate_id: i64,
    issue_date: NaiveDate,
    due_date: NaiveDate,
) -> Result<EstimateSummary, String> {
    let estimate = estimates::convert_to_invoice(&db.pool().await, estimate_id, issue_date, due_date).await?;
    notify(&app);
    Ok(estimate)
}

// ------------------------------------------------------------------------------------------------
// Employees & time
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_employees(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    timesheet::list_employees(&db.pool().await).await
}

#[tauri::command]
pub async fn create_employee(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let employee = timesheet::create_employee(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(employee)
}

#[tauri::command]
pub async fn create_time_activities(
    app: AppHandle,
    db: State<'_, Db>,
    inputs: Vec<TimeActivityInput>,
) -> Result<usize, String> {
    let count = timesheet::create_time_activities(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn list_time_activities(
    db: State<'_, Db>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<TimeActivityView>, String> {
    timesheet::list_time_activities(&db.pool().await, from, to).await
}

// ------------------------------------------------------------------------------------------------
// Company file backup & restore
// ------------------------------------------------------------------------------------------------

/// Writes a consistent snapshot of the live company file to `destination_path` while the app keeps running.
#[tauri::command]
pub async fn export_company_file(app_handle: AppHandle, destination_path: String) -> Result<BackupFile, String> {
    let db = app_handle.state::<Db>();
    let destination = PathBuf::from(destination_path);
    backup::snapshot(&db.pool().await, &db.path().await, &destination).await?;
    backup::describe_export(&destination)
}

#[tauri::command]
pub async fn inspect_company_file(source_path: String) -> Result<CompanyFileInfo, String> {
    backup::inspect(&PathBuf::from(source_path)).await
}

/// Verifies `source_path` and replaces the live company file with it (keeping a safety snapshot).
#[tauri::command]
pub async fn import_company_file(app_handle: AppHandle, source_path: String) -> Result<RestoreResult, String> {
    let db = app_handle.state::<Db>();
    let result = backup::restore(&db, &PathBuf::from(source_path)).await?;
    notify(&app_handle);
    Ok(result)
}

#[tauri::command]
pub async fn list_backups(db: State<'_, Db>) -> Result<Vec<BackupFile>, String> {
    backup::list_backups(db.backups_dir())
}

#[tauri::command]
pub async fn backup_now(db: State<'_, Db>) -> Result<BackupFile, String> {
    backup::backup_now(&db).await
}

// ------------------------------------------------------------------------------------------------
// Company file location
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_storage_status(db: State<'_, Db>) -> Result<StorageStatus, String> {
    Ok(db.status().await)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryInspection {
    pub directory: String,
    pub target_path: String,
    pub storage: StorageInfo,
    pub has_company_file: bool,
    /// Present when a company file exists there and passed every check.
    pub company_file: Option<CompanyFileInfo>,
    /// Why the folder (or the company file in it) cannot be used.
    pub problem: Option<String>,
}

/// Looks at a candidate folder without changing anything, so the UI can explain the choice.
#[tauri::command]
pub async fn inspect_database_directory(new_path: String) -> Result<DirectoryInspection, String> {
    let dir = storage::resolve_network_path(&PathBuf::from(&new_path));
    let target = dir.join(storage::DB_FILE_NAME);
    let has_company_file = target.is_file();
    let mut problem = storage::check_writable_dir(&dir).err();
    let company_file = if has_company_file && problem.is_none() {
        match backup::inspect(&target).await {
            Ok(info) => Some(info),
            Err(err) => {
                problem = Some(err);
                None
            }
        }
    } else {
        None
    };
    Ok(DirectoryInspection {
        directory: dir.display().to_string(),
        target_path: target.display().to_string(),
        storage: storage::classify(&dir),
        has_company_file,
        company_file,
        problem,
    })
}

/// Points LocalBooks at `new_path/localbooks.db`. An existing file there is verified and opened;
/// otherwise `mode` chooses between `NEW` (initialize an empty company file, the default) and `MOVE`
/// (copy the current books there). The choice is saved to config.json.
#[tauri::command]
pub async fn update_database_directory(app: AppHandle, db: State<'_, Db>, new_path: String, mode: Option<String>) -> Result<StorageStatus, String> {
    let dir = storage::resolve_network_path(&PathBuf::from(new_path));
    let mode = RelocateMode::parse(mode.as_deref(), dir.join(storage::DB_FILE_NAME).is_file())?;
    let status = db.relocate(&dir, mode).await?;
    refresh_window_title(&app).await;
    notify(&app);
    Ok(status)
}

/// Emitted when a different company file becomes the live one; the window reloads its state.
pub const COMPANY_FILE_CHANGED: &str = "company-file-changed";

/// Titles the main window after the open company file ("Acme Ltd.localbooks — LocalBooks"), or
/// plain "LocalBooks" for the usual company file.
pub async fn refresh_window_title(app: &AppHandle) {
    let status = app.state::<Db>().status().await;
    let title = if status.opened_from_file || !status.is_default {
        format!("{} — LocalBooks", status.file_name)
    } else {
        "LocalBooks".to_string()
    };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&title);
    }
}

/// Makes `path` the live company file for this session and tells the window to reload.
pub async fn mount_company_file(app: &AppHandle, path: &Path) -> Result<StorageStatus, String> {
    let status = app.state::<Db>().mount(path).await?;
    refresh_window_title(app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    notify(app);
    Ok(status)
}

/// Carries out a request handed over by the operating system or a second launch: open the file
/// (reporting any problem in the app's banner) and bring the window forward.
pub async fn handle_hand_off(app: &AppHandle, request: crate::instance::HandOff) {
    if app.try_state::<StartupFailure>().is_some() {
        return; // no company file is open; the window is showing why
    }
    if let crate::instance::HandOff::Open { path } = request {
        if let Err(err) = mount_company_file(app, &path).await {
            app.state::<Db>().set_warning(format!("Could not open {}: {err}.", path.display()));
            let _ = app.emit(COMPANY_FILE_CHANGED, ());
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// File › Open: opens a company file (a `.localbooks` file or a LocalBooks backup) in place.
#[tauri::command]
pub async fn open_company_file(app: AppHandle, path: String) -> Result<StorageStatus, String> {
    mount_company_file(&app, &storage::resolve_network_path(&PathBuf::from(path))).await
}

// ------------------------------------------------------------------------------------------------
// Company logo & printable documents
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn set_company_logo(app: AppHandle, db: State<'_, Db>, source_path: String) -> Result<String, String> {
    let bytes = std::fs::read(&source_path).map_err(|e| format!("PREF_LOGO_READ: cannot read {source_path}: {e}"))?;
    let url = preferences::set_logo(&db.pool().await, &bytes).await?;
    notify(&app);
    Ok(url)
}

#[tauri::command]
pub async fn clear_company_logo(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    preferences::clear_logo(&db.pool().await).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_company_logo(db: State<'_, Db>) -> Result<Option<String>, String> {
    preferences::get_logo(&db.pool().await).await
}

#[tauri::command]
pub async fn get_invoice_detail(db: State<'_, Db>, invoice_id: i64) -> Result<InvoiceDetail, String> {
    invoices::invoice_detail(&db.pool().await, invoice_id).await
}

// ------------------------------------------------------------------------------------------------
// Preferences & tax calendar
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_preferences(db: State<'_, Db>) -> Result<std::collections::BTreeMap<String, String>, String> {
    preferences::get_all(&db.pool().await).await
}

#[tauri::command]
pub async fn set_preference(app: AppHandle, db: State<'_, Db>, key: String, value: String) -> Result<String, String> {
    let stored = preferences::set(&db.pool().await, &key, &value).await?;
    notify(&app);
    Ok(stored)
}

#[tauri::command]
pub async fn list_tax_forms() -> Result<Vec<(String, String)>, String> {
    Ok(preferences::TAX_FORMS.iter().map(|(c, l)| (c.to_string(), l.to_string())).collect())
}

#[tauri::command]
pub async fn get_fiscal_periods(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<FiscalPeriods, String> {
    let month = preferences::fiscal_start_month(&db.pool().await).await?;
    Ok(preferences::fiscal_periods(as_of.unwrap_or_else(|| Local::now().date_naive()), month))
}

#[tauri::command]
pub async fn get_tax_calendar(db: State<'_, Db>, as_of: Option<NaiveDate>) -> Result<Vec<TaxDeadline>, String> {
    let pool = db.pool().await;
    let month = preferences::fiscal_start_month(&pool).await?;
    let form = preferences::get(&pool, "tax_form").await?;
    Ok(preferences::tax_calendar(&form, month, as_of.unwrap_or_else(|| Local::now().date_naive())))
}

// ------------------------------------------------------------------------------------------------
// Report export
// ------------------------------------------------------------------------------------------------

/// Renders a financial statement as CSV (`profit_and_loss` needs start/end, `balance_sheet` needs as_of).
#[tauri::command]
pub async fn report_csv(
    db: State<'_, Db>,
    report: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
) -> Result<String, String> {
    export_engine::render(&db.pool().await, &report, start_date, end_date, as_of).await
}

/// Prompts with the native Save dialog and writes `report_html_or_matrix` to the chosen file.
/// Returns the saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_report_to_disk(app: AppHandle, report_html_or_matrix: String, filename: String) -> Result<Option<String>, String> {
    let extension = std::path::Path::new(&filename).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "csv".into());
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_file_name(&filename)
            .add_filter(extension.to_uppercase(), &[extension.as_str()])
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("EXPORT_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, report_html_or_matrix.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

/// Opens the system print dialog for the current window (Print / Save as PDF).
#[tauri::command]
pub async fn print_report(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| format!("PRINT_FAILED: {e}"))
}

// ------------------------------------------------------------------------------------------------
// Desktop Migration Wizard: .qbw import (staging sandbox)
// ------------------------------------------------------------------------------------------------

pub const QBW_PROGRESS: &str = "qbw-import-progress";

/// Reads a .qbw company file into a new staging batch, reporting progress as
/// `qbw-import-progress` events. Nothing is posted to the ledger.
#[tauri::command]
pub async fn ingest_raw_qbw(app: AppHandle, db: State<'_, Db>, file_path: String) -> Result<BatchSummary, String> {
    let path = PathBuf::from(&file_path);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !qbw_import::ACCEPTED_EXTENSIONS.contains(&ext.as_str()) {
        return Err("QBW_BAD_EXTENSION: choose a .qbw company file".into());
    }
    let emitter = app.clone();
    let source = path.clone();
    // A panic inside the parser surfaces here as a join error instead of taking the app down.
    let extraction = tauri::async_runtime::spawn_blocking(move || {
        qbw_import::extract(&source, &mut |p| {
            let _ = emitter.emit(QBW_PROGRESS, p);
        })
    })
    .await
    .map_err(|e| format!("QBW_PARSE_FAILED: the .qbw file could not be read ({e})"))??;

    let staged = (extraction.lines.len() as u64).max(1);
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: 0, total: staged });
    let pool = db.pool().await;
    let batch_id = qbw_import::stage(&pool, &path, &extraction).await?;
    let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage: "Staging rows", done: staged, total: staged });
    qbw_import::batch_summary(&pool, batch_id).await
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

// ------------------------------------------------------------------------------------------------
// Local MCP server (read-only AI access)
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_mcp_status(mcp: State<'_, McpService>) -> Result<McpStatus, String> {
    Ok(mcp.status().await)
}

#[tauri::command]
pub async fn set_mcp_enabled(mcp: State<'_, McpService>, enabled: bool) -> Result<McpStatus, String> {
    mcp.set_enabled(enabled).await
}

#[tauri::command]
pub async fn regenerate_mcp_token(mcp: State<'_, McpService>) -> Result<McpStatus, String> {
    mcp.regenerate_token().await
}

// ------------------------------------------------------------------------------------------------
// Closing date
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_closing_status(db: State<'_, Db>) -> Result<ClosingStatus, String> {
    closing::status(&db.pool().await).await
}

#[tauri::command]
pub async fn set_closing_date(app: AppHandle, db: State<'_, Db>, input: ClosingInput) -> Result<ClosingStatus, String> {
    let status = closing::set_closing_date(&db.pool().await, &input).await?;
    notify(&app);
    Ok(status)
}

// ------------------------------------------------------------------------------------------------
// Startup failure
// ------------------------------------------------------------------------------------------------

/// Set when the company file could not be opened at launch. The window then shows this instead
/// of the app, and no command that needs the books is available.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupFailure {
    pub message: String,
    pub company_file: PathBuf,
}

/// Appends the failure to `startup-error.log` in the app data folder, for support.
pub fn record_startup_failure(data_dir: &Path, message: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir.join("startup-error.log")) {
        let _ = writeln!(log, "{} LocalBooks {}: {message}", Local::now().format("%Y-%m-%d %H:%M:%S %z"), env!("CARGO_PKG_VERSION"));
    }
}

#[tauri::command]
pub fn get_startup_failure(app: AppHandle) -> Option<StartupFailure> {
    app.try_state::<StartupFailure>().map(|s| s.inner().clone())
}

// ------------------------------------------------------------------------------------------------
// Inventory
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_inventory_items(db: State<'_, Db>) -> Result<Vec<InventoryItemView>, String> {
    inventory::list_items(&db.pool().await).await
}

#[tauri::command]
pub async fn track_inventory(app: AppHandle, db: State<'_, Db>, input: TrackInput) -> Result<InventoryItemView, String> {
    let item = inventory::track_product(&db.pool().await, &input).await?;
    notify(&app);
    Ok(item)
}

#[tauri::command]
pub async fn adjust_inventory(app: AppHandle, db: State<'_, Db>, input: AdjustInput) -> Result<InventoryItemView, String> {
    let item = inventory::adjust(&db.pool().await, &input).await?;
    notify(&app);
    Ok(item)
}

#[tauri::command]
pub async fn list_stock_movements(db: State<'_, Db>, item_id: i64) -> Result<Vec<StockMovementView>, String> {
    inventory::movements(&db.pool().await, item_id).await
}

// ------------------------------------------------------------------------------------------------
// Foreign currency revaluation
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn preview_fx_revaluation(db: State<'_, Db>, as_of: NaiveDate, rates: BTreeMap<String, i64>) -> Result<RevaluationPreview, String> {
    revaluation::preview(&db.pool().await, as_of, &rates).await
}

#[tauri::command]
pub async fn post_fx_revaluation(app: AppHandle, db: State<'_, Db>, as_of: NaiveDate, rates: BTreeMap<String, i64>) -> Result<RevaluationView, String> {
    let posted = revaluation::post(&db.pool().await, as_of, &rates).await?;
    notify(&app);
    Ok(posted)
}

#[tauri::command]
pub async fn list_fx_revaluations(db: State<'_, Db>) -> Result<Vec<RevaluationView>, String> {
    revaluation::list(&db.pool().await).await
}

#[tauri::command]
pub async fn void_fx_revaluation(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    revaluation::void(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Estimates → invoice form
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_estimate_detail(db: State<'_, Db>, estimate_id: i64) -> Result<EstimateDetail, String> {
    estimates::estimate_detail(&db.pool().await, estimate_id).await
}

/// Posts an invoice started from an estimate (possibly edited) and marks the estimate converted.
#[tauri::command]
pub async fn invoice_from_estimate(app: AppHandle, db: State<'_, Db>, estimate_id: i64, input: InvoiceInput) -> Result<InvoiceSummary, String> {
    let invoice = estimates::invoice_from_estimate(&db.pool().await, estimate_id, input).await?;
    notify(&app);
    Ok(invoice)
}
