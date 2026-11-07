// Customers, products and sales receipts.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listCustomers: () => invoke('list_customers'),
  createCustomer: (name, email) => invoke('create_customer', { name, email: email || null }),
  listProducts: () => invoke('list_products'),
  createProduct: (input) => invoke('create_product', { input }),
  postSalesReceipt: (input) => recordTransaction('post_sales_receipt', { input }),
  listAllSales: () => invoke('list_all_sales'),
};
