import { test } from 'node:test';
import assert from 'node:assert/strict';
import { notOnList } from './notOnList.js';

test('reads the name and list from a refusal', () => {
  assert.deepEqual(notOnList("NAME_NOT_ON_LIST: Acmee isn't on your customer list. Choose a name from the list, or add it first."), { kind: 'customer', name: 'Acmee' });
  assert.deepEqual(notOnList(new Error("NAME_NOT_ON_LIST: Acme:Bathroom isn't on your customer list. Choose a name from the list, or add it first.")), { kind: 'customer', name: 'Acme:Bathroom' });
  assert.deepEqual(notOnList("NAME_NOT_ON_LIST: O'Neil Supply isn't on your vendor list. Choose a name from the list, or add it first."), { kind: 'vendor', name: "O'Neil Supply" });
});

test('other errors are not a missing name', () => {
  assert.equal(notOnList('NEEDS_CONFIRMATION: {"warnings":[]}'), null);
  assert.equal(notOnList("INV_AMOUNT_TOO_LARGE: Acme isn't on your customer list."), null);
  assert.equal(notOnList(null), null);
  assert.equal(notOnList({}), null);
});
