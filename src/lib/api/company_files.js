// Company files: location, opening and closing, the No Company Open screen, window titles and startup failure.
import { invoke } from '@tauri-apps/api/core';

export default {
  getStorageStatus: () => invoke('get_storage_status'),
  getStartupFailure: () => invoke('get_startup_failure'),
  openCompanyFile: (path) => invoke('open_company_file', { path }),
  getStartScreen: () => invoke('get_start_screen'),
  forgetRecentCompany: (path) => invoke('forget_recent_company', { path }),
  findCompanyFiles: () => invoke('find_company_files'),
  createCompanyFile: (path, companyName, setup = null) => invoke('create_company_file', { path, companyName, setup }),
  getCompanySetupOptions: () => invoke('get_company_setup_options'),
  getStarterChart: (industry, entityType) => invoke('get_starter_chart', { industry, entityType }),
  restoreBackupAs: (sourcePath, destinationPath) => invoke('restore_backup_as', { sourcePath, destinationPath }),
  closeCompanyFile: () => invoke('close_company_file'),
  inspectDatabaseDirectory: (newPath) => invoke('inspect_database_directory', { newPath }),
  updateDatabaseDirectory: (newPath, mode) => invoke('update_database_directory', { newPath, mode: mode ?? null }),
};
