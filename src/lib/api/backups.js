// Company file backup and restore.
import { invoke } from '@tauri-apps/api/core';

export default {
  exportCompanyFile: (destinationPath) => invoke('export_company_file', { destinationPath }),
  inspectCompanyFile: (sourcePath) => invoke('inspect_company_file', { sourcePath }),
  importCompanyFile: (sourcePath) => invoke('import_company_file', { sourcePath }),
  listBackups: () => invoke('list_backups'),
  backupNow: () => invoke('backup_now'),
};
