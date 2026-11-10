// Duplicate Transaction for the forms that show saved documents read-only (T28).
import { invoke } from '@tauri-apps/api/core';

export default {
  /** A saved document read as its form loads it: kind is SALES_RECEIPT, ESTIMATE, CREDIT_MEMO, VENDOR_CREDIT, PURCHASE_ORDER, DEPOSIT or TRANSFER. */
  getCopySource: (kind, id) => invoke('get_copy_source', { kind, id }),
};
