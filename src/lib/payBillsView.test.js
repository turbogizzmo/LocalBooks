import { test } from 'node:test';
import assert from 'node:assert/strict';
import { payBillsVendors, payBillsView } from './payBillsView.js';

const bills = [
  { id: 1, vendorId: 10, vendorName: 'Zeta Supply', dueDate: '2026-12-01', discountUntil: null, balanceCents: 5_000 },
  { id: 2, vendorId: 11, vendorName: 'Acme Paper', dueDate: '2026-11-20', discountUntil: '2026-11-15', balanceCents: 20_000 },
  { id: 3, vendorId: 10, vendorName: 'Zeta Supply', dueDate: '2026-11-20', discountUntil: '2026-11-10', balanceCents: 1_000 },
];
const ids = (rows) => rows.map((b) => b.id);

test('Pay Bills sorts by due date by default, ties by vendor', () => {
  assert.deepEqual(ids(payBillsView(bills)), [2, 3, 1]);
});

test('Sort By discount date, vendor and amount due', () => {
  assert.deepEqual(ids(payBillsView(bills, { sortBy: 'discount' })), [3, 2, 1]);
  assert.deepEqual(ids(payBillsView(bills, { sortBy: 'vendor' })), [2, 3, 1]);
  assert.deepEqual(ids(payBillsView(bills, { sortBy: 'amount' })), [2, 1, 3]);
});

test('Filter By shows one vendor’s bills', () => {
  assert.deepEqual(ids(payBillsView(bills, { vendorId: 10 })), [3, 1]);
  assert.deepEqual(payBillsVendors(bills), [{ id: 11, name: 'Acme Paper' }, { id: 10, name: 'Zeta Supply' }]);
});
