import { test } from 'node:test';
import assert from 'node:assert/strict';
import { blankFormLine, fromItem, layOutPurchase, readValue, rowsFromStored } from './purchaseLines.js';

const row = (afterLine, kind, value, accountId = 9) => ({ ...blankFormLine(afterLine, kind), value, accountId });

test('lines lay out as the ledger lays them out', () => {
  // The same cases as purchase_lines::tests::lay_out_works_out_each_line_and_the_total.
  const laid = layOutPurchase([10_000, 3_333, 2_000], [row(1, 'DISCOUNT', '10%'), row(2, 'CHARGE', '2.5%'), row(3, 'DISCOUNT', '5.00')]);
  assert.deepEqual(laid.amounts, [1_000, 83, 500]);
  assert.equal(laid.total, 10_000 + 3_333 + 2_000 - 1_000 + 83 - 500);
  assert.deepEqual(laid.formLines[0], { afterLine: 1, kind: 'DISCOUNT', description: '', productServiceId: null, percentPpm: 100_000, amountCents: null, accountId: 9, classId: null });
  assert.equal(laid.formLines[2].amountCents, 500);
  assert.deepEqual(layOutPurchase([100], [row(1, 'CHARGE', '0.5%')]).amounts, [1]);
  assert.equal(layOutPurchase([5_000], []).total, 5_000);
});

test('lines the ledger refuses are refused here first', () => {
  assert.equal(layOutPurchase([1_000], [row(2, 'DISCOUNT', '1.00')]).error, 'PLACE');
  assert.equal(layOutPurchase([1_000], [row(1, 'DISCOUNT', '1.00'), row(1, 'CHARGE', '1.00')]).error, 'NEEDS_BASE');
  assert.equal(layOutPurchase([1_000], [row(1, 'DISCOUNT', '1.00', null)]).error, 'ACCOUNT');
  assert.equal(layOutPurchase([1_000], [row(1, 'DISCOUNT', '')]).error, 'AMOUNT');
  assert.equal(layOutPurchase([10], [row(1, 'DISCOUNT', '0.0001%')]).error, 'ZERO');
  assert.equal(layOutPurchase([1_000], [row(1, 'DISCOUNT', '10.01')]).error, 'TOO_LARGE');
  assert.equal(layOutPurchase([1_000], [row(1, 'DISCOUNT', '100%')]).error, 'TOTAL');
});

test('values, stored lines and items fill a row', () => {
  assert.deepEqual(readValue('12.5%'), { percentPpm: 125_000, amountCents: null });
  assert.deepEqual(readValue('4.20'), { percentPpm: null, amountCents: 420 });
  assert.equal(readValue('-3'), null);
  const [stored] = rowsFromStored([{ afterLine: 2, kind: 'CHARGE', productServiceId: 4, accountId: 7, classId: null, description: 'Freight', percentPpm: 50_000, amountCents: 1_500 }]);
  assert.deepEqual([stored.afterLine, stored.kind, stored.itemId, stored.value], [2, 'CHARGE', 4, '5%']);
  assert.equal(rowsFromStored([{ afterLine: 1, kind: 'DISCOUNT', accountId: 7, description: 'Off', percentPpm: null, amountCents: 250 }])[0].value, '2.50');
  const filled = fromItem(blankFormLine(1, 'CHARGE'), { id: 3, name: 'Freight', description: '', rateCents: 0, percentPpm: 20_000, incomeAccountId: 1, expenseAccountId: 6 });
  assert.deepEqual([filled.itemId, filled.accountId, filled.description, filled.value], [3, 6, 'Freight', '2%']);
  assert.equal(fromItem(blankFormLine(), { id: 5, name: 'Rebate', rateCents: 1_000, percentPpm: null, incomeAccountId: 2, expenseAccountId: null }).accountId, 2);
});
