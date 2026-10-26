// Change Item Prices.
import { invoke } from '@tauri-apps/api/core';

export default {
  listPriceItems: () => invoke('list_price_items'),
  previewItemPrices: (input) => invoke('preview_item_prices', { input }),
  applyItemPrices: (input) => invoke('apply_item_prices', { input }),
  listPriceChanges: () => invoke('list_price_changes'),
};
