// Run with `npm test` (Node's built-in test runner; no packages needed).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { addDays, dateForKey, isIsoDate, stepNumber } from '../dates.js';

const TODAY = '2026-11-06'; // a Friday

test('t is today, whatever the box holds', () => {
  assert.equal(dateForKey('t', '2020-02-02', TODAY), TODAY);
  assert.equal(dateForKey('T', '', TODAY), TODAY);
});

test('+, = and - move one day, across month, year and leap days', () => {
  assert.equal(dateForKey('+', '2026-01-31', TODAY), '2026-02-01');
  assert.equal(dateForKey('=', '2026-12-31', TODAY), '2027-01-01');
  assert.equal(dateForKey('-', '2026-03-01', TODAY), '2026-02-28');
  assert.equal(dateForKey('-', '2024-03-01', TODAY), '2024-02-29');
  assert.equal(dateForKey('-', '2027-01-01', TODAY), '2026-12-31');
});

test('w and k are the Sunday and Saturday of the week', () => {
  assert.equal(dateForKey('w', '2026-11-06', TODAY), '2026-11-01');
  assert.equal(dateForKey('k', '2026-11-06', TODAY), '2026-11-07');
  // A Sunday is its own first day; a Saturday its own last.
  assert.equal(dateForKey('w', '2026-11-01', TODAY), '2026-11-01');
  assert.equal(dateForKey('k', '2026-11-07', TODAY), '2026-11-07');
  // Weeks that cross a month or a year.
  assert.equal(dateForKey('w', '2027-01-01', TODAY), '2026-12-27');
  assert.equal(dateForKey('k', '2026-12-30', TODAY), '2027-01-02');
});

test('m and h are the first and last day of the month', () => {
  assert.equal(dateForKey('m', '2026-11-06', TODAY), '2026-11-01');
  assert.equal(dateForKey('h', '2026-11-06', TODAY), '2026-11-30');
  assert.equal(dateForKey('h', '2026-02-10', TODAY), '2026-02-28');
  assert.equal(dateForKey('h', '2028-02-10', TODAY), '2028-02-29');
  assert.equal(dateForKey('H', '2026-12-25', TODAY), '2026-12-31');
});

test('y and r are the first and last day of the year', () => {
  assert.equal(dateForKey('y', '2026-11-06', TODAY), '2026-01-01');
  assert.equal(dateForKey('r', '2026-11-06', TODAY), '2026-12-31');
});

test('an empty or unfinished box counts from today', () => {
  assert.equal(dateForKey('+', '', TODAY), '2026-11-07');
  assert.equal(dateForKey('m', '2026-02-31', TODAY), '2026-11-01');
  assert.equal(dateForKey('k', undefined, TODAY), '2026-11-07');
});

test('other keys are left to the box', () => {
  for (const key of ['1', 'a', 'x', '/', 'Enter', 'ArrowUp', 'Tab', '', null]) assert.equal(dateForKey(key, TODAY, TODAY), null);
});

test('repeated keys keep moving', () => {
  let d = '2026-11-28';
  for (let i = 0; i < 5; i += 1) d = dateForKey('+', d, TODAY);
  assert.equal(d, '2026-12-03');
  assert.equal(addDays('2026-12-03', -5), '2026-11-28');
});

test('isIsoDate takes only real calendar dates', () => {
  assert.equal(isIsoDate('2026-11-06'), true);
  assert.equal(isIsoDate('2026-02-29'), false);
  assert.equal(isIsoDate('2026-13-01'), false);
  assert.equal(isIsoDate('11/06/2026'), false);
  assert.equal(isIsoDate(''), false);
});

test('stepNumber steps the last number and keeps its width', () => {
  assert.equal(stepNumber('1009', 1), '1010');
  assert.equal(stepNumber('1010', -1), '1009');
  assert.equal(stepNumber('0099', 1), '0100');
  assert.equal(stepNumber('A-0099', 1), 'A-0100');
  assert.equal(stepNumber('CHK12x', 1), 'CHK13x');
  assert.equal(stepNumber('0', -1), '0');
  assert.equal(stepNumber('', 1), null);
  assert.equal(stepNumber('EFT', 1), null);
});
