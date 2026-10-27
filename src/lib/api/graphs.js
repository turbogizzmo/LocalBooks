// Graph reports (Reports › Graphs).
import { invoke } from '@tauri-apps/api/core';

export default {
  getIncomeExpenseGraph: (startDate, endDate, basis) => invoke('get_income_expense_graph', { startDate, endDate, basis }),
  getNetWorthGraph: (startDate, endDate) => invoke('get_net_worth_graph', { startDate, endDate }),
  getAgingGraph: (payable, asOf) => invoke('get_aging_graph', { payable, asOf }),
  getSalesGraph: (by, startDate, endDate) => invoke('get_sales_graph', { by, startDate, endDate }),
  getBudgetGraph: (id, fromPeriod, toPeriod) => invoke('get_budget_graph', { id, fromPeriod, toPeriod }),
};
