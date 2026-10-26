// Inventory.
import { invoke } from '@tauri-apps/api/core';

export default {
  listInventoryItems: () => invoke('list_inventory_items'),
  trackInventory: (input) => invoke('track_inventory', { input }),
  adjustInventory: (input) => invoke('adjust_inventory', { input }),
  listStockMovements: (itemId) => invoke('list_stock_movements', { itemId }),
};
