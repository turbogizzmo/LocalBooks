import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextTabStop } from './tabTrap.js';

test('Tab moves to the next stop, and from the last back to the first', () => {
  assert.equal(nextTabStop(4, { after: 2, before: 0 }, false), 2);
  assert.equal(nextTabStop(4, { after: null, before: 3 }, false), 0, 'past the last field: the first, not a window behind');
});

test('Shift+Tab moves to the previous stop, and from the first round to the last', () => {
  assert.equal(nextTabStop(4, { after: 2, before: 0 }, true), 0);
  assert.equal(nextTabStop(4, { after: 1, before: null }, true), 3);
});

test('a window with nothing to tab to keeps the key', () => {
  assert.equal(nextTabStop(0, { after: null, before: null }, false), null);
});
