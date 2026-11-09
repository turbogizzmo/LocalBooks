import { test } from 'node:test';
import assert from 'node:assert/strict';
import { stepTargets, byDateThenNumber, stepButtons, duplicateDates, discountDate } from './formStep.js';

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

test('a duplicate is dated today and keeps its terms’ days to the due date', () => {
  assert.deepEqual(duplicateDates('2026-01-10', '2026-02-09', '2026-11-08'), { date: '2026-11-08', due: '2026-12-08' });
  assert.deepEqual(duplicateDates('2026-03-01', '2026-03-01', '2026-11-08'), { date: '2026-11-08', due: '2026-11-08' });
  // Across a year end.
  assert.deepEqual(duplicateDates('2026-05-01', '2026-05-11', '2026-12-28'), { date: '2026-12-28', due: '2027-01-07' });
  assert.deepEqual(duplicateDates('2026-05-01', null, '2026-11-08'), { date: '2026-11-08', due: null });
  assert.deepEqual(duplicateDates('2026-05-01T10:00', '2026-05-11', '2026-11-08'), { date: '2026-11-08', due: null });
});

test('a bill’s discount date is the term’s discount days after the bill date', () => {
  const twoTenNet30 = { netDays: 30, discountPpm: 20_000, discountDays: 10 };
  assert.equal(discountDate(twoTenNet30, '2026-11-25'), '2026-12-05');
  assert.equal(discountDate({ netDays: 30, discountPpm: 0, discountDays: null }, '2026-11-25'), null);
  assert.equal(discountDate(null, '2026-11-25'), null);
  assert.equal(discountDate(twoTenNet30, ''), null);
});
