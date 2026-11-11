// Recurring transactions.
import { invoke } from '@tauri-apps/api/core';

export default {
  listRecurring: () => invoke('list_recurring'),
  saveRecurring: (id, input) => invoke('save_recurring', { id, input }),
  setRecurringActive: (id, active) => invoke('set_recurring_active', { id, active }),
  deleteRecurring: (id) => invoke('delete_recurring', { id }),
  // Returns { entered, skipped: [{ templateId, name, date, reason }] }.
  enterRecurring: (asOf = null, only = null) => invoke('enter_recurring', { asOf, only }),
  editRecurringSchedule: (id, input) => invoke('edit_recurring_schedule', { id, input }),
  // A memorized transaction with the transaction it enters, for Edit transaction….
  getRecurring: (id) => invoke('get_recurring', { id }),
};
