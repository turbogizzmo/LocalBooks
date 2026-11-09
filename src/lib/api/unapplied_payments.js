// Receive Payments with money left over: overpayments, prepayments and payments from a customer with
// no open invoice, kept as their credit, refunded or written off (docs/plans/overpayments.md).
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  // input: { payments: [PaymentInput], unapplied: { customerName, depositAccountId, paymentDate, amountCents,
  //   paymentMethod, paymentMethodId?, reference?, keepAs: 'CREDIT' | 'REFUND' | 'WRITE_OFF', accountId? } | null }
  receiveCustomerPayment: (input) => recordTransaction('receive_customer_payment', { input }),
  // input: { creditId, accountId (income), refundDate, amountCents, reference? }
  writeOffCustomerCredit: (input) => invoke('write_off_customer_credit', { input }),
  // A customer's unused credit: { unappliedCents, remainingCents }
  getCustomerCredit: (customerId) => invoke('get_customer_credit', { customerId }),
};
