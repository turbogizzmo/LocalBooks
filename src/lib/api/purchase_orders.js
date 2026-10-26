// Purchase orders.
import { invoke } from '@tauri-apps/api/core';

export default {
  listPurchaseOrders: () => invoke('list_purchase_orders'),
  getPurchaseOrderLines: (id) => invoke('get_purchase_order_lines', { id }),
  createPurchaseOrder: (input) => invoke('create_purchase_order', { input }),
  setPurchaseOrderClosed: (id, closed) => invoke('set_purchase_order_closed', { id, closed }),
  billPurchaseOrder: (id, billDate, dueDate, billNumber) => invoke('bill_purchase_order', { id, billDate, dueDate, billNumber }),
};
