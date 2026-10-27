// Export from an installed QuickBooks (Preview): LocalBooks asks the QuickBooks on this computer for its data.
import { invoke } from '@tauri-apps/api/core';

export default {
  inspectInstalledQuickbooks: () => invoke('inspect_installed_quickbooks'),
  exportFromInstalledQuickbooks: () => invoke('export_from_installed_quickbooks'),
};
