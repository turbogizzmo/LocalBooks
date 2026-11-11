import { test } from 'node:test';
import assert from 'node:assert/strict';
import { billableOfferText, timeBillable } from './billableOffer.js';

const time = (productServiceId) => ({ id: Math.random(), productServiceId });

test('names the costs and the time the picker can bill', () => {
  assert.equal(billableOfferText([{}], [time(4)]), '1 unbilled cost and 1 unbilled time entry');
  assert.equal(billableOfferText([{}, {}], [time(4), time(5), time(null)]), '2 unbilled costs and 2 unbilled time entries');
  assert.equal(billableOfferText([{}], [time(null)]), '1 unbilled cost');
});

test('time with no service item alone is nothing to offer', () => {
  assert.equal(billableOfferText([], [time(null), time(null)]), null);
  assert.equal(billableOfferText([], []), null);
  assert.equal(timeBillable(time(null)), false);
  assert.equal(timeBillable(time(0)), true);
});
