// File › Utilities › Export (T26).
import { invoke } from '@tauri-apps/api/core';

export default {
  /** Asks where to save the lists as an IIF file; resolves the path, or null when cancelled. */
  exportListsIif: () => invoke('export_lists_iif'),
  /** Asks where to save names and addresses as tab-separated text; resolves the path, or null. */
  exportAddressesText: () => invoke('export_addresses_text'),
};
