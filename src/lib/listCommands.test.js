import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pickList } from './listCommands.js';

// A stand-in for a DOM element: `contains` is true for itself and for what it holds, as Element.contains is.
const node = (shown, children = []) => {
  const self = { isConnected: true, getClientRects: () => (shown ? [1] : []), contains: (x) => x === self || children.includes(x) };
  return self;
};

test('a list command goes to the focused list, else the last visible one', () => {
  const focused = {};
  const rootA = node(true, [focused]);
  const a = { root: () => rootA };
  const b = { root: () => node(true) };
  const hidden = { root: () => node(false) };
  assert.equal(pickList([a, b, hidden], focused), a);
  assert.equal(pickList([a, b, hidden], null), b);
  assert.equal(pickList([hidden], null), null);
  assert.equal(pickList([], null), null);
});
