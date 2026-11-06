// Where a posted transaction opens: its own form when LocalBooks has one that opens by id (bills,
// checks and expenses, invoices), otherwise the journal with the entry expanded.

/** Source types whose own form can be opened, with the screen and the id it takes. */
const FORMS = {
  BILL: ['expenses/bills', 'billId'],
  CHECK: ['expenses/expenses', 'expenseId'],
  EXPENSE: ['expenses/expenses', 'expenseId'],
  CARD_CREDIT: ['expenses/expenses', 'expenseId'],
  INVOICE: ['sales/invoices', 'invoiceId'],
};

/**
 * `[view, action, data]` to pass to `navigate` or a window opener, for a row that has
 * `journalEntryId` and, when it came from a business document, `sourceType` and `sourceId`.
 * Voided entries and their reversals open in the journal, where the void is shown.
 */
export function transactionTarget({ journalEntryId, sourceType, sourceId, isVoided = false, isReversal = false }) {
  const form = FORMS[sourceType];
  if (form && sourceId != null && !isVoided && !isReversal) return [form[0], 'edit', { [form[1]]: sourceId }];
  return ['accounting/journal', 'open-entry', { entryId: journalEntryId }];
}

/** Whether {@link transactionTarget} opens the transaction's own form rather than the journal. */
export function opensOwnForm(row) {
  return transactionTarget(row)[1] === 'edit';
}
