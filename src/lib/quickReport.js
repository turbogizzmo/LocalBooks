// QuickReport (Ctrl+Q), Transaction History (Ctrl+H) and Transaction Journal (Ctrl+Y): what each
// opens, as `[view, action, data]` for a window opener. QuickReport and the journal are presets of
// the custom report engine (`custom_reports.rs`) narrowed to what was selected.

import { useCallback, useContext } from 'react';
import { WindowManagerContext } from './windows.js';

/** The preset QuickReports start from; their columns and filters replace its own. */
const BASE = 'preset:transaction-list-date';

const range = (startDate, endDate) =>
  startDate || endDate ? { period: 'custom', startDate: startDate || null, endDate: endDate || null } : { period: 'all', startDate: null, endDate: null };

/**
 * A QuickReport for an account (its lines with a running balance), a customer or vendor (their
 * transactions), or a product or service (the lines it is on). `startDate` and `endDate` narrow
 * it, as a register's date filter does; without them it covers all dates.
 */
export function quickReport({ kind, id, name, startDate = null, endDate = null }) {
  const title = `QuickReport: ${name}`;
  let settings;
  let filters;
  if (kind === 'account') {
    settings = { source: 'ledger', mode: 'detail', columns: ['type', 'date', 'num', 'name', 'memo', 'split', 'amount', 'balance'], groupBy: 'account', perTransaction: false, sign: 'natural', openingBalances: Boolean(startDate) };
    filters = { accountIds: [id] };
  } else if (kind === 'customer' || kind === 'vendor') {
    settings = { source: 'ledger', mode: 'detail', columns: ['type', 'date', 'num', 'memo', 'account', 'split', 'amount'], groupBy: 'name', perTransaction: true, sign: 'natural' };
    filters = { names: [{ kind, id }] };
  } else if (kind === 'item') {
    settings = { source: 'items', mode: 'detail', columns: ['type', 'date', 'num', 'name', 'memo', 'qty', 'rate', 'amount', 'balance'], groupBy: 'item', perTransaction: false, showQty: true, sign: 'natural' };
    filters = { itemIds: [id] };
  } else {
    throw new Error(`QuickReport: unknown kind ${kind}`);
  }
  return ['reports', 'open', { report: BASE, zoom: { title, settings, filters, ...range(startDate, endDate) } }];
}

/** The Transaction Journal: the debits and credits one transaction posted. */
export function transactionJournal({ journalEntryId, label }) {
  return [
    'reports',
    'open',
    { report: 'preset:journal', zoom: { title: `Transaction Journal${label ? `: ${label}` : ''}`, filters: { entryIds: [journalEntryId] }, period: 'all', startDate: null, endDate: null } },
  ];
}

/**
 * Ctrl+Q, Ctrl+H and Ctrl+Y as the classic desktop software has them. Ctrl rather than Cmd on a
 * Mac too, where Cmd+Q quits and Cmd+H hides the app. Returns 'quickReport', 'history',
 * 'journal' or null.
 */
export function reportShortcut(e) {
  if (!e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  switch (e.key.toLowerCase()) {
    case 'q':
      return 'quickReport';
    case 'h':
      return 'history';
    case 'y':
      return 'journal';
    default:
      return null;
  }
}

/** Opens a `[view, action, data]` report target in a window of its own. */
export function useReportOpener() {
  const { open } = useContext(WindowManagerContext);
  return useCallback(([view, action, data]) => open(view, action, data, { forceNew: true }), [open]);
}
