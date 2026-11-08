// Customer credit memos and vendor credits.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  listCreditMemos: () => invoke('list_credit_memos'),
  createCreditMemo: (input) => recordTransaction('create_credit_memo', { input }),
  applyCustomerCredit: (input) => invoke('apply_customer_credit', { input }),
  // Applies the customer's open credits, oldest first, to one invoice (the invoice form's offer).
  applyOpenCreditsToInvoice: (invoiceId, appliedDate) => invoke('apply_open_credits_to_invoice', { invoiceId, appliedDate }),
  listCustomerCreditApplications: (creditId) => invoke('list_customer_credit_applications', { creditId }),
  removeCustomerCredit: (applicationId) => invoke('remove_customer_credit', { applicationId }),
  refundCustomerCredit: (input) => invoke('refund_customer_credit', { input }),
  listVendorCredits: () => invoke('list_vendor_credits'),
  createVendorCredit: (input) => recordTransaction('create_vendor_credit', { input }),
  applyVendorCredit: (input) => invoke('apply_vendor_credit', { input }),
  listVendorCreditApplications: (creditId) => invoke('list_vendor_credit_applications', { creditId }),
  removeVendorCredit: (applicationId) => invoke('remove_vendor_credit', { applicationId }),
  refundVendorCredit: (input) => invoke('refund_vendor_credit', { input }),
};
