// Recurring transactions.
import { invoke } from '@tauri-apps/api/core';

export default {
  listRecurring: () => invoke('list_recurring'),
  saveRecurring: (id, input) => invoke('save_recurring', { id, input }),
  setRecurringActive: (id, active) => invoke('set_recurring_active', { id, active }),
  deleteRecurring: (id) => invoke('delete_recurring', { id }),
  enterRecurring: (asOf = null, only = null) => invoke('enter_recurring', { asOf, only }),
};
