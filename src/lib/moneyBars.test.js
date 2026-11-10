import { test } from 'node:test';
import assert from 'node:assert/strict';
import { itemDetail, openTarget, segmentItem } from './moneyBars.js';

test('a segment becomes a bar cell with its rows', () => {
  const cell = segmentItem({ key: 'UNBILLED', label: 'Unbilled', cents: 12_500, count: 2, items: [{ id: 1 }, { id: 2 }] });
  assert.equal(cell.label, 'Unbilled');
  assert.equal(cell.cents, 12_500);
  assert.equal(cell.count, 2);
  assert.equal(cell.list.length, 2);
  assert.ok(cell.icon && cell.color);
  assert.ok(segmentItem({ key: 'NEW', label: 'New', cents: 0, count: 0, items: [] }).icon);
});

test('each row opens the window that holds it', () => {
  assert.deepEqual(openTarget({ kind: 'COST', id: 9, customerId: 4 }), ['sales/bill-costs', 'open', { customerId: 4 }]);
  assert.deepEqual(openTarget({ kind: 'TIME', id: 3, customerId: 5 }), ['sales/bill-costs', 'open', { customerId: 5 }]);
  assert.deepEqual(openTarget({ kind: 'ESTIMATE', id: 7 }), ['sales/estimates', 'edit', { estimateId: 7 }]);
  assert.deepEqual(openTarget({ kind: 'PURCHASE_ORDER', id: 8 }), ['expenses/purchase-orders', 'edit', { purchaseOrderId: 8 }]);
  assert.deepEqual(openTarget({ kind: 'CARD', id: 30 }), ['accounting/register', 'open', { accountId: 30 }]);
  assert.equal(openTarget({ kind: 'OTHER', id: 1 }), null);
});

test('the detail shows the number and memo that are there', () => {
  assert.equal(itemDetail({ number: '12', memo: 'Paint' }), '#12 · Paint');
  assert.equal(itemDetail({ number: null, memo: 'Paint' }), 'Paint');
  assert.equal(itemDetail({ number: null, memo: null }), '');
});
