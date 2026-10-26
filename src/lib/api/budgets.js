// Budgets.
import { invoke } from '@tauri-apps/api/core';

export default {
  listBudgets: () => invoke('list_budgets'),
  createBudget: (name, inYear) => invoke('create_budget', { name, inYear }),
  deleteBudget: (id) => invoke('delete_budget', { id }),
  getBudgetAmounts: (id) => invoke('get_budget_amounts', { id }),
  setBudgetAmounts: (id, amounts) => invoke('set_budget_amounts', { id, amounts }),
  getBudgetVsActual: (id, fromPeriod, toPeriod) => invoke('get_budget_vs_actual', { id, fromPeriod, toPeriod }),
  getBalanceSheetBudgetVsActual: (id, period) => invoke('get_balance_sheet_budget_vs_actual', { id, period }),
};
