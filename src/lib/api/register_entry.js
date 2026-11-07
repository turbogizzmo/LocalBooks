// Typing transactions into a register, and a transaction's history.
import { invoke } from '@tauri-apps/api/core';

export default {
  postRegisterEntry: (input) => invoke('post_register_entry', { input }),
  getTransactionHistory: (journalEntryId) => invoke('get_transaction_history', { journalEntryId }),
};
