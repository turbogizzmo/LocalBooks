// Sales tax adjustments (T70): raise or lower what is owed on a rate.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  adjustSalesTax: (input) => recordTransaction('adjust_sales_tax', { input }),
  listSalesTaxAdjustments: () => invoke('list_sales_tax_adjustments'),
};
