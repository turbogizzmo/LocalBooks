// Invoices, customer payments and A/R aging.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listInvoices: () => invoke('list_invoices'),
  createInvoice: (input) => recordTransaction('create_invoice', { input }),
  recordInvoicePayment: (input) => recordTransaction('record_invoice_payment', { input }),
  listInvoicePayments: (invoiceId) => invoke('list_invoice_payments', { invoiceId }),
  getArAging: () => invoke('get_ar_aging', { asOf: null }),
  recordInvoicePayments: (inputs) => recordTransaction('record_invoice_payments', { inputs }),
  getInvoiceDetail: (invoiceId) => invoke('get_invoice_detail', { invoiceId }),
};
