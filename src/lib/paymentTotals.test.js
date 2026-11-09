import { test } from 'node:test';
import assert from 'node:assert/strict';
import { paymentTotals } from './paymentTotals.js';

const rows = [
  { id: 1, totalCents: 10_000, balanceCents: 10_000 },
  { id: 2, totalCents: 5_000, balanceCents: 2_000 },
];

test('totals under the grid add the invoices, payments and discounts', () => {
  const applied = { 1: 9_800, 2: 2_000 };
  const discounts = { 1: 200 };
  const t = paymentTotals(rows, (i) => applied[i.id] ?? 0, (i) => discounts[i.id] ?? 0, 11_800);
  assert.deepEqual(t, { original: 15_000, amountDue: 12_000, applied: 11_800, discounts: 200, unapplied: 0, leftDue: 0 });
});

test('an amount received above what is applied is unapplied; nothing typed is null', () => {
  const t = paymentTotals(rows, (i) => (i.id === 1 ? 5_000 : 0), () => 0, 6_000);
  assert.equal(t.unapplied, 1_000);
  assert.equal(t.leftDue, 7_000);
  assert.equal(paymentTotals(rows, () => 0, () => 0).unapplied, null);
  // A negative typed amount (shown as an error) doesn't lower the totals.
  assert.equal(paymentTotals(rows, () => -50, () => -5).applied, 0);
});
