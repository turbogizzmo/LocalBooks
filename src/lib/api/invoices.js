// Invoices, customer payments and A/R aging.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listInvoices: () => invoke('list_invoices'),
  /** `paid`: the payment taken with the invoice (T21), saved with it; null for none. */
  // `billable`: costs and time chosen with Add billable costs… (`{ costs, time, markupPercent }`),
  // added after the form's lines and marked billed by this invoice.
  createInvoice: (input, toBePrinted = false, paid = null, billable = null) => recordTransaction('create_invoice', { input, toBePrinted, paid, billable }),
  recordInvoicePayment: (input) => recordTransaction('record_invoice_payment', { input }),
  listInvoicePayments: (invoiceId) => invoke('list_invoice_payments', { invoiceId }),
  getArAging: () => invoke('get_ar_aging', { asOf: null }),
  recordInvoicePayments: (inputs) => recordTransaction('record_invoice_payments', { inputs }),
  getInvoiceDetail: (invoiceId) => invoke('get_invoice_detail', { invoiceId }),
};
