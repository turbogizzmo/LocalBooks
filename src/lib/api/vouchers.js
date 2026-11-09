// Vouchers (bills, checks, expenses, credits, deposits, journal entries) as PDF: saved or emailed.
import { invoke } from '@tauri-apps/api/core';

export default {
  saveVoucherPdf: (doc) => invoke('save_voucher_pdf', { doc }),
  getVoucherEmailDraft: (doc) => invoke('get_voucher_email_draft', { doc }),
  emailVoucherWithMailApp: (doc, email) => invoke('email_voucher_with_mail_app', { doc, email }),
  emailVoucherWithSmtp: (doc, email) => invoke('email_voucher_with_smtp', { doc, email }),
};
