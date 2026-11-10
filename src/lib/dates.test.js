import { test } from 'node:test';
import assert from 'node:assert/strict';
import { periodRange } from './dates.js';

test('the last twelve months are whole months ending with this one', () => {
  assert.deepEqual(periodRange('last12Months', '2026-11-09'), ['2025-12-01', '2026-11-30']);
  assert.deepEqual(periodRange('last12Months', '2026-12-31'), ['2026-01-01', '2026-12-31']);
  assert.deepEqual(periodRange('last12Months', '2024-02-10'), ['2023-03-01', '2024-02-29']);
});
