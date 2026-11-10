import { test } from 'node:test';
import assert from 'node:assert/strict';
import { commentNumbers, reportSnapshot, statementSnapshot } from './reportComments.js';

test('commented figures are numbered in reading order, one number per cell', () => {
  const n = commentNumbers([
    { rowIndex: 5, colIndex: 1 },
    { rowIndex: 2, colIndex: 2 },
    { rowIndex: 2, colIndex: 1 },
    { rowIndex: 5, colIndex: 1 },
  ]);
  assert.deepEqual([...n.entries()], [['2:1', 1], ['2:2', 2], ['5:1', 3]]);
});

test('a statement keeps its labels and amounts in the report shape', () => {
  const lines = [
    { kind: 'section', depth: 0, label: 'Income', amountCents: null },
    { kind: 'account', depth: 1, label: 'Design services', amountCents: 1250000, accountId: 7 },
    { kind: 'total', depth: 0, label: 'Total Income', amountCents: 1250000 },
  ];
  assert.deepEqual(statementSnapshot(lines, null), {
    columns: [{ label: '', kind: 'text' }, { label: 'Total', kind: 'money' }],
    rows: [
      { kind: 'section', depth: 0, cells: ['Income', null] },
      { kind: 'detail', depth: 1, cells: ['Design services', 1250000] },
      { kind: 'total', depth: 0, cells: ['Total Income', 1250000] },
    ],
  });
  const byClass = statementSnapshot([{ kind: 'account', depth: 0, label: 'Rent', amountsCents: [100, 200, 300] }], [{ label: 'East' }, { label: 'West' }, { label: 'Total' }]);
  assert.deepEqual(byClass.columns.map((c) => c.label), ['', 'East', 'West', 'Total']);
  assert.deepEqual(byClass.rows[0].cells, ['Rent', 100, 200, 300]);
});

test('a report keeps only its labels, kinds and cells', () => {
  const report = { columns: [{ label: 'Name', kind: 'text', extra: 1 }, { label: 'Amount', kind: 'money' }], rows: [{ kind: 'section', depth: 0, cells: ['Acme'], drill: {} }, { kind: 'detail', cells: ['Inv 1', 500], journalEntryId: 3 }] };
  assert.deepEqual(reportSnapshot(report), {
    columns: [{ label: 'Name', kind: 'text' }, { label: 'Amount', kind: 'money' }],
    rows: [{ kind: 'section', depth: 0, cells: ['Acme', null] }, { kind: 'detail', depth: 0, cells: ['Inv 1', 500] }],
  });
});
