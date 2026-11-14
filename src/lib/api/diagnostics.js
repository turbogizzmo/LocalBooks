// Help › Save Diagnostics: a local bundle of versions, schema and the diagnostics log.
import { invoke } from '@tauri-apps/api/core';

export default {
  previewDiagnostics: () => invoke('preview_diagnostics'),
  saveDiagnostics: () => invoke('save_diagnostics'),
  // A screen that failed to draw (ErrorBoundary.jsx): one line, with no business data.
  logScreenError: (line) => invoke('log_screen_error', { line }),
};
