import { test } from 'node:test';
import assert from 'node:assert/strict';
import { needsFocus } from './scrollFocus.js';

const box = (scrollHeight, clientHeight, scrollWidth = 100, clientWidth = 100) => ({ scrollHeight, clientHeight, scrollWidth, clientWidth });

test('a panel that scrolls with nothing to Tab to inside takes focus', () => {
  assert.equal(needsFocus(box(500, 200), false), true);
  assert.equal(needsFocus(box(200, 200, 400, 100), false), true);
});

test('a panel that fits, or holds something focusable, does not', () => {
  assert.equal(needsFocus(box(200, 200), false), false);
  assert.equal(needsFocus(box(201, 200), false), false);
  assert.equal(needsFocus(box(500, 200), true), false);
});
