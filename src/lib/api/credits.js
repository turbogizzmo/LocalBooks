// Customer credit memos and vendor credits.
import { invoke } from '@tauri-apps/api/core';

export default {
  listCreditMemos: () => invoke('list_credit_memos'),
  createCreditMemo: (input) => invoke('create_credit_memo', { input }),
  applyCustomerCredit: (input) => invoke('apply_customer_credit', { input }),
  listCustomerCreditApplications: (creditId) => invoke('list_customer_credit_applications', { creditId }),
  removeCustomerCredit: (applicationId) => invoke('remove_customer_credit', { applicationId }),
  refundCustomerCredit: (input) => invoke('refund_customer_credit', { input }),
  listVendorCredits: () => invoke('list_vendor_credits'),
  createVendorCredit: (input) => invoke('create_vendor_credit', { input }),
  applyVendorCredit: (input) => invoke('apply_vendor_credit', { input }),
  listVendorCreditApplications: (creditId) => invoke('list_vendor_credit_applications', { creditId }),
  removeVendorCredit: (applicationId) => invoke('remove_vendor_credit', { applicationId }),
  refundVendorCredit: (input) => invoke('refund_vendor_credit', { input }),
};
