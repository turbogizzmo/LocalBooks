// Customer groups (Customer Center › Groups): manual or kept up to date by rules, with a statement reminder.
import { invoke } from '@tauri-apps/api/core';

export default {
  listCustomerGroups: () => invoke('list_customer_groups'),
  // input: { name, rules: [{ field, ... }], memberIds: [], statement: { everyDays, lastSent, openOnly, styleId, message } }
  saveCustomerGroup: (input, id = null) => invoke('save_customer_group', { id, input }),
  deleteCustomerGroup: (id) => invoke('delete_customer_group', { id }),
  markGroupStatementsSent: (id, date = null) => invoke('mark_group_statements_sent', { id, date }),
};
