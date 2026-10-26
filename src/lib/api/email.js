// Emailing forms (Mail app and SMTP) and the email log.
import { invoke } from '@tauri-apps/api/core';

export default {
  // `period` is for statements ({ startDate, endDate }).
  getEmailDraft: (kind, documentId, period = null) => invoke('get_email_draft', { kind, documentId, period }),
  emailFormWithMailApp: (kind, documentId, email, period = null) => invoke('email_form_with_mail_app', { kind, documentId, period, email }),
  emailFormWithSmtp: (kind, documentId, email, period = null) => invoke('email_form_with_smtp', { kind, documentId, period, email }),
  getSmtpSettings: () => invoke('get_smtp_settings'),
  sendSmtpTestEmail: (settings, password = null) => invoke('send_smtp_test_email', { settings, password }),
  getEmailHistory: (kind, documentId) => invoke('get_email_history', { kind, documentId }),
  listEmailStatuses: (kind) => invoke('list_email_statuses', { kind }),
  setEmailLater: (kind, documentId, queued) => invoke('set_email_later', { kind, documentId, queued }),
  listEmailQueue: () => invoke('list_email_queue'),
  saveSmtpSettings: (settings, password = null) => invoke('save_smtp_settings', { settings, password }),
  testSmtpSettings: (settings, password = null) => invoke('test_smtp_settings', { settings, password }),
};
