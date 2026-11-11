import { test } from 'node:test';
import assert from 'node:assert/strict';
import { enteredText, skippedLines } from './recurringReport.js';

test('entered copies, and those skipped', () => {
  assert.equal(enteredText({ entered: 2, skipped: [] }), 'Entered 2 transactions; each is posted and in the journal.');
  assert.equal(enteredText({ entered: 0, skipped: [] }), 'Nothing was due.');
  const report = { entered: 1, skipped: [{ name: 'Old rent', date: '2026-02-15', reason: 'LEDGER_CLOSED_PERIOD: 2026-02-15 is on or before the closing date' }] };
  assert.equal(enteredText(report), "Entered 1 transaction; each is posted and in the journal. 1 couldn't be entered; see below.");
  assert.deepEqual(skippedLines(report), ['Old rent, 2026-02-15: 2026-02-15 is on or before the closing date']);
  assert.equal(enteredText({ entered: 0, skipped: report.skipped }), "Nothing was entered. 1 couldn't be entered; see below.");
});
