// Sales orders: non-posting orders invoiced whole or in parts, and the open-orders reports.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listSalesOrders: (openOnly = false) => invoke('list_sales_orders', { openOnly }),
  getSalesOrder: (id) => invoke('get_sales_order', { id }),
  createSalesOrder: (input) => invoke('create_sales_order', { input }),
  updateSalesOrder: (id, input) => invoke('update_sales_order', { id, input }),
  closeSalesOrder: (id) => invoke('close_sales_order', { id }),
  reopenSalesOrder: (id) => invoke('reopen_sales_order', { id }),
  deleteSalesOrder: (id) => invoke('delete_sales_order', { id }),
  invoiceFromSalesOrder: (id, input, takes) => recordTransaction('invoice_from_sales_order', { id, input, takes }),
  getOpenSalesOrders: () => invoke('get_open_sales_orders'),
  getPendingSales: () => invoke('get_pending_sales'),
};
