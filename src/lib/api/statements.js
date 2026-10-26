// Dashboard and financial statements.
import { invoke } from '@tauri-apps/api/core';

export default {
  getDashboard: () => invoke('get_dashboard'),
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
};
