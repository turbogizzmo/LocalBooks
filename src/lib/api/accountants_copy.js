// Accountant's Copy: making the copy, the accountant's changes file, and bringing it back.
import { invoke } from '@tauri-apps/api/core';

export default {
  getAccountantsCopyState: () => invoke('get_accountants_copy_state'),
  createAccountantsCopy: (dividingDate, destination) => invoke('create_accountants_copy', { dividingDate, destination }),
  cancelAccountantsCopy: () => invoke('cancel_accountants_copy'),
  exportAccountantChanges: (destination) => invoke('export_accountant_changes', { destination }),
  previewAccountantChanges: (path) => invoke('preview_accountant_changes', { path }),
  importAccountantChanges: (path) => invoke('import_accountant_changes', { path }),
};
