// File › Print Forms batches: credit memos, sales receipts, purchase orders, bill payment stubs
// and mailing labels.
import { invoke } from '@tauri-apps/api/core';

export default {
  /** kind: 'CREDIT_MEMO', 'SALES_RECEIPT', 'PURCHASE_ORDER' or 'BILL_PAYMENT'; dates are ISO days. */
  listPrintBatch: (kind, from, to) => invoke('list_print_batch', { kind, from, to }),
  /** names: 'CUSTOMERS' or 'VENDORS'. */
  listLabelNames: (names) => invoke('list_label_names', { names }),
  /** skip: labels already used on the first sheet. */
  renderMailingLabels: (names, ids, skip = 0) => invoke('render_mailing_labels', { names, ids, skip }),
};
