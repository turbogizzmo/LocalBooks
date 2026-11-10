// Discount and charge lines on the purchase forms. `doc` is BILL, EXPENSE, VENDOR_CREDIT or PURCHASE_ORDER.
import { invoke } from '@tauri-apps/api/core';

export default {
  getPurchaseFormLines: (doc, id) => invoke('get_purchase_form_lines', { doc, id }),
};
