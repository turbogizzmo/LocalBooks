// Purchase orders.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listPurchaseOrders: () => invoke('list_purchase_orders'),
  getPurchaseOrderLines: (id) => invoke('get_purchase_order_lines', { id }),
  createPurchaseOrder: (input) => recordTransaction('create_purchase_order', { input }),
  // An open order with nothing received or billed; it keeps its number.
  updatePurchaseOrder: (id, input) => recordTransaction('update_purchase_order', { id, input }),
  deletePurchaseOrder: (id) => invoke('delete_purchase_order', { id }),
  setPurchaseOrderClosed: (id, closed) => invoke('set_purchase_order_closed', { id, closed }),
  billPurchaseOrder: (id, billDate, dueDate, billNumber) => invoke('bill_purchase_order', { id, billDate, dueDate, billNumber }),
};
