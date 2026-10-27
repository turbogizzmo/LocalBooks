// Report export (CSV, Excel, save, print).
import { invoke } from '@tauri-apps/api/core';

export default {
  reportCsv: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}) =>
    invoke('report_csv', { report, startDate, endDate, asOf, classFilter }),
  saveReportToDisk: (reportHtmlOrMatrix, filename) => invoke('save_report_to_disk', { reportHtmlOrMatrix, filename }),
  saveReportXlsx: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}, filename) =>
    invoke('save_report_xlsx', { report, startDate, endDate, asOf, classFilter, filename }),
  saveXlsx: (sheet, filename) => invoke('save_xlsx', { sheet, filename }),
  savePng: (pngBase64, filename) => invoke('save_png', { pngBase64, filename }),
  printReport: (title = null) => invoke('print_report', { title }),
  finishPrinting: () => invoke('finish_printing'),
};
