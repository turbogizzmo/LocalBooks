// Preferences batch 5 (tax and time): sales tax periods, timesheet weeks, billable expenses,
// recalling a vendor's last transaction, 1099 boxes and vendor account numbers.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  getSalesTaxPeriods: (asOf) => invoke('get_sales_tax_periods', { asOf }),
  getTimesheetWeek: (date) => invoke('get_timesheet_week', { date }),
  listUnbilledCosts: (customerId = null) => invoke('list_unbilled_costs', { customerId }),
  getBillingDefaults: () => invoke('get_billing_defaults'),
  billCosts: (input) => recordTransaction('bill_costs', { input }),
  getRecalledVendorTransaction: (vendorName, kind = null) => invoke('get_recalled_vendor_transaction', { vendorName, kind }),
  list1099Boxes: () => invoke('list_1099_boxes'),
  getVendorAccountNumber: (vendorId) => invoke('get_vendor_account_number', { vendorId }),
  setVendorAccountNumber: (vendorId, accountNumber) => invoke('set_vendor_account_number', { vendorId, accountNumber }),
};
