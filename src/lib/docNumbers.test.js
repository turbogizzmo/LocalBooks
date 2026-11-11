import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MAX_DOC_NUMBER, numberTaken, readDocNumber } from './docNumbers.js';

test('reads a typed number', () => {
  assert.deepEqual(readDocNumber('1005'), { value: 1005 });
  assert.deepEqual(readDocNumber(' #1005 '), { value: 1005 });
  assert.deepEqual(readDocNumber(''), { value: null });
  assert.deepEqual(readDocNumber(String(MAX_DOC_NUMBER)), { value: MAX_DOC_NUMBER });
});

test('refuses what is not a whole number in range', () => {
  for (const bad of ['INV-7', '10.5', '-3', '1e3']) assert.match(readDocNumber(bad).error, /^Invoice # is a whole number/);
  assert.match(readDocNumber('0', 'Estimate').error, /^Estimate # is from 1 to 999,999,999/);
  assert.match(readDocNumber(String(MAX_DOC_NUMBER + 1)).error, /is from 1 to/);
});

test('reads a number already used, with the next free one', () => {
  assert.deepEqual(numberTaken('DOC_NUMBER_TAKEN: Invoice #1005 is already used (Acme, dated 03/04/2026). The next free number is #1012.'), {
    message: 'Invoice #1005 is already used (Acme, dated 03/04/2026).',
    next: 1012,
  });
  assert.equal(numberTaken('DOC_NUMBER_RANGE: an invoice number is a whole number from 1 to 999999999'), null);
  assert.equal(numberTaken(null), null);
});
