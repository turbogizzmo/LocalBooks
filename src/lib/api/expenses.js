// Vendors, bills, bill payments and expenses.
import { invoke } from '@tauri-apps/api/core';

export default {
  listVendors: () => invoke('list_vendors'),
  createVendor: (name, email) => invoke('create_vendor', { name, email: email || null }),
  listBills: () => invoke('list_bills'),
  postVendorBill: (input) => invoke('post_vendor_bill', { input }),
  payVendorBill: (input) => invoke('pay_vendor_bill', { input }),
  payVendorBills: (inputs) => invoke('pay_vendor_bills', { inputs }),
  listBillPayments: (billId) => invoke('list_bill_payments', { billId }),
  nextCheckNumber: (accountId) => invoke('next_check_number', { accountId }),
  postExpense: (input) => invoke('post_expense', { input }),
  listExpenses: () => invoke('list_expenses'),
};
