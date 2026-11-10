import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextRowIndex } from './rowKeys.js';

test('arrow keys move one row and stop at the ends', () => {
  assert.equal(nextRowIndex('ArrowDown', 0, 3), 1);
  assert.equal(nextRowIndex('ArrowDown', 2, 3), 2);
  assert.equal(nextRowIndex('ArrowUp', 0, 3), 0);
  assert.equal(nextRowIndex('ArrowUp', 2, 3), 1);
});

test('Home and End jump to the ends; other keys and empty lists do nothing', () => {
  assert.equal(nextRowIndex('Home', 2, 3), 0);
  assert.equal(nextRowIndex('End', 0, 3), 2);
  assert.equal(nextRowIndex('a', 1, 3), null);
  assert.equal(nextRowIndex('ArrowDown', 0, 0), null);
});
