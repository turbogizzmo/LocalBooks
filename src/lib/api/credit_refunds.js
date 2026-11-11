// A customer credit's refunds and write-offs (T73).
import { invoke } from '@tauri-apps/api/core';

export default {
  listCustomerCreditRefunds: (creditId) => invoke('list_customer_credit_refunds', { creditId }),
};
