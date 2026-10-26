// Customer statements.
import { invoke } from '@tauri-apps/api/core';

export default {
  getCustomerStatement: (customerId, startDate, endDate) => invoke('get_customer_statement', { customerId, startDate, endDate }),
  listStatementCustomers: (startDate, endDate) => invoke('list_statement_customers', { startDate, endDate }),
};
