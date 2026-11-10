// Run with `npm test` (Node's built-in runner). Imports only modules without packages.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scaleGroupQuantity } from './itemTypes.js';

test('a group member scales with the group quantity', () => {
  assert.equal(scaleGroupQuantity(30_000, 10_000, 20_000), 60_000, '3 of each, 2 groups: 6');
  assert.equal(scaleGroupQuantity(60_000, 20_000, 10_000), 30_000, 'and back');
  assert.equal(scaleGroupQuantity(10_000, 30_000, 10_000), 3_333, 'a third, rounded');
  assert.equal(scaleGroupQuantity(5_000, 10_000, 15_000), 7_500, 'half an hour, one and a half groups');
});

test('a group quantity that is not above zero changes nothing', () => {
  assert.equal(scaleGroupQuantity(30_000, 10_000, 0), null);
  assert.equal(scaleGroupQuantity(30_000, 0, 10_000), null);
  assert.equal(scaleGroupQuantity(30_000, null, 10_000), null);
});
