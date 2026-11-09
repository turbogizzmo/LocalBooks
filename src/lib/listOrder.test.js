// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { arrange, moveRow, parentByName, parentIds, visibleRows } from './listOrder.js';

const accounts = [
  { id: 1, parentId: null, name: 'Checking' },
  { id: 2, parentId: null, name: 'Savings' },
  { id: 3, parentId: 1, name: 'Payroll' },
  { id: 4, parentId: 1, name: 'Operating' },
  { id: 5, parentId: 4, name: 'Petty' },
];
const opts = (order = []) => ({ parentOf: (a) => a.parentId, order });
const ids = (arranged) => arranged.map((a) => a.id);

test('a tree keeps each parent above its children, in the list order without a custom one', () => {
  const a = arrange(accounts, opts());
  assert.deepEqual(ids(a), [1, 3, 4, 5, 2]);
  assert.deepEqual(a.map((x) => x.depth), [0, 1, 1, 2, 0]);
  assert.deepEqual(a.map((x) => x.hasChildren), [true, false, true, false, false]);
});

test('the custom order sorts siblings; rows not in it follow in the list order', () => {
  assert.deepEqual(ids(arrange(accounts, opts([2, 1, 4, 3]))), [2, 1, 4, 5, 3]);
  assert.deepEqual(ids(arrange(accounts, opts([4]))), [1, 4, 5, 3, 2]);
  // A parent not in the list (filtered out) makes its children top-level.
  assert.deepEqual(ids(arrange(accounts.filter((x) => x.id !== 1), opts())), [2, 3, 4, 5]);
});

test('collapsing hides a row’s children and grandchildren; Collapse all names every parent', () => {
  const a = arrange(accounts, opts());
  assert.deepEqual(ids(visibleRows(a, new Set([1]))), [1, 2]);
  assert.deepEqual(ids(visibleRows(a, new Set([4]))), [1, 3, 4, 2]);
  assert.deepEqual([...parentIds(a)].sort(), [1, 4]);
  assert.deepEqual(ids(visibleRows(a, parentIds(a))), [1, 2]);
});

test('dragging moves a row with its children among its siblings only', () => {
  const a = arrange(accounts, opts());
  // Savings before Checking: Checking's subtree moves along.
  assert.deepEqual(moveRow(a, 2, 1), [2, 1, 3, 4, 5]);
  // Checking after Savings.
  assert.deepEqual(moveRow(a, 1, 2, { after: true }), [2, 1, 3, 4, 5]);
  // Operating (with Petty) before Payroll.
  assert.deepEqual(moveRow(a, 4, 3), [1, 4, 5, 3, 2]);
  assert.deepEqual(moveRow(a, 3, 4, { after: true }), [1, 4, 5, 3, 2]);
  // Different parents, or the same row: no move.
  assert.equal(moveRow(a, 5, 3), null);
  assert.equal(moveRow(a, 3, 2), null);
  assert.equal(moveRow(a, 2, 2), null);
  // The new order arranges the same way.
  assert.deepEqual(ids(arrange(accounts, opts(moveRow(a, 4, 3)))), [1, 4, 5, 3, 2]);
});

test('jobs and sub-items find their parent by name', () => {
  const byName = new Map([['acme', 1], ['acme:kitchen', 2]]);
  assert.equal(parentByName('Acme:Kitchen', byName), 1);
  assert.equal(parentByName('Acme:Kitchen:Phase 1', byName), 2);
  assert.equal(parentByName('Acme', byName), null);
  assert.equal(parentByName('Other:Job', byName), null);
  assert.equal(parentByName(null, byName), null);
  assert.equal(parentByName(undefined, byName), null);
});
