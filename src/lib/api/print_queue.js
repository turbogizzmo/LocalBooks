// Print later: the check and invoice queues behind File › Print Forms.
import { invoke } from '@tauri-apps/api/core';

export default {
  listChecksToPrint: (accountId = null) => invoke('list_checks_to_print', { accountId }),
  listInvoicesToPrint: () => invoke('list_invoices_to_print'),
  /** kind: 'CHECK' or 'INVOICE'. */
  setToBePrinted: (kind, id, queued) => invoke('set_to_be_printed', { kind, id, queued }),
  /** printed: [{ expenseId, checkNumber }] for the checks that came out right. */
  markChecksPrinted: (printed) => invoke('mark_checks_printed', { printed }),
  markInvoicesPrinted: (invoiceIds) => invoke('mark_invoices_printed', { invoiceIds }),
  setPrintLaterDefault: (kind, on) => invoke('set_print_later_default', { kind, on }),
};
