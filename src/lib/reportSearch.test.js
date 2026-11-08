// Run with `npm test`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { reportMatcher, stepMatch } from './reportSearch.js';
import { setImpliedDecimal } from './money.js';

test('words match anywhere in a cell, ignoring case and spacing', () => {
  const m = reportMatcher('  office   SUPPLIES ');
  assert.equal(m('6420 · Office supplies'), true);
  assert.equal(m('Office'), false);
  assert.equal(reportMatcher('   '), null);
});

test('an amount matches however the cell writes it', () => {
  const m = reportMatcher('1250');
  assert.equal(m('1,250.00'), true);
  assert.equal(m('(1,250.00)'), true);
  assert.equal(m('$1,250.00'), true);
  assert.equal(m('12,500.00'), false);
  assert.equal(m('Invoice #1250'), true, 'text containing the number still matches');
  assert.equal(reportMatcher('1,250.5')('1,250.50'), true);
  assert.equal(reportMatcher('$99.95')('99.95'), true);
});

test('stepping wraps at both ends', () => {
  assert.equal(stepMatch(-1, 3), 0);
  assert.equal(stepMatch(2, 3), 0);
  assert.equal(stepMatch(0, 3, true), 2);
  assert.equal(stepMatch(-1, 3, true), 2);
  assert.equal(stepMatch(0, 0), -1);
});

test('the automatic decimal point preference does not change what an amount finds', () => {
  setImpliedDecimal(true);
  try {
    assert.equal(reportMatcher('1250')('1,250.00'), true);
    assert.equal(reportMatcher('1250')('12.50'), false);
  } finally {
    setImpliedDecimal(false);
  }
});
