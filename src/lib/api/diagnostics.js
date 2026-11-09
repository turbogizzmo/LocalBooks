// Help › Save Diagnostics: a local bundle of versions, schema and the diagnostics log.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewDiagnostics: () => invoke('preview_diagnostics'),
  saveDiagnostics: () => invoke('save_diagnostics'),
};
