// Pure helpers for comments on report figures (see components/ReportComments.jsx).

/** A key for a cell of a kept report. */
export const cellKey = (row, col) => `${row}:${col}`;

/** Numbers each commented cell 1, 2, 3… in reading order (row, then column): cell key → number. */
export function commentNumbers(comments) {
  const sorted = [...comments].sort((a, b) => a.rowIndex - b.rowIndex || a.colIndex - b.colIndex);
  const keys = [...new Set(sorted.map((c) => cellKey(c.rowIndex, c.colIndex)))];
  return new Map(keys.map((k, i) => [k, i + 1]));
}

/**
 * A financial statement (Profit and Loss, Balance Sheet: `lines` with a label and one amount, or
 * one per column) in the report engine's shape, so it is kept and printed like any report.
 */
export function statementSnapshot(lines, columns) {
  const heads = [{ label: '', kind: 'text' }, ...(columns ?? [{ label: 'Total' }]).map((c) => ({ label: c.label, kind: 'money' }))];
  const rows = lines.map((l) => ({
    kind: l.kind === 'account' ? 'detail' : l.kind,
    depth: l.depth ?? 0,
    cells: [l.label, ...(columns ? l.amountsCents : [l.amountCents]).map((v) => (v === undefined ? null : v))],
  }));
  return { columns: heads, rows };
}

/** A report in the engine's shape, reduced to what is kept: column labels and kinds, and rows. */
export function reportSnapshot(report) {
  return {
    columns: report.columns.map((c) => ({ label: c.label, kind: c.kind })),
    rows: report.rows.map((r) => ({ kind: r.kind, depth: r.depth ?? 0, cells: r.kind === 'section' ? [r.cells[0], ...report.columns.slice(1).map(() => null)] : r.cells })),
  };
}
