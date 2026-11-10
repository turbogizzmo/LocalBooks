// Backup safety (gap R8): a copy off this computer and a test restore.
import { invoke } from '@tauri-apps/api/core';

export default {
  /** { lastOffDisk, lastTestRestore }: local date-times, or null. */
  getBackupSafety: () => invoke('get_backup_safety'),
  /** Opens a backup read-only and checks it as a restore would; the books are not replaced. */
  testRestoreBackup: (path) => invoke('test_restore_backup', { path }),
};
