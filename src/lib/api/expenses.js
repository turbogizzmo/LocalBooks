// Vendors, bills, bill payments and expenses.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listVendors: () => invoke('list_vendors'),
  createVendor: (name, email) => invoke('create_vendor', { name, email: email || null }),
  listBills: () => invoke('list_bills'),
  postVendorBill: (input) => recordTransaction('post_vendor_bill', { input }),
  payVendorBill: (input) => recordTransaction('pay_vendor_bill', { input }),
  payVendorBills: (inputs) => recordTransaction('pay_vendor_bills', { inputs }),
  listBillPayments: (billId) => invoke('list_bill_payments', { billId }),
  nextCheckNumber: (accountId) => invoke('next_check_number', { accountId }),
  postExpense: (input, toBePrinted = false) => recordTransaction('post_expense', { input, toBePrinted }),
  listExpenses: () => invoke('list_expenses'),
};
