// Customers › Create Batch Invoices, and payment reminders by customer group.
import { invoke } from '@tauri-apps/api/core';

export default {
  // input: { groupId?, customerIds?, issueDate, dueDays?, lines: [{ productServiceId?, description, quantityE4, rateCents, incomeAccountId, classId?, isTaxable? }] }
  createBatchInvoices: (input) => invoke('create_batch_invoices', { input }),
  groupOverdueInvoices: (groupId) => invoke('group_overdue_invoices', { groupId }),
  sendPaymentReminders: (groupId, invoiceIds) => invoke('send_payment_reminders', { groupId, invoiceIds }),
};
