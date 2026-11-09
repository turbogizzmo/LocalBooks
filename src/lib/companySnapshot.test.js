import { test } from 'node:test';
import assert from 'node:assert/strict';
import { moveWidget } from './companySnapshot.js';

test('a widget moves one place and stays on the page', () => {
  assert.deepEqual(moveWidget([1, 2, 3], 1, -1), [2, 1, 3]);
  assert.deepEqual(moveWidget([1, 2, 3], 1, 1), [1, 3, 2]);
  assert.deepEqual(moveWidget([1, 2, 3], 0, -1), [1, 2, 3]);
  assert.deepEqual(moveWidget([1, 2, 3], 2, 1), [1, 2, 3]);
});
