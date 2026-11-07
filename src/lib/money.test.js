// Run with `npm test` (Node's built-in runner); `cargo test` runs it too (frontend_checks.rs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { centsToInput, formatMinutes, impliedDecimalText, parseMoneyToCents, setHoursFormat, setImpliedDecimal } from './money.js';
import { formatDate, setFourDigitYears } from './dates.js';

test('amounts parse exactly with a typed decimal point', () => {
  assert.equal(parseMoneyToCents('1,234.5'), 123_450);
  assert.equal(parseMoneyToCents('$12'), 1_200);
  assert.equal(parseMoneyToCents('.99'), 99);
  assert.equal(parseMoneyToCents('-0.29'), -29);
  assert.equal(parseMoneyToCents('12.345'), null);
  assert.equal(parseMoneyToCents('abc'), null);
  assert.equal(parseMoneyToCents(''), null);
});

test('with the decimal point added automatically, digits without a point are cents', () => {
  const auto = { impliedDecimal: true };
  assert.equal(parseMoneyToCents('1234', auto), 1_234);
  assert.equal(parseMoneyToCents('5', auto), 5);
  assert.equal(parseMoneyToCents('1,234', auto), 1_234);
  assert.equal(parseMoneyToCents('-250', auto), -250);
  assert.equal(parseMoneyToCents('$100', auto), 100);
  // A typed point always wins, so amounts shown for editing read back unchanged.
  assert.equal(parseMoneyToCents('12.', auto), 1_200);
  assert.equal(parseMoneyToCents('12.5', auto), 1_250);
  assert.equal(parseMoneyToCents(centsToInput(123_456), auto), 123_456);
  assert.equal(parseMoneyToCents('', auto), null);
  assert.equal(parseMoneyToCents('99999999999999999', auto), null);
});

test('the decimal-point preference applies to every parse until turned off', () => {
  setImpliedDecimal(true);
  try {
    assert.equal(parseMoneyToCents('1234'), 1_234);
    assert.equal(parseMoneyToCents('1234', { impliedDecimal: false }), 123_400);
  } finally {
    setImpliedDecimal(false);
  }
  assert.equal(parseMoneyToCents('1234'), 123_400);
});

test('a money box shows the placed decimal point once you leave it', () => {
  const auto = { impliedDecimal: true };
  assert.equal(impliedDecimalText('1234', auto), '12.34');
  assert.equal(impliedDecimalText('5', auto), '0.05');
  assert.equal(impliedDecimalText('-250', auto), '-2.50');
  assert.equal(impliedDecimalText('$1,234', auto), '12.34');
  // What the box shows reads back as the same amount.
  assert.equal(parseMoneyToCents(impliedDecimalText('1234', auto), auto), 1_234);
  // A typed point, text that isn't an amount, or the preference off: left as typed.
  assert.equal(impliedDecimalText('12.5', auto), null);
  assert.equal(impliedDecimalText('12.', auto), null);
  assert.equal(impliedDecimalText('10%', auto), null);
  assert.equal(impliedDecimalText('', auto), null);
  assert.equal(impliedDecimalText('99999999999999999', auto), null);
  assert.equal(impliedDecimalText('1234', { impliedDecimal: false }), null);
  assert.equal(impliedDecimalText('1234'), null);
});

test('hours show as hours:minutes or decimal hours', () => {
  assert.equal(formatMinutes(90), '1:30');
  assert.equal(formatMinutes(90, 'decimal'), '1.50');
  assert.equal(formatMinutes(20, 'decimal'), '0.33');
  assert.equal(formatMinutes(50, 'decimal'), '0.83');
  assert.equal(formatMinutes(1, 'decimal'), '0.02');
  assert.equal(formatMinutes(0, 'decimal'), '0.00');
  setHoursFormat('decimal');
  try {
    assert.equal(formatMinutes(135), '2.25');
  } finally {
    setHoursFormat('hhmm');
  }
  assert.equal(formatMinutes(135), '2:15');
});

test('dates show four-digit or two-digit years', () => {
  assert.equal(formatDate('2026-10-13'), '10/13/2026');
  assert.equal(formatDate('2026-10-13', { fourDigits: false }), '10/13/26');
  setFourDigitYears(false);
  try {
    assert.equal(formatDate('2009-01-02'), '01/02/09');
  } finally {
    setFourDigitYears(true);
  }
  assert.equal(formatDate(''), '');
});
