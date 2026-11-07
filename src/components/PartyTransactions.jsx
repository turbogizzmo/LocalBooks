import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import TransactionHistory from './TransactionHistory.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, periodRange } from '../lib/dates.js';
import { useOpenFromList } from '../lib/windows.js';
import { opensOwnForm, transactionTarget } from '../lib/openTransaction.js';
import { quickReport, reportShortcut, transactionJournal, useReportOpener } from '../lib/quickReport.js';
import DateBox from './DateBox.jsx';

/** How many transactions the panel lists; the count and total still cover them all. */
const LIMIT = 500;

/** The transaction types each kind of name has, as journal entries show them. */
const TYPES = {
  customer: ['INVOICE', 'RECEIVE_PAYMENT', 'SALES_RECEIPT', 'CREDIT_MEMO', 'JOURNAL'],
  vendor: ['BILL', 'BILL_PAYMENT', 'CHECK', 'EXPENSE', 'CARD_CREDIT', 'VENDOR_CREDIT', 'JOURNAL'],
};

const DATE_FILTERS = [
  ['all', 'All dates'],
  ['thisMonth', 'This month'],
  ['thisQuarter', 'This quarter'],
  ['thisYear', 'This year'],
  ['lastYear', 'Last year'],
  ['custom', 'Custom'],
];

/**
 * Every transaction with one customer or vendor, newest first, under the Customers or Vendors list
 * (as a customer or vendor center shows it). Double-click a transaction to open it.
 */
export default function PartyTransactions({ kind, party, onClose }) {
  const [includeVoided, setIncludeVoided] = useState(false);
  const [type, setType] = useState('');
  const [dates, setDates] = useState('all');
  const [[from, to], setRange] = useState(['', '']);
  const [selected, setSelected] = useState(null);
  const [history, setHistory] = useState(null);
  const openFromList = useOpenFromList();
  const openReport = useReportOpener();
  const chooseDates = (id) => {
    setDates(id);
    if (id === 'all') setRange(['', '']);
    else if (id !== 'custom') setRange(periodRange(id));
  };
  // Without an end date the search stops at today, so "All dates" also reaches post-dated ones.
  const query = {
    filters: { names: [{ kind, id: party.id }], excludeVoided: !includeVoided, transactionTypes: type ? [type] : [] },
    startDate: from || null,
    endDate: to || (dates === 'all' ? '9999-12-31' : null),
    limit: LIMIT,
  };
  const { data, error, reload } = useLedgerQuery(() => api.findTransactions(query));
  useEffect(() => {
    reload();
  }, [kind, party.id, includeVoided, type, from, to, dates, reload]);
  useEffect(() => {
    setSelected(null);
    setType('');
  }, [kind, party.id]);

  const sorter = useSort(
    data?.rows,
    {
      date: (r) => r.date,
      type: (r) => r.typeLabel,
      num: (r) => r.num,
      account: (r) => r.account,
      memo: (r) => r.memo,
      amount: (r) => r.amountCents,
    },
    { storageKey: 'party-transactions' },
  );
  const open = (r) => openFromList(...transactionTarget(r));
  const selectedRow = data?.rows.find((r) => r.journalEntryId === selected) ?? null;
  const runJournal = (r) => openReport(transactionJournal({ journalEntryId: r.journalEntryId, label: `${r.typeLabel}${r.num ? ` #${r.num}` : ''}` }));
  // Ctrl+H and Ctrl+Y act on the selected transaction; Ctrl+Q is the list's (the name's QuickReport).
  const onKeyDown = (e) => {
    const key = reportShortcut(e);
    if (!selectedRow || (key !== 'history' && key !== 'journal')) return;
    e.preventDefault();
    if (key === 'history') setHistory(selectedRow.journalEntryId);
    else runJournal(selectedRow);
  };

  return (
    <section className="qb-panel flex h-[40%] min-h-[160px] flex-none flex-col outline-none" aria-label={`Transactions with ${party.name}`} tabIndex={-1} onKeyDown={onKeyDown}>
      {history !== null && <TransactionHistory journalEntryId={history} onClose={() => setHistory(null)} onOpen={openFromList} />}
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="truncate text-md font-semibold">Transactions · {party.name}</h2>
        {data && (
          <span className="text-xs text-qb-ink-muted">
            {data.matchCount} transaction{data.matchCount === 1 ? '' : 's'}
            {data.matchCount > data.rows.length && ` · newest ${data.rows.length} shown`} · double-click to open
          </span>
        )}
        <select className="qb-input ml-auto h-row w-[150px] text-sm" value={type} onChange={(e) => setType(e.target.value)} aria-label="Transaction type">
          <option value="">All types</option>
          {TYPES[kind].map((t) => (
            <option key={t} value={t}>{SOURCE_LABEL[t] ?? t}</option>
          ))}
        </select>
        <select className="qb-input h-row w-[120px] text-sm" value={dates} onChange={(e) => chooseDates(e.target.value)} aria-label="Dates">
          {DATE_FILTERS.map(([id, label]) => (
            <option key={id} value={id}>{label}</option>
          ))}
        </select>
        {dates !== 'all' && (
          <>
            <DateBox className="qb-input h-row w-[128px] text-sm" value={from} onChange={(e) => { setDates('custom'); setRange([e.target.value, to]); }} aria-label="From" />
            <DateBox className="qb-input h-row w-[128px] text-sm" value={to} onChange={(e) => { setDates('custom'); setRange([from, e.target.value]); }} aria-label="To" />
          </>
        )}
        <label className="flex items-center gap-1 text-sm">
          <input type="checkbox" className="accent-qb-green" checked={includeVoided} onChange={(e) => setIncludeVoided(e.target.checked)} />
          Show voided
        </label>
        <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => openReport(quickReport({ kind, id: party.id, name: party.name, startDate: from, endDate: to }))} title="These transactions as a report (Ctrl+Q)">
          QuickReport
        </button>
        <button type="button" className="text-sm text-qb-link hover:underline" onClick={onClose} aria-label="Close transactions">
          Close
        </button>
      </div>
      {error && <Notice kind="error" text={error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <colgroup>
            <col className="w-[92px]" />
            <col className="w-[130px]" />
            <col className="w-[80px]" />
            <col className="w-[200px]" />
            <col />
            <col className="w-[120px]" />
          </colgroup>
          <thead>
            <tr>
              <SortTh sorter={sorter} col="date">Date</SortTh>
              <SortTh sorter={sorter} col="type">Type</SortTh>
              <SortTh sorter={sorter} col="num">Num</SortTh>
              <SortTh sorter={sorter} col="account">Account</SortTh>
              <SortTh sorter={sorter} col="memo">Memo</SortTh>
              <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
            </tr>
          </thead>
          <tbody>
            {data && data.rows.length === 0 && (
              <tr>
                <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                  {type || dates !== 'all' ? `No ${type ? (SOURCE_LABEL[type] ?? type).toLowerCase() + ' ' : ''}transactions with ${party.name} for these dates.` : `No transactions with ${party.name} yet.`}
                </td>
              </tr>
            )}
            {sorter.sorted.map((r) => (
              <tr
                key={r.journalEntryId}
                className={`qb-grid-row cursor-pointer ${r.journalEntryId === selected ? 'is-active' : ''} ${r.isVoided || r.isReversal ? 'text-qb-ink-faint' : ''}`}
                aria-selected={r.journalEntryId === selected}
                onClick={() => setSelected(r.journalEntryId)}
                onDoubleClick={() => open(r)}
                title={opensOwnForm(r) ? `Double-click to open this ${(SOURCE_LABEL[r.sourceType] ?? 'transaction').toLowerCase()}` : 'Double-click to open in the journal'}
              >
                <td className="qb-grid-cell">{formatDate(r.date)}</td>
                <td className="qb-grid-cell">
                  {r.typeLabel}
                  {r.isVoided && <span className="ml-1 text-2xs font-semibold uppercase text-qb-negative">Voided</span>}
                </td>
                <td className="qb-grid-cell">{r.num ?? ''}</td>
                <td className="qb-grid-cell truncate">{r.account}</td>
                <td className="qb-grid-cell truncate text-qb-ink-muted">{r.memo ?? ''}</td>
                <td className="qb-grid-cell num">{formatCents(r.amountCents)}</td>
              </tr>
            ))}
          </tbody>
          {data && data.rows.length > 0 && (
            <tfoot>
              <tr>
                {/* Invoices and payments move a balance in opposite directions, so only one type at a time adds up. */}
                <td className="qb-grid-cell font-semibold" colSpan={5}>
                  {type ? `Total ${(SOURCE_LABEL[type] ?? type).toLowerCase()}` : `${data.matchCount} ${data.matchCount === 1 ? 'transaction' : 'transactions'}`}
                </td>
                <td className="qb-grid-cell num font-semibold" title={type ? undefined : 'Choose one type to see its total'}>
                  {type ? formatCents(data.totalCents) : ''}
                </td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </section>
  );
}
