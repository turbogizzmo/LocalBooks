// Sales form numbers: the next invoice, estimate, sales receipt, credit memo or sales order number,
// and whether a typed one is free. `kind`: INVOICE, ESTIMATE, SALES_RECEIPT, CREDIT_MEMO, SALES_ORDER.
import { invoke } from '@tauri-apps/api/core';

export default {
  getNextDocumentNumber: (kind) => invoke('get_next_document_number', { kind }),
  /** Why `number` can't be used (already used, out of range), or null when it is free. */
  getDocumentNumberProblem: (kind, number) => invoke('get_document_number_problem', { kind, number }),
};
