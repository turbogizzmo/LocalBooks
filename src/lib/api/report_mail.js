// Reports, lists and registers as PDF (email or save), and Scheduled Reports.
import { invoke } from '@tauri-apps/api/core';

export default {
  // sheet: the same shape `saveXlsx` takes. email: { to, subject, body }.
  emailReportWithSmtp: (sheet, email) => invoke('email_report_with_smtp', { sheet, email }),
  emailReportWithMailApp: (sheet, email) => invoke('email_report_with_mail_app', { sheet, email }),
  saveReportPdf: (sheet) => invoke('save_report_pdf', { sheet }),
  reportSheet: (report, { startDate = null, endDate = null, asOf = null, classFilter = null } = {}) => invoke('report_sheet', { report, startDate, endDate, asOf, classFilter }),
  listReportSchedules: () => invoke('list_report_schedules'),
  saveReportSchedule: (schedule, id = null) => invoke('save_report_schedule', { id, schedule }),
  deleteReportSchedule: (id) => invoke('delete_report_schedule', { id }),
  runReportScheduleNow: (id) => invoke('run_report_schedule_now', { id }),
};
