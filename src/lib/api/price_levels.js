// Price levels and the Item Price List.
import { invoke } from '@tauri-apps/api/core';

export default {
  listPriceLevels: () => invoke('list_price_levels'),
  previewPriceLevel: (input) => invoke('preview_price_level', { input }),
  savePriceLevel: (id, input) => invoke('save_price_level', { id, input }),
  setPriceLevelActive: (id, active) => invoke('set_price_level_active', { id, active }),
  deletePriceLevel: (id) => invoke('delete_price_level', { id }),
  getItemPriceList: () => invoke('get_item_price_list'),
  listPriceLevelChanges: () => invoke('list_price_level_changes'),
};
