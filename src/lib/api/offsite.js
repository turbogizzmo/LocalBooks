// Offsite backup: encrypted copies in a folder the user picks, test restores and restores.
import { invoke } from '@tauri-apps/api/core';

export default {
  getOffsiteStatus: () => invoke('get_offsite_status'),
  setUpOffsiteBackup: (folder, password, keep) => invoke('set_up_offsite_backup', { folder, password, keep }),
  turnOffOffsiteBackup: () => invoke('turn_off_offsite_backup'),
  offsiteBackupNow: () => invoke('offsite_backup_now'),
  testOffsiteRestore: (path, password) => invoke('test_offsite_restore', { path, password }),
  restoreOffsiteBackup: (path, password) => invoke('restore_offsite_backup', { path, password }),
};
