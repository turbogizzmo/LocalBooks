// Purchase orders.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listPurchaseOrders: () => invoke('list_purchase_orders'),
  getPurchaseOrderLines: (id) => invoke('get_purchase_order_lines', { id }),
  createPurchaseOrder: (input) => recordTransaction('create_purchase_order', { input }),
  setPurchaseOrderClosed: (id, closed) => invoke('set_purchase_order_closed', { id, closed }),
  // Checked like Enter Bills: approval rules, and the duplicate number and date warnings to confirm.
  billPurchaseOrder: (id, billDate, dueDate, billNumber) => recordTransaction('bill_purchase_order', { id, billDate, dueDate, billNumber }),
};
