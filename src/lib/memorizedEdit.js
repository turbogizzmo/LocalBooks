// Edit transaction… on Recurring Transactions (gaps row T72): a memorized transaction's payload (the
// input its form posts) turned back into what each form loads, and the changed payload saved back to
// the template. Copies already entered are posted transactions and don't change.

/** Journal Entries' form source ({ entry, lines }) from a JOURNAL payload. */
export function journalSource(payload) {
  return {
    entry: { memo: payload.description ?? '', isAdjusting: false },
    lines: (payload.lines ?? []).map((l) => ({ accountId: l.account_id, debitCents: Number(l.debit ?? 0), creditCents: Number(l.credit ?? 0), memo: l.memo ?? '', classId: l.class_id ?? null })),
  };
}

/** The expense form's source ({ expense, lines, formLines }) from an EXPENSE payload. */
export function expenseSource(payload) {
  return {
    expense: {
      payeeName: payload.payeeName ?? '',
      paymentAccountId: payload.paymentAccountId ?? null,
      paymentMethod: payload.paymentMethod ?? null,
      paymentMethodId: payload.paymentMethodId ?? null,
      memo: payload.memo ?? '',
      currencyCode: payload.currency?.code ?? null,
      exchangeRateE6: payload.currency?.exchangeRateE6 ?? null,
    },
    lines: payload.lines ?? [],
    formLines: payload.formLines ?? [],
  };
}

/** The bill form's source ({ bill, lines, formLines }) from a BILL payload. */
export function billSource(payload) {
  return {
    bill: { id: null, vendorName: payload.vendorName ?? '', billDate: payload.billDate, dueDate: payload.dueDate, termsId: payload.termsId ?? null, totalCents: payload.totalCents ?? 0 },
    lines: payload.lines ?? [],
    formLines: payload.formLines ?? [],
  };
}

/**
 * An INVOICE payload as the invoice form loads a saved invoice: its item lines and form lines
 * numbered in one sequence (`items`, `storedForm`), placing each form line after the item lines it
 * follows (`afterLines`). A percent charge is stored as an item; a discount is stored negative.
 */
export function invoiceDocument(payload) {
  const lines = payload.lines ?? [];
  const form = payload.formLines ?? [];
  const items = [];
  const storedForm = [];
  let n = 0;
  const placeForm = (after) => {
    for (const f of form.filter((x) => (x.afterLines ?? 0) === after)) {
      n += 1;
      if (f.kind === 'CHARGE') {
        items.push({ lineNumber: n, description: f.description ?? '', percentPpm: f.percentPpm, incomeAccountId: f.accountId, productServiceId: f.productServiceId ?? null, classId: f.classId ?? null, isTaxable: Boolean(f.isTaxable) });
      } else {
        storedForm.push({
          lineNumber: n,
          kind: f.kind,
          description: f.description ?? '',
          productServiceId: f.productServiceId ?? null,
          percentPpm: f.percentPpm ?? null,
          amountCents: f.kind === 'DISCOUNT' && f.amountCents ? -f.amountCents : 0,
          accountId: f.accountId ?? null,
          classId: f.classId ?? null,
          isTaxable: Boolean(f.isTaxable),
          itemGroupId: f.itemGroupId ?? null,
          groupLineCount: f.groupLineCount ?? null,
          groupQuantityE4: f.groupQuantityE4 ?? null,
          printItems: f.printItems ?? true,
        });
      }
    }
  };
  lines.forEach((l, i) => {
    placeForm(i);
    n += 1;
    items.push({ ...l, lineNumber: n, percentPpm: null });
  });
  placeForm(lines.length);
  return { items, storedForm };
}

/** The template saved back with its changed transaction: same name and schedule. */
export function templateInput(template, payload) {
  return { name: template.name, kind: template.kind, payload, frequency: template.frequency, every: template.every, nextDate: template.nextDate, endDate: template.endDate ?? null };
}

/** Which screen edits a memorized transaction of `kind`. */
export const MEMORIZED_VIEW = { JOURNAL: 'accounting/journal', EXPENSE: 'expenses/expenses', BILL: 'expenses/bills', INVOICE: 'sales/invoices' };
