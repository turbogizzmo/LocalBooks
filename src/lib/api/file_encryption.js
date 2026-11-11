// Whole-file encryption of the company file (Settings › Users & Passwords, Admins).
import { invoke } from '@tauri-apps/api/core';

export default {
  /** input: { password, temporaryPasswords: [{ userId, password }], understood } → { user, recoveryKey } */
  encryptCompanyFile: (input) => invoke('encrypt_company_file', { input }),
  decryptCompanyFile: (password) => invoke('decrypt_company_file', { password }),
  listUnencryptedBackups: () => invoke('list_unencrypted_backups'),
  deleteUnencryptedBackups: () => invoke('delete_unencrypted_backups'),
};
