// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  registerColor,
  steppedTextSize,
  textSizeFor,
  textSizeStepForKey,
  windowKind,
  withRegisterColor,
  withTextSize,
} from './textSize.js';

test('each page opens in a kind of window', () => {
  assert.equal(windowKind('accounting/register'), 'register');
  assert.equal(windowKind('reports'), 'report');
  assert.equal(windowKind('sales/invoices'), 'form');
  assert.equal(windowKind('accounting/journal'), 'form');
  assert.equal(windowKind('sales/customers'), 'list');
  assert.equal(windowKind('accounting/chart-of-accounts'), 'list');
  assert.equal(windowKind('dashboard'), null);
  assert.equal(windowKind('settings/preferences'), null);
  assert.equal(windowKind(null), null);
});

test('Larger and Smaller step through the sizes and stop at the ends', () => {
  assert.equal(steppedTextSize(100, 1), 110);
  assert.equal(steppedTextSize(100, -1), 90);
  assert.equal(steppedTextSize(160, 1), 160);
  assert.equal(steppedTextSize(80, -1), 80);
  assert.equal(steppedTextSize(140, 0), 100);
  assert.equal(steppedTextSize(123, 1), 110); // an unknown size starts from normal
});

test('each kind keeps its own size', () => {
  let sizes = {};
  sizes = withTextSize(sizes, 'register', 1);
  sizes = withTextSize(sizes, 'register', 1);
  sizes = withTextSize(sizes, 'report', -1);
  assert.deepEqual(sizes, { register: 125, report: 90 });
  assert.equal(textSizeFor(sizes, 'register'), 125);
  assert.equal(textSizeFor(sizes, 'form'), 100);
  assert.deepEqual(withTextSize(sizes, 'register', 0), { report: 90 });
  assert.equal(withTextSize(sizes, null, 1), sizes);
  assert.equal(textSizeFor({ list: 'huge' }, 'list'), 100);
});

test('Cmd on a Mac, Ctrl elsewhere, with + = − and 0', () => {
  assert.equal(textSizeStepForKey({ key: '=', metaKey: true }, true), 1);
  assert.equal(textSizeStepForKey({ key: '+', metaKey: true, shiftKey: true }, true), 1);
  assert.equal(textSizeStepForKey({ key: '-', ctrlKey: true }, false), -1);
  assert.equal(textSizeStepForKey({ key: '0', ctrlKey: true }, false), 0);
  assert.equal(textSizeStepForKey({ key: '-', ctrlKey: true }, true), null);
  assert.equal(textSizeStepForKey({ key: '=', metaKey: true, altKey: true }, true), null);
  assert.equal(textSizeStepForKey({ key: 'a', metaKey: true }, true), null);
});

test('a register color is kept per company file and account', () => {
  let colors = withRegisterColor({}, '/books/a.localbooks', 7, 'blue');
  colors = withRegisterColor(colors, '/books/b.localbooks', 7, 'pink');
  assert.equal(registerColor(colors, '/books/a.localbooks', 7), 'blue');
  assert.equal(registerColor(colors, '/books/b.localbooks', 7), 'pink');
  assert.equal(registerColor(colors, '/books/a.localbooks', 8), '');
  colors = withRegisterColor(colors, '/books/a.localbooks', 7, '');
  assert.equal(registerColor(colors, '/books/a.localbooks', 7), '');
  assert.equal(registerColor({ 'x#1': 'neon' }, 'x', 1), '');
});
