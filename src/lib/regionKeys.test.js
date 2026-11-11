import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextRegion } from './regionKeys.js';

test('F6 moves to the next part and Shift+F6 to the one before, wrapping round', () => {
  assert.equal(nextRegion(3, 0, false), 1);
  assert.equal(nextRegion(3, 2, false), 0);
  assert.equal(nextRegion(3, 0, true), 2);
  assert.equal(nextRegion(3, 1, true), 0);
});

test('from outside every part, F6 starts at the first and Shift+F6 at the last', () => {
  assert.equal(nextRegion(3, -1, false), 0);
  assert.equal(nextRegion(3, -1, true), 2);
});

test('with no parts there is nowhere to go', () => {
  assert.equal(nextRegion(0, -1, false), null);
});

test('with one part, F6 stays in it', () => {
  assert.equal(nextRegion(1, 0, false), 0);
  assert.equal(nextRegion(1, 0, true), 0);
});
