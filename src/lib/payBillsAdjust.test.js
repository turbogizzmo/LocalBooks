import { test } from 'node:test';
import assert from 'node:assert/strict';
import { creditLeftFor, overrideProblem, withOverrides } from './payBillsAdjust.js';

const plan = {
  discounts: [{ billId: 1, amountCents: 200 }, { billId: 2, amountCents: 100 }],
  credits: [{ creditId: 9, billId: 1, amountCents: 500 }, { creditId: 9, billId: 2, amountCents: 300 }],
};

test('a bill set by hand takes its own discount and credits; the others keep the plan', () => {
  const out = withOverrides(plan, { 1: { discountCents: 50, credits: [{ creditId: 9, amountCents: 100 }, { creditId: 7, amountCents: 0 }] } });
  assert.deepEqual(out.discounts, [{ billId: 2, amountCents: 100 }, { billId: 1, amountCents: 50 }]);
  assert.deepEqual(out.credits, [{ creditId: 9, billId: 2, amountCents: 300 }, { creditId: 9, billId: 1, amountCents: 100 }]);
});

test('a discount of zero takes none, and setting only credits keeps the planned discount', () => {
  assert.deepEqual(withOverrides(plan, { 2: { discountCents: 0 } }).discounts, [{ billId: 1, amountCents: 200 }]);
  assert.deepEqual(withOverrides(plan, { 1: { credits: [] } }).discounts, plan.discounts);
  assert.deepEqual(withOverrides(plan, { 1: { credits: [] } }).credits, [{ creditId: 9, billId: 2, amountCents: 300 }]);
  assert.deepEqual(withOverrides(plan, {}), plan);
});

test('what is left of a credit for a bill excludes its use on that bill', () => {
  assert.equal(creditLeftFor({ id: 9, remainingCents: 1000 }, plan.credits, 1), 700);
  assert.equal(creditLeftFor({ id: 9, remainingCents: 200 }, plan.credits, 1), 0);
});

test('amounts are checked against the offer, the credit left and the balance', () => {
  const facts = { balanceCents: 1000, offeredCents: 200, leftOf: () => 500 };
  assert.equal(overrideProblem({ discountCents: 150, credits: [{ creditId: 9, amountCents: 500 }] }, facts), null);
  assert.match(overrideProblem({ discountCents: 250 }, facts), /more than the discount/);
  assert.match(overrideProblem({ discountCents: 10 }, { ...facts, offeredCents: 0 }), /no discount/);
  assert.match(overrideProblem({ discountCents: null }, facts), /0.00 or more/);
  assert.match(overrideProblem({ credits: [{ creditId: 9, amountCents: 600 }] }, facts), /more of a credit/);
  assert.match(overrideProblem({ discountCents: 200, credits: [{ creditId: 9, amountCents: 500 }, { creditId: 8, amountCents: 400 }] }, facts), /more than the bill/);
});
