// Reports outside the report builder (list reports, statement summaries, time, purchase orders,
// inventory worksheet, audit trail) and saving several reports as one workbook.
import { invoke } from '@tauri-apps/api/core';

export default {
  listListReports: () => invoke('list_list_reports'),
  getListReport: (id, startDate = null, endDate = null) => invoke('get_list_report', { id, startDate, endDate }),
  saveXlsxBook: (sheets, filename) => invoke('save_xlsx_book', { sheets, filename }),
};
