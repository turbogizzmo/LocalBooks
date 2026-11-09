import { test } from 'node:test';
import assert from 'node:assert/strict';
import { clearCopiedLine, copiedLine, copyLine, formKeyCommand, pasteLineAt } from './formKeys.js';

const key = (k, mods = {}) => ({ key: k, code: /^[a-z]$/i.test(k) ? `Key${k.toUpperCase()}` : k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, target: null, ...mods });

test('the desktop form keys name their commands', () => {
  const ctrl = (k) => formKeyCommand(key(k, { ctrlKey: true }), false);
  assert.equal(ctrl('n'), 'new');
  assert.equal(ctrl('p'), 'print');
  assert.equal(ctrl('e'), 'edit');
  assert.equal(ctrl('g'), 'goto-transfer');
  assert.equal(ctrl('l'), 'open-list');
  assert.equal(ctrl('d'), 'delete');
  assert.equal(ctrl('m'), 'memorize');
  assert.equal(ctrl('Insert'), 'insert-line');
  assert.equal(ctrl('Delete'), 'delete-line');
  assert.equal(formKeyCommand(key('PageUp'), false), 'previous');
  assert.equal(formKeyCommand(key('PageDown'), false), 'next');
  assert.equal(formKeyCommand(key('s', { altKey: true }), false), 'save-close');
  assert.equal(formKeyCommand(key('n', { altKey: true }), false), 'save-new');
});

test('Copy Line and Paste Line are Ctrl+Alt+Y and Ctrl+Alt+V', () => {
  assert.equal(formKeyCommand(key('y', { ctrlKey: true, altKey: true }), false), 'copy-line');
  assert.equal(formKeyCommand(key('v', { ctrlKey: true, altKey: true }), false), 'paste-line');
  // Plain Ctrl+V stays the system's Paste.
  assert.equal(formKeyCommand(key('v', { ctrlKey: true }), false), null);
  assert.equal(formKeyCommand(key('y', { ctrlKey: true, altKey: true, shiftKey: true }), false), null);
});

test('letters are read from the physical key, so Option+letter on a Mac still counts', () => {
  assert.equal(formKeyCommand({ ...key('y', { ctrlKey: true, altKey: true }), key: '¥' }, true), 'copy-line');
});

test('on a Mac, Cmd+N, Cmd+P, Cmd+E, Cmd+G and Cmd+L stay the system’s; Cmd+D still voids', () => {
  for (const k of ['n', 'p', 'e', 'g', 'l', 'm']) assert.equal(formKeyCommand(key(k, { metaKey: true }), true), null, k);
  assert.equal(formKeyCommand(key('d', { metaKey: true }), true), 'delete');
  // Ctrl works on a Mac too.
  assert.equal(formKeyCommand(key('g', { ctrlKey: true }), true), 'goto-transfer');
});

test('Page Up and Page Down are left to a box that scrolls', () => {
  const inList = { closest: (sel) => (sel.includes('select') ? {} : null) };
  assert.equal(formKeyCommand(key('PageDown', { target: inList }), false), null);
  const inField = { closest: () => null };
  assert.equal(formKeyCommand(key('PageDown', { target: inField }), false), 'next');
  assert.equal(formKeyCommand(key('PageUp', { ctrlKey: true }), false), null);
});

test('a copied line pastes only into the same kind of grid, as a copy', () => {
  clearCopiedLine();
  assert.equal(copiedLine('items'), null);
  const line = { key: 4, productId: 7, description: 'Widget', qty: '2', rate: '5.00' };
  assert.equal(copyLine('items', line), true);
  line.description = 'changed after copying';
  assert.equal(copiedLine('splits'), null);
  const pasted = copiedLine('items');
  assert.equal(pasted.description, 'Widget');
  pasted.qty = '9';
  assert.equal(copiedLine('items').qty, '2', 'each paste gets its own copy');
  assert.equal(copyLine('items', null), false, 'no line to copy leaves the clipboard as it was');
  assert.equal(copiedLine('items').description, 'Widget');
});

test('Paste Line inserts above the line with focus, under a new key', () => {
  const lines = [{ key: 1 }, { key: 2 }];
  let next = 10;
  const newKey = () => next++;
  assert.deepEqual(pasteLineAt(lines, 1, { key: 1, memo: 'x' }, newKey), [{ key: 1 }, { key: 10, memo: 'x' }, { key: 2 }]);
  assert.deepEqual(pasteLineAt(lines, null, { key: 1 }, newKey).map((l) => l.key), [1, 2, 11]);
  assert.deepEqual(pasteLineAt(lines, 9, { key: 1 }, newKey).map((l) => l.key), [1, 2, 12]);
  assert.deepEqual(lines, [{ key: 1 }, { key: 2 }], 'the lines given are left as they were');
});
