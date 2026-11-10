// The rest of the item record and list: purchase description and cost, making several items active
// or inactive, and Add/Edit Multiple Items from pasted rows.
import { invoke } from '@tauri-apps/api/core';

export default {
  setItemPurchaseDetails: (productId, description, costCents) => invoke('set_item_purchase_details', { productId, description, costCents }),
  setItemsActive: (ids, active) => invoke('set_items_active', { ids, active }),
  // rows: [{ name, description?, rate?, incomeAccount?, purchaseDescription?, cost? }] → { added, updated, problems }
  pasteItems: (rows) => invoke('paste_items', { rows }),
};
