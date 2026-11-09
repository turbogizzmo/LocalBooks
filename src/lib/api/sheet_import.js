// Importing a spreadsheet the user laid out: customers, vendors, sales and purchase transactions, or journal entries.
import { invoke } from '@tauri-apps/api/core';

export default {
  readImportSheet: (filePath, kind) => invoke('read_import_sheet', { filePath, kind }),
  checkImportSheet: (filePath, kind, mapping, dateOrder) => invoke('check_import_sheet', { filePath, kind, mapping, dateOrder }),
  stageImportSheet: (filePath, kind, mapping, dateOrder) => invoke('stage_import_sheet', { filePath, kind, mapping, dateOrder }),
  saveImportTemplate: (kind) => invoke('save_import_template', { kind }),
};
