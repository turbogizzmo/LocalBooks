// Payroll import: a payroll service's journal export posted as journal entries.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewPayrollImport: (path, choices) => invoke('preview_payroll_import', { path, choices }),
  postPayrollImport: (path, choices) => invoke('post_payroll_import', { path, choices }),
  listPayrollImports: () => invoke('list_payroll_imports'),
};
