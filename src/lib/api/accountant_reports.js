// Accountant & Taxes reports: previous reconciliations, reconciliation discrepancy, income tax
// summary and detail (with each account's tax line), collections and open purchase orders.
import { invoke } from '@tauri-apps/api/core';

export default {
  listCompletedReconciliations: () => invoke('list_completed_reconciliations'),
  getPreviousReconciliationReport: (reconciliationId, detail) => invoke('get_previous_reconciliation_report', { reconciliationId, detail }),
  getReconciliationDiscrepancyReport: (accountId = null) => invoke('get_reconciliation_discrepancy_report', { accountId }),
  listTaxLines: () => invoke('list_tax_lines'),
  listAccountTaxLines: () => invoke('list_account_tax_lines'),
  setAccountTaxLine: (accountId, taxLine) => invoke('set_account_tax_line', { accountId, taxLine }),
  getIncomeTaxReport: (startDate, endDate, detail, form = null, taxLine = null) => invoke('get_income_tax_report', { startDate, endDate, detail, form, taxLine }),
  getCollectionsReport: (asOf) => invoke('get_collections_report', { asOf }),
  getOpenPurchaseOrdersReport: (asOf) => invoke('get_open_purchase_orders_report', { asOf }),
};
