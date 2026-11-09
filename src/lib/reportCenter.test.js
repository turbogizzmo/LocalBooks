// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { datesFor, describeReport, pushRecent, searchReports, toggleFavorite, withFavoritesAndRecent } from './reportCenter.js';

const groups = [
  { title: 'Business overview', reports: [['pl', 'Profit and Loss'], ['bs', 'Balance Sheet']] },
  { title: 'Who owes you', reports: [['ar', 'A/R Aging Summary'], ['preset:open-invoices', 'Open Invoices']] },
  { title: 'Memorized reports', reports: [['saved:3', 'Monthly sales']] },
];

test('every report has a description in a line', () => {
  assert.match(describeReport('pl'), /net income/);
  assert.match(describeReport('list:customers', 'Customer Contact List'), /Customer Contact List/);
  assert.match(describeReport('saved:3', 'Monthly sales'), /memorized/);
  assert.match(describeReport('preset:sales-detail', 'Sales by Customer Detail'), /Every transaction/);
  assert.equal(describeReport('unknown'), '');
});

test('search narrows names, descriptions and groups, every word in any order', () => {
  const names = (gs) => gs.flatMap((g) => g.reports.map((r) => r[1]));
  assert.deepEqual(names(searchReports(groups, 'balance')), ['Balance Sheet']);
  assert.deepEqual(names(searchReports(groups, '  aging   a/r ')), ['A/R Aging Summary']);
  assert.deepEqual(names(searchReports(groups, 'net income')), ['Profit and Loss'], 'found by its description');
  assert.deepEqual(names(searchReports(groups, 'who owes')), ['A/R Aging Summary', 'Open Invoices'], 'found by its group');
  assert.deepEqual(searchReports(groups, 'nothing like this'), []);
  assert.equal(searchReports(groups, ''), groups);
});

test('favorites toggle; recent keeps the newest first without repeats', () => {
  assert.deepEqual(toggleFavorite(['pl'], 'bs'), ['pl', 'bs']);
  assert.deepEqual(toggleFavorite(['pl', 'bs'], 'pl'), ['bs']);
  assert.deepEqual(pushRecent(['bs', 'pl'], 'pl'), ['pl', 'bs']);
  assert.deepEqual(pushRecent(['a', 'b', 'c'], 'd', 3), ['d', 'a', 'b']);
});

test('Favorites and Recent go on top with only reports still in the list', () => {
  const out = withFavoritesAndRecent(groups, ['bs', 'saved:9'], ['ar', 'pl']);
  assert.deepEqual(out.slice(0, 2).map((g) => [g.title, g.reports.map((r) => r[0])]), [['Favorites', ['bs']], ['Recent', ['ar', 'pl']]]);
  assert.equal(out.length, groups.length + 2);
  assert.equal(withFavoritesAndRecent(groups, [], []).length, groups.length);
});

test('the dates chosen in the center open the report on them', () => {
  assert.deepEqual(datesFor('pl', '2026-01-01', '2026-03-31'), { startDate: '2026-01-01', endDate: '2026-03-31' });
  assert.deepEqual(datesFor('bs', '2026-01-01', '2026-03-31'), { asOf: '2026-03-31' });
  assert.equal(datesFor('tb', '2026-01-01', '2026-03-31'), null, 'a report without dates opens as usual');
  assert.equal(datesFor('pl', '2026-04-01', '2026-03-31'), null);
});

test('reports are previewable; the report center tools and non-ids are not', async () => {
  const { previewable } = await import('./reportCenter.js');
  assert.equal(previewable('pl'), true);
  assert.equal(previewable('preset:journal'), true);
  assert.equal(previewable('list:customer-phone'), true);
  assert.equal(previewable('process-multiple'), false);
  assert.equal(previewable('scheduled-reports'), false);
  assert.equal(previewable(null), false);
  assert.equal(previewable({ id: 'pl' }), false);
  assert.equal(previewable(''), false);
});
