import { test } from 'node:test';
import assert from 'node:assert/strict';
import { budgetYearChoices, fiscalStartMonth } from './budgetYears.js';

test('a July fiscal year in November 2026 starts with the year that began in July 2026', () => {
  const years = budgetYearChoices('2026-11-10', 7);
  assert.deepEqual(years.map((y) => y.label), ['Jul 2025 – Jun 2026', 'Jul 2026 – Jun 2027', 'Jul 2027 – Jun 2028', 'Jul 2028 – Jun 2029']);
  assert.equal(years.find((y) => y.current).start, '2026-07-01');
});

test('before the fiscal year turns, the current year is the one that began last year', () => {
  assert.equal(budgetYearChoices('2026-03-15', 4).find((y) => y.current).label, 'Apr 2025 – Mar 2026');
});

test('a calendar fiscal year', () => {
  const years = budgetYearChoices('2026-11-10', 1);
  assert.equal(years.find((y) => y.current).label, 'Jan – Dec 2026');
  assert.equal(years[0].start, '2025-01-01');
});

test('the preference reads as a month', () => {
  assert.equal(fiscalStartMonth('07-01'), 7);
  assert.equal(fiscalStartMonth(undefined), 1);
  assert.equal(fiscalStartMonth('xx'), 1);
});
