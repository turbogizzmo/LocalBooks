import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const LEDGER_CHANGED = 'ledger-changed';
/** A different company file became the live one (double-click, File › Open), or none is (Close Company). */
export const COMPANY_FILE_CHANGED = 'company-file-changed';

export const api = {
  listAccounts: () => invoke('list_accounts'),
  listAccountTypes: () => invoke('list_account_types'),
  createAccount: (input) => invoke('create_account', { input }),
  updateAccount: (id, input) => invoke('update_account', { id, input }),
  setAccountActive: (id, active) => invoke('set_account_active', { id, active }),
  listClasses: () => invoke('list_classes'),
  createClass: (input) => invoke('create_class', { input }),
  updateClass: (id, input) => invoke('update_class', { id, input }),
  setClassActive: (id, active) => invoke('set_class_active', { id, active }),
  deleteClass: (id) => invoke('delete_class', { id }),
  listSalesReps: () => invoke('list_sales_reps'),
  createSalesRep: (input) => invoke('create_sales_rep', { input }),
  updateSalesRep: (id, input) => invoke('update_sales_rep', { id, input }),
  setSalesRepActive: (id, active) => invoke('set_sales_rep_active', { id, active }),
  deleteSalesRep: (id) => invoke('delete_sales_rep', { id }),
  listShipMethods: () => invoke('list_ship_methods'),
  createShipMethod: (name) => invoke('create_ship_method', { name }),
  renameShipMethod: (id, name) => invoke('rename_ship_method', { id, name }),
  setShipMethodActive: (id, active) => invoke('set_ship_method_active', { id, active }),
  deleteShipMethod: (id) => invoke('delete_ship_method', { id }),
  // list: 'CUSTOMER' | 'VENDOR' | 'ITEM'
  listCustomFields: () => invoke('list_custom_fields'),
  createCustomField: (list, input) => invoke('create_custom_field', { list, input }),
  updateCustomField: (id, input) => invoke('update_custom_field', { id, input }),
  deleteCustomField: (id) => invoke('delete_custom_field', { id }),
  getCustomValues: (list, recordId) => invoke('get_custom_values', { list, recordId }),
  saveCustomValues: (list, recordId, values) => invoke('save_custom_values', { list, recordId, values }),
  listReclassLines: (query) => invoke('list_reclass_lines', { query }),
  reclassifyLines: (lineIds, classId) => invoke('reclassify_lines', { lineIds, classId }),
  getDashboard: () => invoke('get_dashboard'),
  listInvoices: () => invoke('list_invoices'),
  createInvoice: (input) => invoke('create_invoice', { input }),
  recordInvoicePayment: (input) => invoke('record_invoice_payment', { input }),
  listInvoicePayments: (invoiceId) => invoke('list_invoice_payments', { invoiceId }),
  getArAging: () => invoke('get_ar_aging', { asOf: null }),
  previewFinanceCharges: (asOf) => invoke('preview_finance_charges', { asOf }),
  assessFinanceCharges: (input) => invoke('assess_finance_charges', { input }),
  importBankFile: (path, bankAccountId, mapping = null) => invoke('import_bank_csv', { path, bankAccountId, mapping }),
  inspectBankFile: (path, mapping = null) => invoke('inspect_bank_file', { path, mapping }),
  listBankTransactions: () => invoke('list_bank_transactions'),
  approveBankTransaction: (transactionId, categoryAccountId, classId = null) =>
    invoke('approve_bank_transaction', { transactionId, categoryAccountId, classId }),
  excludeBankTransaction: (transactionId) => invoke('exclude_bank_transaction', { transactionId }),
  listBankConnections: () => invoke('simplefin_list'),
  connectBank: (token) => invoke('simplefin_connect', { token }),
  reconnectBank: (connectionId, token) => invoke('simplefin_reconnect', { connectionId, token }),
  refreshBankAccounts: (connectionId) => invoke('simplefin_refresh_accounts', { connectionId }),
  linkBankAccount: (accountId, bankAccountId, syncFrom) => invoke('simplefin_link', { accountId, bankAccountId, syncFrom }),
  unlinkBankAccount: (accountId) => invoke('simplefin_unlink', { accountId }),
  suggestedBankStart: (bankAccountId) => invoke('simplefin_suggested_start', { bankAccountId }),
  syncBankConnection: (connectionId) => invoke('simplefin_sync', { connectionId }),
  disconnectBank: (connectionId) => invoke('simplefin_disconnect', { connectionId }),

  getReconcileState: (accountId) => invoke('get_reconcile_state', { accountId }),
  getUnreconciledLines: (accountId) => invoke('get_unreconciled_lines', { accountId }),
  listReconciliations: (accountId) => invoke('list_reconciliations', { accountId }),
  startReconciliation: (accountId, statementEndDate, statementEndingBalanceCents) =>
    invoke('start_reconciliation', { accountId, statementEndDate, statementEndingBalanceCents }),
  setLinesCleared: (reconciliationId, lineIds, cleared) =>
    invoke('set_lines_cleared', { reconciliationId, lineIds, cleared }),
  lockReconciliation: (reconciliationId) => invoke('lock_reconciliation', { reconciliationId }),
  cancelReconciliation: (reconciliationId) => invoke('cancel_reconciliation', { reconciliationId }),

  getProfitAndLoss: (startDate, endDate, classFilter = null, basis = 'ACCRUAL') => invoke('get_profit_and_loss', { startDate, endDate, classFilter, basis }),
  getProfitAndLossPeriods: (input) => invoke('get_profit_and_loss_periods', { input }),
  getTrialBalanceAsOf: (asOf) => invoke('get_trial_balance_as_of', { asOf }),
  getCashFlow: (startDate, endDate) => invoke('get_cash_flow', { startDate, endDate }),
  getGeneralLedger: (startDate, endDate) => invoke('get_general_ledger', { startDate, endDate }),
  getAging: (payable, asOf) => invoke('get_aging', { payable, asOf }),
  getSummaryReport: (by, startDate, endDate) => invoke('get_summary_report', { by, startDate, endDate }),
  getProfitAndLossByClass: (startDate, endDate) => invoke('get_profit_and_loss_by_class', { startDate, endDate }),
  getBalanceSheet: (asOf) => invoke('get_balance_sheet', { asOf }),
  getBalanceSheetColumns: (columns, compare) => invoke('get_balance_sheet_columns', { columns, compare }),
  getInventoryValuation: (asOf) => invoke('get_inventory_valuation', { asOf }),
  getInventoryValuationDetail: (startDate, endDate) => invoke('get_inventory_valuation_detail', { startDate, endDate }),
  getStockStatus: (startDate, endDate) => invoke('get_stock_status', { startDate, endDate }),
  getMissingChecks: (accountId, startDate, endDate) => invoke('get_missing_checks', { accountId, startDate: startDate || null, endDate: endDate || null }),

  listCustomers: () => invoke('list_customers'),
  createCustomer: (name, email) => invoke('create_customer', { name, email: email || null }),
  listProducts: () => invoke('list_products'),
  listPriceItems: () => invoke('list_price_items'),
  previewItemPrices: (input) => invoke('preview_item_prices', { input }),
  applyItemPrices: (input) => invoke('apply_item_prices', { input }),
  listPriceChanges: () => invoke('list_price_changes'),
  listInventoryItems: () => invoke('list_inventory_items'),
  trackInventory: (input) => invoke('track_inventory', { input }),
  adjustInventory: (input) => invoke('adjust_inventory', { input }),
  listStockMovements: (itemId) => invoke('list_stock_movements', { itemId }),
  previewFxRevaluation: (asOf, rates) => invoke('preview_fx_revaluation', { asOf, rates }),
  postFxRevaluation: (asOf, rates) => invoke('post_fx_revaluation', { asOf, rates }),
  listFxRevaluations: () => invoke('list_fx_revaluations'),
  voidFxRevaluation: (id) => invoke('void_fx_revaluation', { id }),
  lookupExchangeRate: (currency, date) => invoke('lookup_exchange_rate', { currency, date }),
  listFormStyles: () => invoke('list_form_styles'),
  createFormStyle: (name, copyFrom = null) => invoke('create_form_style', { name, copyFrom }),
  renameFormStyle: (id, name) => invoke('rename_form_style', { id, name }),
  setDefaultFormStyle: (id) => invoke('set_default_form_style', { id }),
  deleteFormStyle: (id) => invoke('delete_form_style', { id }),
  // styleId null = the default style. For STATEMENT forms the document id is the customer's and
  // period is { startDate, endDate } (last month when omitted).
  getFormTemplate: (kind, styleId = null) => invoke('get_form_template', { kind, styleId }),
  saveFormTemplate: (kind, template, styleId = null) => invoke('save_form_template', { kind, template, styleId }),
  resetFormTemplate: (kind, styleId = null) => invoke('reset_form_template', { kind, styleId }),
  previewForm: (kind, template, documentId = null, period = null) => invoke('preview_form', { kind, template, documentId, period }),
  renderForm: (kind, documentId, period = null) => invoke('render_form', { kind, documentId, period }),
  saveFormPdf: (kind, documentId, period = null) => invoke('save_form_pdf', { kind, documentId, period }),
  // `period` is for statements ({ startDate, endDate }).
  getEmailDraft: (kind, documentId, period = null) => invoke('get_email_draft', { kind, documentId, period }),
  emailFormWithMailApp: (kind, documentId, email, period = null) => invoke('email_form_with_mail_app', { kind, documentId, period, email }),
  emailFormWithSmtp: (kind, documentId, email, period = null) => invoke('email_form_with_smtp', { kind, documentId, period, email }),
  getSmtpSettings: () => invoke('get_smtp_settings'),
  sendSmtpTestEmail: (settings, password = null) => invoke('send_smtp_test_email', { settings, password }),
  getEmailHistory: (kind, documentId) => invoke('get_email_history', { kind, documentId }),
  listEmailStatuses: (kind) => invoke('list_email_statuses', { kind }),
  setEmailLater: (kind, documentId, queued) => invoke('set_email_later', { kind, documentId, queued }),
  listEmailQueue: () => invoke('list_email_queue'),
  saveSmtpSettings: (settings, password = null) => invoke('save_smtp_settings', { settings, password }),
  testSmtpSettings: (settings, password = null) => invoke('test_smtp_settings', { settings, password }),
  getEstimateDetail: (estimateId) => invoke('get_estimate_detail', { estimateId }),
  invoiceFromEstimate: (estimateId, input) => invoke('invoice_from_estimate', { estimateId, input }),
  createProduct: (input) => invoke('create_product', { input }),
  postSalesReceipt: (input) => invoke('post_sales_receipt', { input }),
  listAllSales: () => invoke('list_all_sales'),

  listVendors: () => invoke('list_vendors'),
  createVendor: (name, email) => invoke('create_vendor', { name, email: email || null }),
  listBills: () => invoke('list_bills'),
  replaceInvoice: (id, input) => invoke('replace_invoice', { id, input }),
  replaceBill: (id, input) => invoke('replace_bill', { id, input }),
  replaceExpense: (id, input) => invoke('replace_expense', { id, input }),
  replaceJournalEntry: (id, entry) => invoke('replace_journal_entry', { id, entry }),
  getDocumentLines: (kind, id) => invoke('get_document_lines', { kind, id }),
  getCustomerStatement: (customerId, startDate, endDate) => invoke('get_customer_statement', { customerId, startDate, endDate }),
  listStatementCustomers: (startDate, endDate) => invoke('list_statement_customers', { startDate, endDate }),
  get1099Report: (year) => invoke('get_1099_report', { year }),
  listRecurring: () => invoke('list_recurring'),
  saveRecurring: (id, input) => invoke('save_recurring', { id, input }),
  setRecurringActive: (id, active) => invoke('set_recurring_active', { id, active }),
  deleteRecurring: (id) => invoke('delete_recurring', { id }),
  enterRecurring: (asOf = null, only = null) => invoke('enter_recurring', { asOf, only }),
  listPurchaseOrders: () => invoke('list_purchase_orders'),
  getPurchaseOrderLines: (id) => invoke('get_purchase_order_lines', { id }),
  createPurchaseOrder: (input) => invoke('create_purchase_order', { input }),
  setPurchaseOrderClosed: (id, closed) => invoke('set_purchase_order_closed', { id, closed }),
  billPurchaseOrder: (id, billDate, dueDate, billNumber) => invoke('bill_purchase_order', { id, billDate, dueDate, billNumber }),
  searchBooks: (query) => invoke('search_books', { query }),
  addAttachment: (journalEntryId, path) => invoke('add_attachment', { journalEntryId, path }),
  listAttachments: (journalEntryId) => invoke('list_attachments', { journalEntryId }),
  saveAttachment: (id) => invoke('save_attachment', { id }),
  deleteAttachment: (id) => invoke('delete_attachment', { id }),
  listBudgets: () => invoke('list_budgets'),
  createBudget: (name, inYear) => invoke('create_budget', { name, inYear }),
  deleteBudget: (id) => invoke('delete_budget', { id }),
  getBudgetAmounts: (id) => invoke('get_budget_amounts', { id }),
  setBudgetAmounts: (id, amounts) => invoke('set_budget_amounts', { id, amounts }),
  getBudgetVsActual: (id, fromPeriod, toPeriod) => invoke('get_budget_vs_actual', { id, fromPeriod, toPeriod }),
  getBalanceSheetBudgetVsActual: (id, period) => invoke('get_balance_sheet_budget_vs_actual', { id, period }),
  listTaxRates: () => invoke('list_tax_rates'),
  createTaxRate: (input) => invoke('create_tax_rate', { input }),
  updateTaxRate: (id, input) => invoke('update_tax_rate', { id, input }),
  setTaxRateActive: (id, active) => invoke('set_tax_rate_active', { id, active }),
  paySalesTax: (input) => invoke('pay_sales_tax', { input }),
  listSalesTaxPayments: () => invoke('list_sales_tax_payments'),
  getTaxLiability: (startDate, endDate) => invoke('get_tax_liability', { startDate, endDate }),
  getAccountRegister: (accountId, startDate = null, endDate = null, includeSubaccounts = false) =>
    invoke('get_account_register', { accountId, startDate, endDate, includeSubaccounts }),
  getParty: (kind, id) => invoke('get_party', { kind, id }),
  updateParty: (kind, id, input) => invoke('update_party', { kind, id, input }),
  setPartyActive: (kind, id, active) => invoke('set_party_active', { kind, id, active }),
  updateProduct: (id, input) => invoke('update_product', { id, input }),
  setProductActive: (id, active) => invoke('set_product_active', { id, active }),
  listCreditMemos: () => invoke('list_credit_memos'),
  createCreditMemo: (input) => invoke('create_credit_memo', { input }),
  applyCustomerCredit: (input) => invoke('apply_customer_credit', { input }),
  listCustomerCreditApplications: (creditId) => invoke('list_customer_credit_applications', { creditId }),
  removeCustomerCredit: (applicationId) => invoke('remove_customer_credit', { applicationId }),
  refundCustomerCredit: (input) => invoke('refund_customer_credit', { input }),
  listVendorCredits: () => invoke('list_vendor_credits'),
  createVendorCredit: (input) => invoke('create_vendor_credit', { input }),
  applyVendorCredit: (input) => invoke('apply_vendor_credit', { input }),
  listVendorCreditApplications: (creditId) => invoke('list_vendor_credit_applications', { creditId }),
  removeVendorCredit: (applicationId) => invoke('remove_vendor_credit', { applicationId }),
  refundVendorCredit: (input) => invoke('refund_vendor_credit', { input }),
  postVendorBill: (input) => invoke('post_vendor_bill', { input }),
  payVendorBill: (input) => invoke('pay_vendor_bill', { input }),
  payVendorBills: (inputs) => invoke('pay_vendor_bills', { inputs }),
  listBillPayments: (billId) => invoke('list_bill_payments', { billId }),
  postExpense: (input) => invoke('post_expense', { input }),
  listExpenses: () => invoke('list_expenses'),
  recordInvoicePayments: (inputs) => invoke('record_invoice_payments', { inputs }),

  voidTransaction: (journalEntryId) => invoke('void_transaction', { journalEntryId }),
  listJournalEntries: () => invoke('list_journal_entries', { limit: 500 }),
  getJournalEntryLines: (journalEntryId) => invoke('get_journal_entry_lines', { journalEntryId }),
  listAuditLog: () => invoke('list_audit_log', { limit: 1000 }),
  verifyLedgerIntegrity: () => invoke('verify_ledger_integrity'),
  fileDentistCheckup: () => invoke('file_dentist_checkup'),
  fileDentistRebuild: () => invoke('file_dentist_rebuild'),
  proposeForensicCorrections: () => invoke('propose_forensic_corrections'),
  applyForensicCorrection: (input) => invoke('apply_forensic_correction', { input }),
  postJournalEntry: (entry) => invoke('post_journal_entry', { entry }),

  listUndeposited: () => invoke('list_undeposited'),
  postBankDeposit: (input) => invoke('post_bank_deposit', { input }),
  postTransfer: (input) => invoke('post_transfer', { input }),
  listBankingActivity: () => invoke('list_banking_activity'),

  listEstimates: () => invoke('list_estimates'),
  createEstimate: (input) => invoke('create_estimate', { input }),
  setEstimateStatus: (estimateId, status) => invoke('set_estimate_status', { estimateId, status }),
  convertEstimateToInvoice: (estimateId, issueDate, dueDate) =>
    invoke('convert_estimate_to_invoice', { estimateId, issueDate, dueDate }),

  listEmployees: () => invoke('list_employees'),
  createEmployee: (name, email) => invoke('create_employee', { name, email: email || null }),
  createTimeActivities: (inputs) => invoke('create_time_activities', { inputs }),
  listTimeActivities: (from, to) => invoke('list_time_activities', { from, to }),

  exportCompanyFile: (destinationPath) => invoke('export_company_file', { destinationPath }),
  inspectCompanyFile: (sourcePath) => invoke('inspect_company_file', { sourcePath }),
  importCompanyFile: (sourcePath) => invoke('import_company_file', { sourcePath }),
  listBackups: () => invoke('list_backups'),
  backupNow: () => invoke('backup_now'),

  getPreferences: () => invoke('get_preferences'),
  setPreference: (key, value) => invoke('set_preference', { key, value }),
  listTaxForms: () => invoke('list_tax_forms'),
  getFiscalPeriods: () => invoke('get_fiscal_periods', { asOf: null }),
  getTaxCalendar: () => invoke('get_tax_calendar', { asOf: null }),

  reportCsv: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}) =>
    invoke('report_csv', { report, startDate, endDate, asOf, classFilter }),
  saveReportToDisk: (reportHtmlOrMatrix, filename) => invoke('save_report_to_disk', { reportHtmlOrMatrix, filename }),
  saveReportXlsx: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}, filename) =>
    invoke('save_report_xlsx', { report, startDate, endDate, asOf, classFilter, filename }),
  saveXlsx: (sheet, filename) => invoke('save_xlsx', { sheet, filename }),
  printReport: () => invoke('print_report'),

  getStorageStatus: () => invoke('get_storage_status'),
  getStartupFailure: () => invoke('get_startup_failure'),
  openCompanyFile: (path) => invoke('open_company_file', { path }),
  getStartScreen: () => invoke('get_start_screen'),
  forgetRecentCompany: (path) => invoke('forget_recent_company', { path }),
  findCompanyFiles: () => invoke('find_company_files'),
  createCompanyFile: (path, companyName) => invoke('create_company_file', { path, companyName }),
  restoreBackupAs: (sourcePath, destinationPath) => invoke('restore_backup_as', { sourcePath, destinationPath }),
  closeCompanyFile: () => invoke('close_company_file'),
  getSignInState: () => invoke('get_sign_in_state'),
  turnOnUsers: (username, displayName, password) => invoke('turn_on_users', { username, displayName: displayName || null, password }),
  signIn: (username, password, remember = false) => invoke('sign_in', { username, password, remember }),
  signInRemembered: () => invoke('sign_in_remembered'),
  forgetRememberedSignIn: () => invoke('forget_remembered_sign_in'),
  touchSession: () => invoke('touch_session'),
  signOutIfIdle: () => invoke('sign_out_if_idle'),
  signOut: () => invoke('sign_out'),
  listUsers: () => invoke('list_users'),
  addUser: (input) => invoke('add_user', { input }),
  updateUser: (id, input) => invoke('update_user', { id, input }),
  changeMyPassword: (current, next) => invoke('change_my_password', { current, next }),
  recoverWithRecoveryKey: (recoveryKey, username, newPassword) => invoke('recover_with_recovery_key', { recoveryKey, username, newPassword }),
  replaceRecoveryKey: () => invoke('replace_recovery_key'),
  removeAllUsers: (confirmation) => invoke('remove_all_users', { confirmation }),
  getClosingStatus: () => invoke('get_closing_status'),
  setClosingDate: (input) => invoke('set_closing_date', { input }),
  getMcpStatus: () => invoke('get_mcp_status'),
  setMcpEnabled: (enabled) => invoke('set_mcp_enabled', { enabled }),
  regenerateMcpToken: () => invoke('regenerate_mcp_token'),
  inspectDatabaseDirectory: (newPath) => invoke('inspect_database_directory', { newPath }),
  updateDatabaseDirectory: (newPath, mode) => invoke('update_database_directory', { newPath, mode: mode ?? null }),
  setCompanyLogo: (sourcePath) => invoke('set_company_logo', { sourcePath }),
  clearCompanyLogo: () => invoke('clear_company_logo'),
  getCompanyLogo: () => invoke('get_company_logo'),
  getInvoiceDetail: (invoiceId) => invoke('get_invoice_detail', { invoiceId }),

  ingestRawQbw: (filePath) => invoke('ingest_raw_qbw', { filePath }),
  ingestReportExports: (filePaths) => invoke('ingest_report_exports', { filePaths }),
  listQbwBatches: () => invoke('list_qbw_batches'),
  getQbwBatch: (batchId) => invoke('get_qbw_batch', { batchId }),
  setQbwMapping: (batchId, rawLabel, accountId) => invoke('set_qbw_mapping', { batchId, rawLabel, accountId }),
  autoCreateQbwAccounts: (batchId) => invoke('auto_create_qbw_accounts', { batchId }),
  postQbwBatch: (input) => invoke('post_qbw_batch', { input }),
  discardQbwBatch: (batchId) => invoke('discard_qbw_batch', { batchId }),
  reverseQbwBatch: (batchId) => invoke('reverse_qbw_batch', { batchId }),
  listLedgerBatches: () => invoke('list_ledger_batches'),
  getLedgerBatch: (batchId) => invoke('get_ledger_batch', { batchId }),
  setLedgerAccount: (batchId, sourceId, accountId) => invoke('set_ledger_account', { batchId, sourceId, accountId }),
  autoMapLedgerAccounts: (batchId) => invoke('auto_map_ledger_accounts', { batchId }),
  importLedgerLists: (batchId) => invoke('import_ledger_lists', { batchId }),
  postLedgerBatch: (input) => invoke('post_ledger_batch', { input }),
  reverseLedgerBatch: (batchId) => invoke('reverse_ledger_batch', { batchId }),
  discardLedgerBatch: (batchId) => invoke('discard_ledger_batch', { batchId }),
  startOverLedgerBatch: (batchId) => invoke('start_over_ledger_batch', { batchId }),
};

/** Opens the system print dialog (Print / Save as PDF), falling back to the browser's. */
export async function printDocument() {
  try {
    await api.printReport();
  } catch {
    window.print();
  }
}

export const PAYMENT_METHODS = [
  { id: 'CHECK', label: 'Check' },
  { id: 'CASH', label: 'Cash' },
  { id: 'CREDIT_CARD', label: 'Credit card' },
  { id: 'ACH', label: 'ACH / bank transfer' },
  { id: 'WIRE', label: 'Wire' },
  { id: 'OTHER', label: 'Other' },
];

/** Backend errors look like "JE_UNBALANCED: total debits ..."; show the human part. */
export function errorText(err) {
  const text = typeof err === 'string' ? err : (err?.message ?? String(err));
  return text.replace(/^[A-Z][A-Z0-9_]*: /, '');
}

/**
 * Runs `fetcher` on mount and again whenever the backend reports a committed ledger write.
 * Out-of-order responses are discarded so a slow early request cannot overwrite fresher data.
 */
export function useLedgerQuery(fetcher) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const fetcherRef = useRef(fetcher);
  const seq = useRef(0);
  fetcherRef.current = fetcher;

  const reload = useCallback(() => {
    const id = ++seq.current;
    return fetcherRef.current().then(
      (result) => {
        if (id !== seq.current) return;
        setData(result);
        setError(null);
      },
      (err) => {
        if (id !== seq.current) return;
        setError(errorText(err));
      },
    );
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten = null;
    reload();
    listen(LEDGER_CHANGED, () => reload()).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [reload]);

  return { data, error, reload };
}
