// Preferences batch 4: Receive Payments, Enter Bills and Pay Bills ask the company defaults.
import { invoke } from '@tauri-apps/api/core';

export default {
  getPaymentApplication: (customerName, currencyCode, amountCents, mode = null) => invoke('get_payment_application', { customerName, currencyCode: currencyCode || null, amountCents, mode }),
  getBillDueDate: (vendorName, billDate) => invoke('get_bill_due_date', { vendorName, billDate }),
  getPayBillsPlan: (billIds, paymentDate) => invoke('get_pay_bills_plan', { billIds, paymentDate }),
  setMenuViewEnabled: (view, enabled) => invoke('set_menu_view_enabled', { view, enabled }),
  payBillsWithCredits: (inputs, credits, discounts, paymentDate) => invoke('pay_bills_with_credits', { inputs, credits, discounts, paymentDate }),
};
