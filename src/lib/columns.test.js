// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { arrangeColumns, layoutOf, moveColumn, toggleColumn, visibleColumns } from './columns.js';

const defs = [
  { id: 'name', label: 'Name', required: true },
  { id: 'email', label: 'Email' },
  { id: 'phone', label: 'Phone', hidden: true },
  { id: 'balance', label: 'Balance' },
];
const ids = (cols) => cols.map((c) => c.id);

test('without a choice the usual columns show in their usual order', () => {
  const a = arrangeColumns(defs, null);
  assert.deepEqual(ids(a), ['name', 'email', 'phone', 'balance']);
  assert.deepEqual(ids(visibleColumns(a)), ['name', 'email', 'balance']);
});

test('hiding, showing and moving are kept and read back', () => {
  let a = arrangeColumns(defs, null);
  a = toggleColumn(a, 'email');
  a = toggleColumn(a, 'phone');
  a = moveColumn(a, 'balance', -1);
  a = toggleColumn(a, 'name'); // required: stays
  const saved = layoutOf(a);
  assert.deepEqual(saved, { order: ['name', 'email', 'balance', 'phone'], shown: ['name', 'balance', 'phone'] });
  assert.deepEqual(ids(visibleColumns(arrangeColumns(defs, saved))), ['name', 'balance', 'phone']);
  assert.equal(moveColumn(a, 'name', -1), a, 'nothing before the first');
});

test('a column the saved choice does not know lands after its usual neighbour and shows unless hidden by default', () => {
  const saved = { order: ['balance', 'name'], shown: ['balance', 'name'] };
  const later = [...defs, { id: 'terms', label: 'Terms' }];
  const a = arrangeColumns(later, saved);
  assert.deepEqual(ids(a), ['balance', 'terms', 'name', 'email', 'phone']);
  assert.deepEqual(ids(visibleColumns(a)), ['balance', 'terms', 'name', 'email']);
  // Unknown ids in the saved choice are ignored.
  assert.deepEqual(ids(arrangeColumns(defs, { order: ['gone', 'email'], shown: ['email'] })), ['name', 'email', 'phone', 'balance']);
});
