import { test } from 'node:test';
import assert from 'node:assert/strict';
import { stepTargets, byDateThenNumber, stepButtons } from './formStep.js';

const rows = [{ id: 1 }, { id: 2 }, { id: 3 }];

test('Previous and Next open the neighbours of the open transaction', () => {
  assert.deepEqual(stepTargets(rows, 2), { previous: rows[0], next: rows[2] });
  assert.deepEqual(stepTargets(rows, 1), { previous: null, next: rows[1] });
  assert.deepEqual(stepTargets(rows, 3), { previous: rows[1], next: null });
});

test('from a new form, Previous opens the newest and Next nothing', () => {
  assert.deepEqual(stepTargets(rows, null), { previous: rows[2], next: null });
  assert.deepEqual(stepTargets([], null), { previous: null, next: null });
  // An open transaction that left the list (voided elsewhere) behaves like a new form.
  assert.deepEqual(stepTargets(rows, 9), { previous: rows[2], next: null });
});

test('saved transactions are ordered by date, then number as a number, then id', () => {
  const list = [
    { id: 5, d: '2026-02-01', n: '10' },
    { id: 4, d: '2026-01-15', n: null },
    { id: 3, d: '2026-02-01', n: '9' },
    { id: 2, d: '2026-02-01', n: 'A-2' },
    { id: 1, d: '2026-02-01', n: '9' },
  ];
  assert.deepEqual(byDateThenNumber(list, (r) => r.d, (r) => r.n).map((r) => r.id), [4, 1, 3, 5, 2]);
});

test('unsaved entries stop both buttons, and the hint says why', () => {
  const opened = [];
  const b = stepButtons(rows, 2, (r) => opened.push(r.id));
  assert.equal(b.previousDisabled, false);
  b.onPrevious();
  b.onNext();
  assert.deepEqual(opened, [1, 3]);
  const blocked = stepButtons(rows, 2, () => {}, { blocked: true });
  assert.equal(blocked.previousDisabled && blocked.nextDisabled, true);
  assert.match(blocked.blockedHint, /Save or clear/);
  assert.equal(stepButtons(rows, 3, () => {}).nextDisabled, true);
});
