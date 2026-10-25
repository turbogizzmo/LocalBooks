//! Tauri command surface. Every successful write emits `ledger-changed` so open views refresh live.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::accounts::{self, AccountInput, AccountTypeView};
use crate::backup::{self, BackupFile, CompanyFileInfo, RestoreResult};
use crate::bank::{self, BankFileInspection, BankTransactionView, ColumnMapping, ImportSummary};
use crate::classes::{self, ClassInput, ClassView, ReclassLine, ReclassQuery};
use crate::closing::{self, ClosingInput, ClosingStatus};
use crate::credits::{self, ApplyCreditInput, CreditApplicationView, CreditMemoInput, CreditMemoSummary, RefundCreditInput, VendorCreditInput, VendorCreditSummary};
use crate::parties::{self, PartyDetails, PartyDetailsInput};
use crate::registers::{self, Register};
use crate::edits::{self, DocumentLineView};
use crate::workflows::{self, AttachmentView, CustomerStatement, PurchaseOrderInput, PurchaseOrderLineView, PurchaseOrderView, RecurringInput, RecurringView, SearchHit, Vendor1099Row};
use crate::sales_tax::{self, SalesTaxPaymentInput, SalesTaxPaymentView, TaxLiabilityReport, TaxRateInput, TaxRateView};
use crate::forms::{self, EmailDraft, FormKind, FormStyle, FormTemplate, Period, RenderedForm};
use crate::email_log::{self, EmailStatus, QueuedForm, SentRecord};
use crate::mail_app::{self, ComposeOutcome};
use crate::secrets::SecretStore;
use crate::windows;
use crate::smtp::{self, SmtpSettings, SmtpStatus};
use crate::file_dentist::{self, CheckupReport, RebuildResult};
use crate::forensic::{self, AppliedCorrection, ApplyCorrectionInput, CorrectionProposal};
use crate::{iif_import, qif_import};
use crate::db::{Db, RelocateMode, StorageStatus, NO_COMPANY_OPEN};
use crate::tax_ids;
use crate::users::{self, NewUserInput, Session, UserUpdate, UserView};
use crate::storage::{self, RecentCompany, RecentCompanyView, StorageInfo};
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
use crate::qbw_ledger_import::{self, AutoMapResult, ImportListsResult, LedgerBatchSummary, PostLedgerInput, PostLedgerResult, ReverseLedgerResult};
use crate::{qbw_backup, qbw_company};
use crate::timesheet::{self, TimeActivityInput, TimeActivityView};
use crate::reconcile::{self, ReconcileAccountState, ReconcileProgress, Reconciliation, UnreconciledLine};
use crate::reports::{self, AccountView, Dashboard};
use crate::simplefin::{self, ConnectionView, SimpleFin, SyncSummary};
use crate::sales::{self, PartyView, ProductInput, ProductView, SalesReceiptInput, SalesReceiptSummary, SalesTransaction};
use crate::statements::{self, BalanceSheet, Basis, BudgetAmount, BudgetView, CashFlowStatement, ClassFilter, DatedTrialBalance, PeriodReportInput, ProfitAndLoss, ProfitAndLossByClass};
use crate::report_center::{self, AgingReport, GeneralLedger, SummaryReport};

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
pub async fn list_account_types(db: State<'_, Db>) -> Result<Vec<AccountTypeView>, String> {
    accounts::list_account_types(&db.pool().await).await
}

#[tauri::command]
pub async fn create_account(app: AppHandle, db: State<'_, Db>, input: AccountInput) -> Result<AccountView, String> {
    let account = accounts::create_account(&db.pool().await, input).await?;
    notify(&app);
    Ok(account)
}

#[tauri::command]
pub async fn update_account(app: AppHandle, db: State<'_, Db>, id: i64, input: AccountInput) -> Result<AccountView, String> {
    let account = accounts::update_account(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(account)
}

#[tauri::command]
pub async fn set_account_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<AccountView, String> {
    let account = accounts::set_account_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(account)
}

#[tauri::command]
pub async fn list_classes(db: State<'_, Db>) -> Result<Vec<ClassView>, String> {
    classes::list_classes(&db.pool().await).await
}

#[tauri::command]
pub async fn create_class(app: AppHandle, db: State<'_, Db>, input: ClassInput) -> Result<ClassView, String> {
    let class = classes::create_class(&db.pool().await, input).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn update_class(app: AppHandle, db: State<'_, Db>, id: i64, input: ClassInput) -> Result<ClassView, String> {
    let class = classes::update_class(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn set_class_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ClassView, String> {
    let class = classes::set_class_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn list_reclass_lines(db: State<'_, Db>, query: ReclassQuery) -> Result<Vec<ReclassLine>, String> {
    classes::reclass_candidates(&db.pool().await, &query).await
}

#[tauri::command]
pub async fn reclassify_lines(app: AppHandle, db: State<'_, Db>, line_ids: Vec<i64>, class_id: Option<i64>) -> Result<usize, String> {
    let moved = classes::reclassify(&db.pool().await, &line_ids, class_id).await?;
    notify(&app);
    Ok(moved)
}

#[tauri::command]
pub async fn delete_class(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    classes::delete_class(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
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
    class_id: Option<i64>,
) -> Result<i64, String> {
    let entry_id = bank::approve(&db.pool().await, transaction_id, category_account_id, class_id).await?;
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
    class_filter: Option<ClassFilter>,
    basis: Option<Basis>,
) -> Result<ProfitAndLoss, String> {
    statements::profit_and_loss_basis(&db.pool().await, start_date, end_date, class_filter.as_ref(), basis.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_profit_and_loss_periods(db: State<'_, Db>, input: PeriodReportInput) -> Result<ProfitAndLossByClass, String> {
    statements::profit_and_loss_periods(&db.pool().await, &input).await
}

#[tauri::command]
pub async fn get_trial_balance_as_of(db: State<'_, Db>, as_of: NaiveDate) -> Result<DatedTrialBalance, String> {
    statements::trial_balance_as_of(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn get_cash_flow(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<CashFlowStatement, String> {
    statements::cash_flow(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_general_ledger(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<GeneralLedger, String> {
    report_center::general_ledger(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_aging(db: State<'_, Db>, payable: bool, as_of: NaiveDate) -> Result<AgingReport, String> {
    report_center::aging(&db.pool().await, payable, as_of).await
}

/// `customer` (sales by customer), `vendor` (expenses by vendor) or `product` (sales by product/service).
#[tauri::command]
pub async fn get_summary_report(db: State<'_, Db>, by: String, start_date: NaiveDate, end_date: NaiveDate) -> Result<SummaryReport, String> {
    let pool = db.pool().await;
    match by.as_str() {
        "customer" => report_center::by_party(&pool, false, start_date, end_date).await,
        "vendor" => report_center::by_party(&pool, true, start_date, end_date).await,
        "product" => report_center::sales_by_product(&pool, start_date, end_date).await,
        other => Err(format!("RPT_UNKNOWN: '{other}' is not a summary report")),
    }
}

#[tauri::command]
pub async fn get_profit_and_loss_by_class(
    db: State<'_, Db>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<ProfitAndLossByClass, String> {
    statements::profit_and_loss_by_class(&db.pool().await, start_date, end_date).await
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

/// File Dentist check-up of the live company file (read-only).
#[tauri::command]
pub async fn file_dentist_checkup(db: State<'_, Db>) -> Result<CheckupReport, String> {
    file_dentist::checkup_live(&db).await
}

/// File Dentist rebuild: snapshot, restore this build's safeguards and indexes, compact, re-check.
#[tauri::command]
pub async fn file_dentist_rebuild(app: AppHandle, db: State<'_, Db>) -> Result<RebuildResult, String> {
    let result = file_dentist::rebuild_live(&db).await?;
    notify(&app);
    Ok(result)
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
    backup::snapshot(&db.pool().await, &db.path().await?, &destination).await?;
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
    company_changed(&app_handle).await;
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
/// The open company file, or `None` when no company is open (the No Company Open screen).
pub async fn get_storage_status(db: State<'_, Db>) -> Result<Option<StorageStatus>, String> {
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
    company_changed(&app).await;
    notify(&app);
    Ok(status)
}

/// Emitted when a different company file becomes the live one; the window reloads its state.
pub const COMPANY_FILE_CHANGED: &str = "company-file-changed";

/// After a different company file becomes the live one (or none does): retitles every window,
/// remembers the file at the top of the recent list (and whether the company was left closed, for
/// the next launch), and refreshes File › Open Previous Company.
pub async fn company_changed(app: &AppHandle) {
    refresh_window_title(app).await;
    let Some(db) = app.try_state::<Db>() else { return };
    let status = db.status().await;
    let updated = storage::update_config(db.config_dir(), |config| {
        config.start_with_no_company = status.is_none();
        if let Some(status) = &status {
            storage::add_recent(&mut config.recent_companies, RecentCompany::opened_now(Path::new(&status.path), status.company_name.clone()));
        }
    });
    if let Err(err) = updated {
        eprintln!("cannot update the recent company list: {err}");
    }
    crate::file_menu::refresh(app, &recent_companies(&db), status.is_some());
}

fn recent_companies(db: &Db) -> Vec<RecentCompanyView> {
    storage::describe_recent(&storage::read_config(db.config_dir()).recent_companies)
}

/// Titles every LocalBooks window after the open company ("Acme Ltd — LocalBooks"), or plain
/// "LocalBooks" for the usual company file before it is named.
async fn refresh_window_title(app: &AppHandle) {
    let title = window_title(app).await;
    for window in app.webview_windows().values() {
        let _ = window.set_title(&title);
    }
}

/// The title every LocalBooks window shows for the open company.
pub async fn window_title(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<Db>() else { return "LocalBooks".to_string() };
    match db.status().await {
        None => "No Company Open — LocalBooks".to_string(),
        Some(StorageStatus { company_name: Some(name), .. }) => format!("{name} — LocalBooks"),
        Some(_) => "LocalBooks".to_string(),
    }
}

/// Window › New Window, or a register or report opened in its own window: another window on the
/// open company, starting on `view` (a navigation id) with the given request, if any.
#[tauri::command]
pub async fn open_window(app: AppHandle, view: Option<String>, action: Option<String>, data: Option<serde_json::Value>) -> Result<String, String> {
    if !app.state::<Db>().is_open().await {
        return Err(NO_COMPANY_OPEN.into());
    }
    let title = window_title(&app).await;
    windows::open(&app, &title, view.as_deref(), action.as_deref(), data.as_ref())
}

/// Makes `path` the live company file for this session and tells the window to reload.
pub async fn mount_company_file(app: &AppHandle, path: &Path) -> Result<StorageStatus, String> {
    let status = app.state::<Db>().mount(path).await?;
    company_changed(app).await;
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
    windows::focus(app);
}

/// File › Open: opens a company file (a `.localbooks` file or a LocalBooks backup) in place.
#[tauri::command]
pub async fn open_company_file(app: AppHandle, path: String) -> Result<StorageStatus, String> {
    mount_company_file(&app, &storage::resolve_network_path(&PathBuf::from(path))).await
}

// ------------------------------------------------------------------------------------------------
// Company files: No Company Open screen, New / Close Company, recent list
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScreen {
    pub recent: Vec<RecentCompanyView>,
    /// Why the last file asked for could not be opened, if it could not.
    pub warning: Option<String>,
}

/// What the No Company Open screen lists.
#[tauri::command]
pub async fn get_start_screen(db: State<'_, Db>) -> Result<StartScreen, String> {
    Ok(StartScreen { recent: recent_companies(&db), warning: db.warning() })
}

/// Edit List: takes a company file off the recent list. The file itself is left alone.
#[tauri::command]
pub async fn forget_recent_company(app: AppHandle, db: State<'_, Db>, path: String) -> Result<Vec<RecentCompanyView>, String> {
    storage::update_config(db.config_dir(), |config| {
        storage::remove_recent(&mut config.recent_companies, Path::new(&path));
    })?;
    let recent = recent_companies(&db);
    crate::file_menu::refresh(&app, &recent, db.is_open().await);
    Ok(recent)
}

/// Find a Company File: searches this computer's usual places for company files: the home folder
/// (Documents, Desktop, Downloads, …), iCloud Drive and other synced folders, and on macOS any
/// connected drive under /Volumes. Bounded in depth, count and time.
#[tauri::command]
pub async fn find_company_files(app: AppHandle) -> Result<Vec<storage::FoundCompanyFile>, String> {
    let home = app.path().home_dir().map_err(|e| format!("FIND_NO_HOME: {e}"))?;
    let mut roots = vec![home.join("Library").join("Mobile Documents").join("com~apple~CloudDocs"), home.join("Library").join("CloudStorage"), home];
    if cfg!(target_os = "macos") {
        if let Ok(volumes) = std::fs::read_dir("/Volumes") {
            roots.extend(volumes.flatten().map(|v| v.path()).filter(|p| p.is_dir() && !p.is_symlink()));
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    tauri::async_runtime::spawn_blocking(move || storage::find_company_files(&roots, 8, 500, deadline))
        .await
        .map_err(|e| format!("FIND_FAILED: {e}"))
}

/// `path` as a company file name: `.localbooks` is added when no extension was typed.
fn with_company_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension(storage::COMPANY_FILE_EXTENSION)
    } else {
        path
    }
}

/// File › New Company: creates an empty company file named `company_name` at `path` and opens it.
#[tauri::command]
pub async fn create_company_file(app: AppHandle, path: String, company_name: String) -> Result<StorageStatus, String> {
    let path = with_company_extension(PathBuf::from(path));
    let status = app.state::<Db>().create(&path, &company_name).await?;
    company_changed(&app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    notify(&app);
    Ok(status)
}

/// Open or Restore Company with a LocalBooks backup: copies the backup to `destination_path` (a new
/// company file) and opens the copy, leaving the backup itself unchanged.
#[tauri::command]
pub async fn restore_backup_as(app: AppHandle, source_path: String, destination_path: String) -> Result<StorageStatus, String> {
    let source = PathBuf::from(source_path);
    let destination = with_company_extension(storage::resolve_network_path(&PathBuf::from(destination_path)));
    backup::inspect(&source).await?;
    if destination.exists() {
        return Err(format!("NEW_COMPANY_EXISTS: {} already exists; choose another name or folder (LocalBooks never replaces a company file)", destination.display()));
    }
    std::fs::copy(&source, &destination).map_err(|e| format!("RESTORE_COPY: cannot copy the backup to {}: {e}", destination.display()))?;
    match mount_company_file(&app, &destination).await {
        Ok(status) => Ok(status),
        Err(err) => {
            let _ = std::fs::remove_file(&destination);
            Err(err)
        }
    }
}

/// File › Close Company: closes the other windows on the company, then the company file itself
/// (releasing its lock), and shows the No Company Open screen.
pub async fn close_company(app: &AppHandle) -> Result<(), String> {
    let db = app.try_state::<Db>().ok_or_else(|| NO_COMPANY_OPEN.to_string())?;
    windows::close_extra(app);
    db.close().await;
    company_changed(app).await;
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub async fn close_company_file(app: AppHandle) -> Result<(), String> {
    close_company(&app).await
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
// Form designer & PDF forms
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_form_styles(db: State<'_, Db>) -> Result<Vec<FormStyle>, String> {
    forms::list_styles(&db.pool().await).await
}

/// A new form style, optionally starting as a copy of another.
#[tauri::command]
pub async fn create_form_style(db: State<'_, Db>, name: String, copy_from: Option<i64>) -> Result<FormStyle, String> {
    forms::create_style(&db.pool().await, &name, copy_from).await
}

#[tauri::command]
pub async fn rename_form_style(db: State<'_, Db>, id: i64, name: String) -> Result<FormStyle, String> {
    forms::rename_style(&db.pool().await, id, &name).await
}

#[tauri::command]
pub async fn set_default_form_style(db: State<'_, Db>, id: i64) -> Result<(), String> {
    forms::set_default_style(&db.pool().await, id).await
}

#[tauri::command]
pub async fn delete_form_style(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    forms::delete_style(&db.pool().await, id).await?;
    // Customers who used the style go back to the default.
    notify(&app);
    Ok(())
}

/// A style's template for a form type (`style_id` None = the default style).
#[tauri::command]
pub async fn get_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    forms::get_template(&db.pool().await, style_id, kind).await
}

#[tauri::command]
pub async fn save_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind, template: FormTemplate) -> Result<FormTemplate, String> {
    forms::save_template(&db.pool().await, style_id, kind, template).await
}

#[tauri::command]
pub async fn reset_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    forms::reset_template(&db.pool().await, style_id, kind).await
}

/// Lays out a form with an unsaved template: a real document, or the sample without one. For a
/// statement, `document_id` is the customer and `period` the statement period.
#[tauri::command]
pub async fn preview_form(db: State<'_, Db>, kind: FormKind, template: FormTemplate, document_id: Option<i64>, period: Option<Period>) -> Result<RenderedForm, String> {
    Ok(forms::render(&db.pool().await, kind, document_id, period, Some(template), false).await?.form)
}

/// Lays out a document with its customer's form style, for printing.
#[tauri::command]
pub async fn render_form(db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<RenderedForm, String> {
    Ok(forms::render(&db.pool().await, kind, Some(document_id), period, None, false).await?.form)
}

/// Prompts with the native Save dialog and writes the document's PDF there. Returns the saved
/// path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_form_pdf(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<Option<String>, String> {
    let (file_name, bytes) = forms::pdf_file_for(&db.pool().await, kind, document_id, period).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&file_name).add_filter("PDF", &["pdf"]).blocking_save_file())
        .await
        .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("EXPORT_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// The email for a document: the customer's address and the form's subject and message.
#[tauri::command]
pub async fn get_email_draft(db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<EmailDraft, String> {
    forms::email_draft(&db.pool().await, kind, document_id, period).await
}

/// The message the user confirmed in the email dialog.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailInput {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl EmailInput {
    fn checked(self) -> Result<(Vec<String>, String, String), String> {
        let to = mail_app::parse_recipients(&self.to)?;
        let subject = self.subject.split_whitespace().collect::<Vec<_>>().join(" ");
        if subject.chars().count() > 200 {
            return Err("MAIL_SUBJECT_TOO_LONG: keep the subject under 200 characters".into());
        }
        if self.body.chars().count() > 20_000 {
            return Err("MAIL_BODY_TOO_LONG: keep the message under 20,000 characters".into());
        }
        Ok((to, subject, self.body))
    }
}

/// Opens a new message in the user's mail app with the document's PDF attached.
#[tauri::command]
pub async fn email_form_with_mail_app(
    app: AppHandle,
    db: State<'_, Db>,
    kind: FormKind,
    document_id: i64,
    period: Option<Period>,
    email: EmailInput,
) -> Result<ComposeOutcome, String> {
    let (to, subject, body) = email.checked()?;
    let pool = db.pool().await;
    let (file_name, bytes) = forms::pdf_file_for(&pool, kind, document_id, period).await?;
    let outgoing = app.path().app_cache_dir().map_err(|e| format!("MAIL_STAGE: {e}"))?.join("outgoing");
    let message = mail_app::Compose { to: to.clone(), subject: subject.clone(), body, attachment: Default::default() };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let attachment = mail_app::stage_attachment(&outgoing, &file_name, &bytes)?;
        mail_app::compose(&mail_app::Compose { attachment, ..message })
    })
    .await
    .map_err(|e| format!("MAIL_OPEN: {e}"))??;
    record_sent(&app, &pool, kind, document_id, &to, email_log::Method::MailApp, &subject).await;
    Ok(outcome)
}

/// Adds the send to the form's history. The message has already gone out (or is open in the mail
/// app), so a failure here is logged rather than reported as a failed send.
async fn record_sent(app: &AppHandle, pool: &sqlx::SqlitePool, kind: FormKind, document_id: i64, to: &[String], method: email_log::Method, subject: &str) {
    match email_log::record(pool, kind, document_id, to, method, subject).await {
        Ok(()) => notify(app),
        Err(err) => eprintln!("could not record the emailed form in its history: {err}"),
    }
}

#[tauri::command]
pub async fn get_email_history(db: State<'_, Db>, kind: FormKind, document_id: i64) -> Result<Vec<SentRecord>, String> {
    email_log::history(&db.pool().await, kind, document_id).await
}

/// The forms of a type that were emailed or are marked Email later.
#[tauri::command]
pub async fn list_email_statuses(db: State<'_, Db>, kind: FormKind) -> Result<Vec<EmailStatus>, String> {
    email_log::statuses(&db.pool().await, kind).await
}

#[tauri::command]
pub async fn set_email_later(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, queued: bool) -> Result<(), String> {
    email_log::set_queued(&db.pool().await, kind, document_id, queued).await?;
    notify(&app);
    Ok(())
}

/// The "Email later" queue, for Send Forms.
#[tauri::command]
pub async fn list_email_queue(db: State<'_, Db>) -> Result<Vec<QueuedForm>, String> {
    email_log::queue(&db.pool().await).await
}

/// Sends the document's PDF to the customer through the company's email server.
#[tauri::command]
pub async fn email_form_with_smtp(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>, email: EmailInput) -> Result<(), String> {
    let (to, subject, body) = email.checked()?;
    let pool = db.pool().await;
    let (file_name, pdf) = forms::pdf_file_for(&pool, kind, document_id, period).await?;
    smtp::send(&pool, &smtp_secrets(&app)?, smtp::Outgoing { to: to.clone(), subject: subject.clone(), body, file_name, pdf }).await?;
    record_sent(&app, &pool, kind, document_id, &to, email_log::Method::Smtp, &subject).await;
    Ok(())
}

/// Sends a short test message to the from address with the settings as entered.
#[tauri::command]
pub async fn send_smtp_test_email(app: AppHandle, db: State<'_, Db>, settings: SmtpSettings, password: Option<String>) -> Result<String, String> {
    let company = forms::load_company(&db.pool().await).await?;
    smtp::send_test(settings, &smtp_secrets(&app)?, password, &company.name).await
}

fn smtp_secrets(app: &AppHandle) -> Result<SecretStore, String> {
    Ok(smtp::secret_store(&app.path().app_config_dir().map_err(|e| format!("SMTP_CONFIG: {e}"))?))
}

#[tauri::command]
pub async fn get_smtp_settings(app: AppHandle, db: State<'_, Db>) -> Result<SmtpStatus, String> {
    smtp::status(&db.pool().await, &smtp_secrets(&app)?).await
}

/// Saves the email server settings; `password` replaces the saved one (empty removes it).
#[tauri::command]
pub async fn save_smtp_settings(app: AppHandle, db: State<'_, Db>, settings: SmtpSettings, password: Option<String>) -> Result<SmtpStatus, String> {
    smtp::save_settings(&db.pool().await, &smtp_secrets(&app)?, settings, password).await
}

/// Connects and signs in with the settings as entered, without saving or sending anything.
#[tauri::command]
pub async fn test_smtp_settings(app: AppHandle, settings: SmtpSettings, password: Option<String>) -> Result<(), String> {
    smtp::test_connection(settings, &smtp_secrets(&app)?, password).await
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
    if key == "company_name" {
        company_changed(&app).await;
    }
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
    class_filter: Option<ClassFilter>,
) -> Result<String, String> {
    export_engine::render(&db.pool().await, &report, start_date, end_date, as_of, class_filter.as_ref()).await
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

/// What a company file was read into: invoice lines (files openqbw reads) or the full ledger
/// (newer files, read by the record-layer decoder).
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum IngestResult {
    Invoices { batch: BatchSummary },
    Ledger { batch: LedgerBatchSummary },
}

/// Reads a .qbw company file, the company file inside a .qbb backup, or an .iif (QuickBooks) or
/// .qif (Quicken) export into a new staging batch, reporting progress as `qbw-import-progress`
/// events. Nothing is posted to the ledger.
///
/// The invoice-line reader runs first, so files it supports import as before; when it finds
/// nothing (newer file versions), the record-layer decoder reads the whole ledger instead.
#[tauri::command]
pub async fn ingest_raw_qbw(app: AppHandle, db: State<'_, Db>, file_path: String) -> Result<IngestResult, String> {
    let path = PathBuf::from(&file_path);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !qbw_import::ACCEPTED_EXTENSIONS.contains(&ext.as_str()) && !matches!(ext.as_str(), "iif" | "qif") {
        return Err("QBW_BAD_EXTENSION: choose a .qbw company file, a .qbb backup, or an .iif or .qif export".into());
    }
    let file_size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
    let progress = |stage: &'static str, done: u64, total: u64| {
        let _ = app.emit(QBW_PROGRESS, qbw_import::Progress { stage, done, total });
    };

    // IIF and QIF exports are text: read straight into a ledger and stage it like a company file.
    if matches!(ext.as_str(), "iif" | "qif") {
        progress("Reading file", 0, 1);
        let bytes = std::fs::read(&path).map_err(|e| format!("QBW_UNREADABLE: {}: {e}", path.display()))?;
        let account = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported account".into());
        let is_iif = ext == "iif";
        let ledger = tauri::async_runtime::spawn_blocking(move || if is_iif { iif_import::read(&bytes) } else { qif_import::read(&bytes, &account) })
            .await
            .map_err(|e| format!("QBW_PARSE_FAILED: the file could not be read ({e})"))??;
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
        return Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await? });
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
            Ok(IngestResult::Ledger { batch: qbw_ledger_import::batch_summary(&pool, batch_id).await? })
        }
        Err(err) => Err(err),
    }
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

// ------------------------------------------------------------------------------------------------
// Registers, list records and credits
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_account_register(
    db: State<'_, Db>,
    account_id: i64,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    include_subaccounts: Option<bool>,
) -> Result<Register, String> {
    registers::account_register(&db.pool().await, account_id, start_date, end_date, include_subaccounts.unwrap_or(false)).await
}

/// The open file's pool and its sign-in, held for the length of a command that uses the data key.
async fn signed_in(db: &Db) -> Result<(sqlx::SqlitePool, tokio::sync::OwnedMutexGuard<Option<Session>>), String> {
    let (pool, slot) = db.pool_and_session().await?;
    Ok((pool, slot.lock_owned().await))
}

/// What the signed-in user may do with sensitive data in the open file.
async fn sensitive_access<'a>(pool: &sqlx::SqlitePool, session: &'a Option<Session>) -> Result<tax_ids::Access<'a>, String> {
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    tax_ids::Access::of(&mut conn, session.as_ref().and_then(|s| s.key.as_ref())).await
}

/// After an import: encrypts the tax IDs it wrote in plain, when the signed-in user holds the key
/// (otherwise they are encrypted at the next sign-in of someone who does).
async fn protect_imported(pool: &sqlx::SqlitePool, db: &Db) {
    let Ok((_, session)) = signed_in(db).await else { return };
    let Some(key) = session.as_ref().and_then(|s| s.key.as_ref()) else { return };
    let protected = async { tax_ids::protect_all(&mut *pool.acquire().await.map_err(ledger::db_err)?, key).await };
    if let Err(err) = protected.await {
        eprintln!("LocalBooks: could not encrypt imported tax IDs: {err}");
    }
}

#[tauri::command]
pub async fn get_party(db: State<'_, Db>, kind: String, id: i64) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    parties::get_party(&pool, &kind, id, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn update_party(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, input: PartyDetailsInput) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    let party = parties::update_party(&pool, &kind, id, input, sensitive_access(&pool, &session).await?).await?;
    notify(&app);
    Ok(party)
}

#[tauri::command]
pub async fn set_party_active(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, active: bool) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    let party = parties::set_party_active(&pool, &kind, id, active, sensitive_access(&pool, &session).await?).await?;
    notify(&app);
    Ok(party)
}

#[tauri::command]
pub async fn update_product(app: AppHandle, db: State<'_, Db>, id: i64, input: ProductInput) -> Result<ProductView, String> {
    let product = parties::update_product(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(product)
}

#[tauri::command]
pub async fn set_product_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ProductView, String> {
    let product = parties::set_product_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(product)
}

#[tauri::command]
pub async fn list_credit_memos(db: State<'_, Db>) -> Result<Vec<CreditMemoSummary>, String> {
    credits::list_credit_memos(&db.pool().await).await
}

#[tauri::command]
pub async fn create_credit_memo(app: AppHandle, db: State<'_, Db>, input: CreditMemoInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::create_credit_memo(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn apply_customer_credit(app: AppHandle, db: State<'_, Db>, input: ApplyCreditInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::apply_customer_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn list_customer_credit_applications(db: State<'_, Db>, credit_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    credits::list_customer_credit_applications(&db.pool().await, credit_id).await
}

#[tauri::command]
pub async fn remove_customer_credit(app: AppHandle, db: State<'_, Db>, application_id: i64) -> Result<(), String> {
    credits::remove_customer_credit(&db.pool().await, application_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn refund_customer_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::refund_customer_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn list_vendor_credits(db: State<'_, Db>) -> Result<Vec<VendorCreditSummary>, String> {
    credits::list_vendor_credits(&db.pool().await).await
}

#[tauri::command]
pub async fn create_vendor_credit(app: AppHandle, db: State<'_, Db>, input: VendorCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::create_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}

#[tauri::command]
pub async fn apply_vendor_credit(app: AppHandle, db: State<'_, Db>, input: ApplyCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::apply_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}

#[tauri::command]
pub async fn list_vendor_credit_applications(db: State<'_, Db>, credit_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    credits::list_vendor_credit_applications(&db.pool().await, credit_id).await
}

#[tauri::command]
pub async fn remove_vendor_credit(app: AppHandle, db: State<'_, Db>, application_id: i64) -> Result<(), String> {
    credits::remove_vendor_credit(&db.pool().await, application_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn refund_vendor_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::refund_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}

// ------------------------------------------------------------------------------------------------
// Sales tax
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_tax_rates(db: State<'_, Db>) -> Result<Vec<TaxRateView>, String> {
    sales_tax::list_tax_rates(&db.pool().await).await
}

#[tauri::command]
pub async fn create_tax_rate(app: AppHandle, db: State<'_, Db>, input: TaxRateInput) -> Result<TaxRateView, String> {
    let rate = sales_tax::create_tax_rate(&db.pool().await, input).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn update_tax_rate(app: AppHandle, db: State<'_, Db>, id: i64, input: TaxRateInput) -> Result<TaxRateView, String> {
    let rate = sales_tax::update_tax_rate(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn set_tax_rate_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<TaxRateView, String> {
    let rate = sales_tax::set_tax_rate_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn pay_sales_tax(app: AppHandle, db: State<'_, Db>, input: SalesTaxPaymentInput) -> Result<i64, String> {
    let id = sales_tax::pay_sales_tax(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_sales_tax_payments(db: State<'_, Db>) -> Result<Vec<SalesTaxPaymentView>, String> {
    sales_tax::list_sales_tax_payments(&db.pool().await).await
}

#[tauri::command]
pub async fn get_tax_liability(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<TaxLiabilityReport, String> {
    sales_tax::tax_liability(&db.pool().await, start_date, end_date).await
}

// ------------------------------------------------------------------------------------------------
// Editing, statements, 1099s, recurring, purchase orders, search, attachments, budgets
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn replace_invoice(app: AppHandle, db: State<'_, Db>, id: i64, input: InvoiceInput) -> Result<i64, String> {
    let new_id = edits::replace_invoice(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_bill(app: AppHandle, db: State<'_, Db>, id: i64, input: BillInput) -> Result<i64, String> {
    let new_id = edits::replace_bill(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_expense(app: AppHandle, db: State<'_, Db>, id: i64, input: ExpenseInput) -> Result<i64, String> {
    let new_id = edits::replace_expense(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn replace_journal_entry(app: AppHandle, db: State<'_, Db>, id: i64, entry: JournalEntry) -> Result<i64, String> {
    let new_id = edits::replace_journal_entry(&db.pool().await, id, entry).await?;
    notify(&app);
    Ok(new_id)
}

#[tauri::command]
pub async fn get_document_lines(db: State<'_, Db>, kind: String, id: i64) -> Result<Vec<DocumentLineView>, String> {
    edits::document_lines(&db.pool().await, &kind, id).await
}

#[tauri::command]
pub async fn get_customer_statement(db: State<'_, Db>, customer_id: i64, start_date: NaiveDate, end_date: NaiveDate) -> Result<CustomerStatement, String> {
    workflows::customer_statement(&db.pool().await, customer_id, start_date, end_date).await
}

#[tauri::command]
pub async fn list_statement_customers(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<(i64, String, i64)>, String> {
    workflows::statement_customers(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_1099_report(db: State<'_, Db>, year: i32) -> Result<Vec<Vendor1099Row>, String> {
    let (pool, session) = signed_in(&db).await?;
    workflows::vendors_1099(&pool, year, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn list_recurring(db: State<'_, Db>) -> Result<Vec<RecurringView>, String> {
    workflows::list_recurring(&db.pool().await).await
}

#[tauri::command]
pub async fn save_recurring(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: RecurringInput) -> Result<i64, String> {
    let id = workflows::save_recurring(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn set_recurring_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<(), String> {
    workflows::set_recurring_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_recurring(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    workflows::delete_recurring(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn enter_recurring(app: AppHandle, db: State<'_, Db>, as_of: Option<NaiveDate>, only: Option<i64>) -> Result<usize, String> {
    let entered = workflows::enter_due(&db.pool().await, as_of.unwrap_or_else(|| Local::now().date_naive()), only).await?;
    notify(&app);
    Ok(entered)
}

#[tauri::command]
pub async fn list_purchase_orders(db: State<'_, Db>) -> Result<Vec<PurchaseOrderView>, String> {
    workflows::list_purchase_orders(&db.pool().await).await
}

#[tauri::command]
pub async fn get_purchase_order_lines(db: State<'_, Db>, id: i64) -> Result<Vec<PurchaseOrderLineView>, String> {
    workflows::purchase_order_lines(&db.pool().await, id).await
}

#[tauri::command]
pub async fn create_purchase_order(app: AppHandle, db: State<'_, Db>, input: PurchaseOrderInput) -> Result<i64, String> {
    let id = workflows::create_purchase_order(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn set_purchase_order_closed(app: AppHandle, db: State<'_, Db>, id: i64, closed: bool) -> Result<(), String> {
    workflows::set_purchase_order_closed(&db.pool().await, id, closed).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn bill_purchase_order(app: AppHandle, db: State<'_, Db>, id: i64, bill_date: NaiveDate, due_date: NaiveDate, bill_number: Option<String>) -> Result<i64, String> {
    let bill = workflows::bill_purchase_order(&db.pool().await, id, bill_date, due_date, bill_number).await?;
    notify(&app);
    Ok(bill)
}

#[tauri::command]
pub async fn search_books(db: State<'_, Db>, query: String) -> Result<Vec<SearchHit>, String> {
    workflows::search(&db.pool().await, &query).await
}

/// Attaches a file chosen in the open dialog (read here, so the page never handles file bytes).
#[tauri::command]
pub async fn add_attachment(app: AppHandle, db: State<'_, Db>, journal_entry_id: i64, path: String) -> Result<AttachmentView, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    if meta.len() as usize > workflows::MAX_ATTACHMENT_BYTES {
        return Err("ATTACH_SIZE: attach files of up to 25 MB".into());
    }
    let content = std::fs::read(&path).map_err(|e| format!("ATTACH_READ: cannot read {path}: {e}"))?;
    let saved = workflows::add_attachment(&db.pool().await, journal_entry_id, &path, &content).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn list_attachments(db: State<'_, Db>, journal_entry_id: i64) -> Result<Vec<AttachmentView>, String> {
    workflows::list_attachments(&db.pool().await, journal_entry_id).await
}

/// Saves a copy of an attachment where the user chooses.
#[tauri::command]
pub async fn save_attachment(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<Option<String>, String> {
    let (name, content) = workflows::attachment_content(&db.pool().await, id).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&name).blocking_save_file())
        .await
        .map_err(|e| format!("ATTACH_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("ATTACH_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &content)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn delete_attachment(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    workflows::delete_attachment(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_budgets(db: State<'_, Db>) -> Result<Vec<BudgetView>, String> {
    statements::list_budgets(&db.pool().await).await
}

#[tauri::command]
pub async fn create_budget(app: AppHandle, db: State<'_, Db>, name: String, in_year: NaiveDate) -> Result<BudgetView, String> {
    let budget = statements::create_budget(&db.pool().await, &name, in_year).await?;
    notify(&app);
    Ok(budget)
}

#[tauri::command]
pub async fn delete_budget(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    statements::delete_budget(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_budget_amounts(db: State<'_, Db>, id: i64) -> Result<Vec<BudgetAmount>, String> {
    statements::budget_amounts(&db.pool().await, id).await
}

#[tauri::command]
pub async fn set_budget_amounts(app: AppHandle, db: State<'_, Db>, id: i64, amounts: Vec<BudgetAmount>) -> Result<(), String> {
    statements::set_budget_amounts(&db.pool().await, id, &amounts).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_budget_vs_actual(db: State<'_, Db>, id: i64, from_period: i64, to_period: i64) -> Result<ProfitAndLossByClass, String> {
    statements::budget_vs_actual(&db.pool().await, id, from_period, to_period).await
}

// ------------------------------------------------------------------------------------------------
// Users and signing in
// ------------------------------------------------------------------------------------------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInState {
    /// The file asks who is signing in.
    pub users_enabled: bool,
    pub user: Option<UserView>,
}

/// Signing in or out changes what every window may show, so each reloads (as for another file).
fn signed_in_changed(app: &AppHandle) {
    let _ = app.emit(COMPANY_FILE_CHANGED, ());
}

/// Whether the open company file has users and who is signed in; `None` with no company open.
#[tauri::command]
pub async fn get_sign_in_state(db: State<'_, Db>) -> Result<Option<SignInState>, String> {
    let Ok((pool, session)) = db.pool_and_session().await else { return Ok(None) };
    let users_enabled = users::users_enabled(&pool).await?;
    let user = session.lock().await.as_ref().map(|s| s.user.clone());
    Ok(Some(SignInState { users_enabled, user }))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersTurnedOn {
    pub user: UserView,
    /// Shown once; LocalBooks does not keep it.
    pub recovery_key: String,
}

#[tauri::command]
pub async fn turn_on_users(db: State<'_, Db>, username: String, display_name: Option<String>, password: String) -> Result<UsersTurnedOn, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let (session, recovery) = users::turn_on(&pool, &username, display_name.as_deref(), &password).await?;
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    // No reload here: the window shows the recovery key first.
    Ok(UsersTurnedOn { user, recovery_key: recovery.as_str().to_string() })
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, db: State<'_, Db>, username: String, password: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let session = users::sign_in(&pool, &username, &password).await?;
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(user)
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let (_, slot) = db.pool_and_session().await?;
    *slot.lock().await = None;
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_users(db: State<'_, Db>) -> Result<Vec<UserView>, String> {
    let (pool, slot) = db.pool_and_session().await?;
    Session::require_admin(slot.lock().await.as_ref())?;
    users::list(&pool).await
}

#[tauri::command]
pub async fn add_user(db: State<'_, Db>, input: NewUserInput) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    users::add(&pool, Session::require_admin(guard.as_ref())?, input).await
}

#[tauri::command]
pub async fn update_user(db: State<'_, Db>, id: i64, input: UserUpdate) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    users::update(&pool, Session::require_admin(guard.as_ref())?, id, input).await
}

#[tauri::command]
pub async fn change_my_password(db: State<'_, Db>, current: String, next: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let user = {
        let mut guard = slot.lock().await;
        let session = guard.as_mut().ok_or_else(|| users::SIGN_IN_REQUIRED.to_string())?;
        users::change_own_password(&pool, session, &current, &next).await?
    };
    // A temporary password no longer holds the user at the sign-in screen.
    db.refresh_gate().await;
    Ok(user)
}

/// A forgotten Admin password: the recovery key sets a new one and signs that Admin in.
#[tauri::command]
pub async fn recover_with_recovery_key(app: AppHandle, db: State<'_, Db>, recovery_key: String, username: String, new_password: String) -> Result<UserView, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let session = users::recover(&pool, &recovery_key, &username, &new_password).await?;
    let user = session.user.clone();
    *slot.lock().await = Some(session);
    db.refresh_gate().await;
    signed_in_changed(&app);
    Ok(user)
}

#[tauri::command]
pub async fn replace_recovery_key(db: State<'_, Db>) -> Result<String, String> {
    let (pool, slot) = db.pool_and_session().await?;
    let guard = slot.lock().await;
    let recovery = users::replace_recovery_key(&pool, Session::require_admin(guard.as_ref())?).await?;
    Ok(recovery.as_str().to_string())
}

/// The last resort on the sign-in screen when no Admin password or recovery key is left.
#[tauri::command]
pub async fn remove_all_users(app: AppHandle, db: State<'_, Db>, confirmation: String) -> Result<(), String> {
    let (pool, slot) = db.pool_and_session().await?;
    users::remove_all_users(&pool, &confirmation).await?;
    *slot.lock().await = None;
    db.refresh_gate().await;
    signed_in_changed(&app);
    notify(&app);
    Ok(())
}
