// Invoices, customer payments and A/R aging.
import { invoke } from '@tauri-apps/api/core';

export default {
  listInvoices: () => invoke('list_invoices'),
  createInvoice: (input) => invoke('create_invoice', { input }),
  recordInvoicePayment: (input) => invoke('record_invoice_payment', { input }),
  listInvoicePayments: (invoiceId) => invoke('list_invoice_payments', { invoiceId }),
  getArAging: () => invoke('get_ar_aging', { asOf: null }),
  recordInvoicePayments: (inputs) => invoke('record_invoice_payments', { inputs }),
  getInvoiceDetail: (invoiceId) => invoke('get_invoice_detail', { invoiceId }),
};
