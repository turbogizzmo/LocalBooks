import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const LEDGER_CHANGED = 'ledger-changed';
/** A different company file became the live one (double-click, File › Open). */
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
  listReclassLines: (query) => invoke('list_reclass_lines', { query }),
  reclassifyLines: (lineIds, classId) => invoke('reclassify_lines', { lineIds, classId }),
  getDashboard: () => invoke('get_dashboard'),
  listInvoices: () => invoke('list_invoices'),
  createInvoice: (input) => invoke('create_invoice', { input }),
  recordInvoicePayment: (input) => invoke('record_invoice_payment', { input }),
  listInvoicePayments: (invoiceId) => invoke('list_invoice_payments', { invoiceId }),
  getArAging: () => invoke('get_ar_aging', { asOf: null }),
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

  listCustomers: () => invoke('list_customers'),
  createCustomer: (name, email) => invoke('create_customer', { name, email: email || null }),
  listProducts: () => invoke('list_products'),
  listInventoryItems: () => invoke('list_inventory_items'),
  trackInventory: (input) => invoke('track_inventory', { input }),
  adjustInventory: (input) => invoke('adjust_inventory', { input }),
  listStockMovements: (itemId) => invoke('list_stock_movements', { itemId }),
  previewFxRevaluation: (asOf, rates) => invoke('preview_fx_revaluation', { asOf, rates }),
  postFxRevaluation: (asOf, rates) => invoke('post_fx_revaluation', { asOf, rates }),
  listFxRevaluations: () => invoke('list_fx_revaluations'),
  voidFxRevaluation: (id) => invoke('void_fx_revaluation', { id }),
  getEstimateDetail: (estimateId) => invoke('get_estimate_detail', { estimateId }),
  invoiceFromEstimate: (estimateId, input) => invoke('invoice_from_estimate', { estimateId, input }),
  createProduct: (input) => invoke('create_product', { input }),
  postSalesReceipt: (input) => invoke('post_sales_receipt', { input }),
  listAllSales: () => invoke('list_all_sales'),

  listVendors: () => invoke('list_vendors'),
  createVendor: (name, email) => invoke('create_vendor', { name, email: email || null }),
  listBills: () => invoke('list_bills'),
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
  printReport: () => invoke('print_report'),

  getStorageStatus: () => invoke('get_storage_status'),
  getStartupFailure: () => invoke('get_startup_failure'),
  openCompanyFile: (path) => invoke('open_company_file', { path }),
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
  listQbwBatches: () => invoke('list_qbw_batches'),
  getQbwBatch: (batchId) => invoke('get_qbw_batch', { batchId }),
  setQbwMapping: (batchId, rawLabel, accountId) => invoke('set_qbw_mapping', { batchId, rawLabel, accountId }),
  autoCreateQbwAccounts: (batchId) => invoke('auto_create_qbw_accounts', { batchId }),
  postQbwBatch: (input) => invoke('post_qbw_batch', { input }),
  discardQbwBatch: (batchId) => invoke('discard_qbw_batch', { batchId }),
  reverseQbwBatch: (batchId) => invoke('reverse_qbw_batch', { batchId }),
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
