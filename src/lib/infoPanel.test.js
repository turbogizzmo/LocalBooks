import { test } from 'node:test';
import assert from 'node:assert/strict';
import { infoPanelHidden, partySummaryLines, readReorderPoint, setInfoPanelHidden, stampNotes } from './infoPanel.js';

const none = { openBalanceCents: 0, activeEstimateCount: 0, activeEstimateCents: 0, unbilledCostCount: 0, unbilledCostCents: 0, unbilledTripCount: 0, openPoCount: 0, openPoCents: 0 };

test('a customer shows its balance, and estimates and unbilled work when there are some', () => {
  assert.deepEqual(partySummaryLines('customer', { ...none, openBalanceCents: 1_500 }), [['Open balance', 1_500, null]]);
  assert.deepEqual(partySummaryLines('customer', { ...none, activeEstimateCount: 2, activeEstimateCents: 9_000, unbilledCostCount: 1, unbilledCostCents: 450, unbilledTripCount: 3 }), [
    ['Open balance', 0, null],
    ['Active estimates', 9_000, '2 estimates'],
    ['Unbilled costs', 450, '1 cost'],
    ['Unbilled mileage', null, '3 trips'],
  ]);
});

test('a vendor shows what is owed and its open purchase orders', () => {
  assert.deepEqual(partySummaryLines('vendor', { ...none, openBalanceCents: 700, openPoCount: 1, openPoCents: 2_500 }), [
    ['Balance you owe', 700, null],
    ['Open purchase orders', 2_500, '1 order'],
  ]);
  assert.deepEqual(partySummaryLines('vendor', null), []);
});

test('hiding the panel is remembered per place', () => {
  const store = new Map();
  const storage = { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, v) };
  assert.equal(infoPanelHidden('customer', storage), false);
  setInfoPanelHidden('customer', true, storage);
  assert.equal(infoPanelHidden('customer', storage), true);
  assert.equal(infoPanelHidden('item', storage), false);
  setInfoPanelHidden('customer', false, storage);
  assert.equal(infoPanelHidden('customer', storage), false);
});

test('Date Stamp starts a new first line with the date', () => {
  assert.equal(stampNotes('', '11/08/2026'), '11/08/2026: ');
  assert.equal(stampNotes('Prefers email.', '11/08/2026'), '11/08/2026: \nPrefers email.');
  assert.equal(stampNotes(null, '11/08/2026'), '11/08/2026: ');
});

test('a reorder point is a quantity, zero, or blank to clear it', () => {
  assert.deepEqual(readReorderPoint(''), { e4: null });
  assert.deepEqual(readReorderPoint('0'), { e4: 0 });
  assert.deepEqual(readReorderPoint('12'), { e4: 120_000 });
  assert.deepEqual(readReorderPoint(' 2.5 '), { e4: 25_000 });
  assert.match(readReorderPoint('-3').error, /quantity/);
  assert.match(readReorderPoint('ten').error, /quantity/);
  assert.match(readReorderPoint('.').error, /quantity/);
  assert.deepEqual(readReorderPoint('.0'), { e4: 0 });
});
