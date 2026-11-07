// The sales form header: Bill To / Ship To defaults and the credit-limit check.
import { invoke } from '@tauri-apps/api/core';

export default {
  /** { billTo, shipTo, creditLimitCents } a new form for this customer starts with. */
  customerSalesDefaults: (customerId) => invoke('customer_sales_defaults', { customerId }),
  /** A warning when an invoice of `amountCents` (home currency) would go over the customer's credit limit, else null. */
  checkCreditLimit: (customerName, amountCents, replacingInvoiceId = null) =>
    invoke('check_credit_limit', { customerName, amountCents, replacingInvoiceId }),
};
