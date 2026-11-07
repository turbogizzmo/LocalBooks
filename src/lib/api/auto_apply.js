// Applying payments and vendor credits automatically (Preferences › Payments and Bills).
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  /** How an amount received applies to the customer's open invoices: `[{documentId, amountCents}]`, empty when turned off. */
  autoApplyCustomerPayment: (customerName, currencyCode, amountCents, onRequest = false) => invoke('auto_apply_customer_payment', { customerName, currencyCode, amountCents, onRequest }),
  /** Pays bills, using the vendors' open credits first when the preference is on: `{payments, creditsCents}`. */
  payBillsApplyingCredits: (inputs) => recordTransaction('pay_bills_applying_credits', { inputs }),
};
