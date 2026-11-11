import { test } from 'node:test';
import assert from 'node:assert/strict';
import { billSource, expenseSource, invoiceDocument, journalSource, templateInput } from './memorizedEdit.js';

test('a journal payload loads as the journal form source', () => {
  const src = journalSource({ date: '2026-01-31', description: 'Accrual', lines: [{ account_id: 7, debit: 500, credit: 0, memo: null, class_id: 3 }, { account_id: 9, debit: 0, credit: 500, memo: 'x', class_id: null }] });
  assert.equal(src.entry.memo, 'Accrual');
  assert.deepEqual(src.lines[0], { accountId: 7, debitCents: 500, creditCents: 0, memo: '', classId: 3 });
  assert.equal(src.lines[1].creditCents, 500);
});

test('expense and bill payloads keep their lines and discount lines', () => {
  const e = expenseSource({ kind: 'CHECK', payeeName: 'Landlord', paymentAccountId: 1, lines: [{ accountId: 6, amountCents: 100 }], formLines: [{ afterLine: 1, kind: 'DISCOUNT' }], currency: null });
  assert.equal(e.expense.payeeName, 'Landlord');
  assert.equal(e.lines.length, 1);
  assert.equal(e.formLines.length, 1);
  const b = billSource({ vendorName: 'Staples', billDate: '2026-01-01', dueDate: '2026-01-31', totalCents: 900, lines: [], termsId: 4 });
  assert.deepEqual([b.bill.vendorName, b.bill.totalCents, b.bill.termsId, b.bill.id], ['Staples', 900, 4, null]);
});

test('an invoice payload numbers its items and form lines in order', () => {
  const doc = invoiceDocument({
    lines: [{ description: 'Chairs', quantityE4: 10_000, rateCents: 10_000, incomeAccountId: 4 }, { description: 'Delivery', quantityE4: 10_000, rateCents: 2_000, incomeAccountId: 4 }],
    formLines: [
      { afterLines: 1, kind: 'CHARGE', description: 'Restocking', percentPpm: 50_000, accountId: 4 },
      { afterLines: 2, kind: 'SUBTOTAL', description: '' },
      { afterLines: 2, kind: 'DISCOUNT', description: 'Off', amountCents: 500, accountId: 4 },
    ],
  });
  assert.deepEqual(doc.items.map((i) => [i.lineNumber, i.description, i.percentPpm]), [[1, 'Chairs', null], [2, 'Restocking', 50_000], [3, 'Delivery', null]]);
  assert.deepEqual(doc.storedForm.map((f) => [f.lineNumber, f.kind, f.amountCents]), [[4, 'SUBTOTAL', 0], [5, 'DISCOUNT', -500]]);
});

test('the template keeps its name and schedule', () => {
  const t = { id: 3, name: 'Rent', kind: 'EXPENSE', frequency: 'MONTHLY', every: 1, nextDate: '2026-03-31', endDate: null };
  assert.deepEqual(templateInput(t, { a: 1 }), { name: 'Rent', kind: 'EXPENSE', payload: { a: 1 }, frequency: 'MONTHLY', every: 1, nextDate: '2026-03-31', endDate: null });
});
