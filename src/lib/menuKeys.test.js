import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextMenuIndex } from './menuKeys.js';

test('arrow keys move through a menu and wrap round', () => {
  assert.equal(nextMenuIndex('ArrowDown', 0, 3), 1);
  assert.equal(nextMenuIndex('ArrowDown', 2, 3), 0);
  assert.equal(nextMenuIndex('ArrowUp', 0, 3), 2);
  assert.equal(nextMenuIndex('ArrowUp', 2, 3), 1);
});

test('with nothing focused, down starts at the top and up at the bottom', () => {
  assert.equal(nextMenuIndex('ArrowDown', -1, 3), 0);
  assert.equal(nextMenuIndex('ArrowUp', -1, 3), 2);
});

test('Home and End go to the ends; other keys and empty menus do nothing', () => {
  assert.equal(nextMenuIndex('Home', 2, 3), 0);
  assert.equal(nextMenuIndex('End', 0, 3), 2);
  assert.equal(nextMenuIndex('Enter', 1, 3), null);
  assert.equal(nextMenuIndex('ArrowDown', -1, 0), null);
});
