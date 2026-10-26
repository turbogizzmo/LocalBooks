//! What every command file shares: the imports below and the few helpers most of them call.

pub(super) use std::collections::BTreeMap;
pub(super) use std::path::{Path, PathBuf};
pub(super) use chrono::{Local, NaiveDate};
pub(super) use tauri::{AppHandle, Emitter, Manager, State};
pub(super) use tauri_plugin_dialog::DialogExt;
pub(super) use crate::accounts::{self, AccountInput, AccountTypeView};
pub(super) use crate::ach::{self, AchFileView, AchPaymentInput, OriginatorDetails, VendorBankInput, VendorBankView};
pub(super) use crate::backup::{self, BackupFile, CompanyFileInfo, RestoreResult};
pub(super) use crate::bank::suggest::Suggester;
pub(super) use crate::bank::{self, BankFileInspection, BankTransactionView, ColumnMapping, ImportSummary};
pub(super) use crate::classes::{self, ClassInput, ClassView, ReclassLine, ReclassQuery};
pub(super) use crate::custom_fields::{self, CustomFieldInput, CustomFieldView, FieldValue, RecordField};
pub(super) use crate::sales_lists::{self, SalesRepInput, SalesRepView, ShipMethodView};
pub(super) use crate::closing::{self, ClosingInput, ClosingStatus};
pub(super) use crate::credits::{self, ApplyCreditInput, CreditApplicationView, CreditMemoInput, CreditMemoSummary, RefundCreditInput, VendorCreditInput, VendorCreditSummary};
pub(super) use crate::parties::{self, PartyDetails, PartyDetailsInput};
pub(super) use crate::registers::{self, Register};
pub(super) use crate::edits::{self, DocumentLineView};
pub(super) use crate::workflows::{self, AttachmentView, CustomerStatement, PurchaseOrderInput, PurchaseOrderLineView, PurchaseOrderView, RecurringInput, RecurringView, SearchHit, Vendor1099Row};
pub(super) use crate::sales_tax::{self, SalesTaxPaymentInput, SalesTaxPaymentView, TaxLiabilityReport, TaxRateInput, TaxRateView};
pub(super) use crate::forms::{self, EmailDraft, FormKind, FormStyle, FormTemplate, Period, RenderedForm};
pub(super) use crate::email_log::{self, EmailStatus, QueuedForm, SentRecord};
pub(super) use crate::mail_app::{self, ComposeOutcome};
pub(super) use crate::secrets::SecretStore;
pub(super) use crate::windows;
pub(super) use crate::smtp::{self, SmtpSettings, SmtpStatus};
pub(super) use crate::file_dentist::{self, CheckupReport, RebuildResult};
pub(super) use crate::forensic::{self, AppliedCorrection, ApplyCorrectionInput, CorrectionProposal};
pub(super) use crate::{gnucash_import, iif_import, qif_import, report_import};
pub(super) use crate::db::{Db, RelocateMode, StorageStatus, NO_COMPANY_OPEN};
pub(super) use crate::tax_ids;
pub(super) use crate::users::{self, NewUserInput, Session, UserUpdate, UserView};
pub(super) use crate::storage::{self, RecentCompany, RecentCompanyView, StorageInfo};
pub(super) use crate::export_engine;
pub(super) use crate::xlsx_export;
pub(super) use crate::preferences::{self, FiscalPeriods, TaxDeadline};
pub(super) use crate::banking::{self, BankingActivity, DepositInput, TransferInput, UndepositedItem};
pub(super) use crate::estimates::{self, EstimateDetail, EstimateInput, EstimateSummary};
pub(super) use crate::inventory::{self, AdjustInput, InventoryItemView, StockMovementView, TrackInput};
pub(super) use crate::fx_rates;
pub(super) use crate::revaluation::{self, RevaluationPreview, RevaluationView};
pub(super) use crate::expenses::{self, BillInput, BillPaymentInput, BillPaymentView, BillSummary, ExpenseInput, ExpenseView};
pub(super) use crate::invoices::{self, ArAging, InvoiceDetail, InvoiceInput, InvoiceSummary, PaymentInput, PaymentView};
pub(super) use crate::ledger::{self, AuditEntry, JournalEntry, JournalEntryView, JournalLineView, LedgerVerification};
pub(super) use crate::mcp_server::{McpService, McpStatus};
pub(super) use crate::qbw_import::{self, BatchListItem, BatchSummary, PostBatchInput, PostBatchResult, ReverseBatchResult};
pub(super) use crate::qbw_ledger_import::{self, AutoMapResult, ImportListsResult, LedgerBatchSummary, PostLedgerInput, PostLedgerResult, ReverseLedgerResult};
pub(super) use crate::{qbw_backup, qbw_company};
pub(super) use crate::timesheet::{self, TimeActivityInput, TimeActivityView};
pub(super) use crate::reconcile::{self, ReconcileAccountState, ReconcileProgress, Reconciliation, UnreconciledLine};
pub(super) use crate::reports::{self, AccountView, Dashboard};
pub(super) use crate::simplefin::{self, ConnectionView, SimpleFin, SyncSummary};
pub(super) use crate::sales::{self, PartyView, ProductInput, ProductView, SalesReceiptInput, SalesReceiptSummary, SalesTransaction};
pub(super) use crate::statements::{self, AsOfColumn, BalanceSheet, Basis, BudgetAmount, BudgetView, CashFlowStatement, ClassFilter, DatedTrialBalance, PeriodReportInput, ProfitAndLoss, ProfitAndLossByClass};
pub(super) use crate::report_center::{self, AgingReport, GeneralLedger, InventoryValuation, InventoryValuationDetail, MissingChecks, StockStatus, SummaryReport};

pub(super) const LEDGER_CHANGED: &str = "ledger-changed";

pub(super) fn notify(app: &AppHandle) {
    // A failed emit only means no window is listening; the write itself already committed.
    let _ = app.emit(LEDGER_CHANGED, ());
}

/// The open file's pool and its sign-in, held for the length of a command that uses the data key.
pub(super) async fn signed_in(db: &Db) -> Result<(sqlx::SqlitePool, tokio::sync::OwnedMutexGuard<Option<Session>>), String> {
    let (pool, slot) = db.pool_and_session().await?;
    Ok((pool, slot.lock_owned().await))
}

/// What the signed-in user may do with sensitive data in the open file.
pub(super) async fn sensitive_access<'a>(pool: &sqlx::SqlitePool, session: &'a Option<Session>) -> Result<tax_ids::Access<'a>, String> {
    let mut conn = pool.acquire().await.map_err(ledger::db_err)?;
    tax_ids::Access::of(&mut conn, session.as_ref().and_then(|s| s.key.as_ref())).await
}

/// After an import: encrypts the tax IDs it wrote in plain, when the signed-in user holds the key
/// (otherwise they are encrypted at the next sign-in of someone who does).
pub(super) async fn protect_imported(pool: &sqlx::SqlitePool, db: &Db) {
    let Ok((_, session)) = signed_in(db).await else { return };
    let Some(key) = session.as_ref().and_then(|s| s.key.as_ref()) else { return };
    let protected = async { tax_ids::protect_all(&mut *pool.acquire().await.map_err(ledger::db_err)?, key).await };
    if let Err(err) = protected.await {
        eprintln!("LocalBooks: could not encrypt imported tax IDs: {err}");
    }
}
