// Reports, lists and registers as PDF (email or save), and Scheduled Reports.
import { invoke } from '@tauri-apps/api/core';

export default {
  // sheet: the same shape `saveXlsx` takes. email: { to, subject, body }. pdfPassword: the PDF
  // opens only with it (null: no password).
  emailReportWithSmtp: (sheet, email, pdfPassword = null) => invoke('email_report_with_smtp', { sheet, email, pdfPassword }),
  emailReportWithMailApp: (sheet, email, pdfPassword = null) => invoke('email_report_with_mail_app', { sheet, email, pdfPassword }),
  saveReportPdf: (sheet, pdfPassword = null) => invoke('save_report_pdf', { sheet, pdfPassword }),
  reportSheet: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}) => invoke('report_sheet', { report, startDate, endDate, asOf, classFilter }),
  listReportSchedules: () => invoke('list_report_schedules'),
  // schedule.protectPdf with schedule.pdfPassword (a new password; omit it to keep the saved one).
  saveReportSchedule: (schedule, id = null) => invoke('save_report_schedule', { id, schedule }),
  deleteReportSchedule: (id) => invoke('delete_report_schedule', { id }),
  runReportScheduleNow: (id) => invoke('run_report_schedule_now', { id }),
};
