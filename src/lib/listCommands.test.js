import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pickList } from './listCommands.js';

const node = (shown, children = []) => ({ isConnected: true, getClientRects: () => (shown ? [1] : []), contains: (x) => x === node || children.includes(x) });

test('a list command goes to the focused list, else the last visible one', () => {
  const focused = {};
  const a = { root: () => ({ ...node(true), contains: (x) => x === focused }) };
  const b = { root: () => node(true) };
  const hidden = { root: () => node(false) };
  assert.equal(pickList([a, b, hidden], focused), a);
  assert.equal(pickList([a, b, hidden], null), b);
  assert.equal(pickList([hidden], null), null);
  assert.equal(pickList([], null), null);
});
