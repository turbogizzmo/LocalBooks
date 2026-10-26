// Chart of accounts.
import { invoke } from '@tauri-apps/api/core';

export default {
  listAccounts: () => invoke('list_accounts'),
  listAccountTypes: () => invoke('list_account_types'),
  createAccount: (input) => invoke('create_account', { input }),
  updateAccount: (id, input) => invoke('update_account', { id, input }),
  setAccountActive: (id, active) => invoke('set_account_active', { id, active }),
};
