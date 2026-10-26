// Custom reports: the report builder, its presets and saved (memorized) reports.
import { invoke } from '@tauri-apps/api/core';

export default {
  getCustomReport: (settings) => invoke('get_custom_report', { settings }),
  listReportPresets: () => invoke('list_report_presets'),
  listSavedReports: () => invoke('list_saved_reports'),
  saveCustomReport: (name, preset, settings, replace = false) => invoke('save_custom_report', { name, preset, settings, replace }),
  deleteSavedReport: (id) => invoke('delete_saved_report', { id }),
};
