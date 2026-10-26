import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { MoneyBound, MultiPick } from './CustomReport.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { PERIODS, resolvePeriod } from './ReportParts.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/**
 * Find transactions (`custom_reports/find.rs`), like classic desktop accounting software's Edit ›
 * Find: posted transactions by date, type, name, account, amount, number and text, one row per
 * transaction. ⌘K's "Advanced search…" opens it with the typed text.
 */

const RANGES = [{ id: 'all', label: 'All dates' }, ...PERIODS.filter((p) => p.id !== 'custom'), { id: 'custom', label: 'Custom' }];
const AMOUNT_MODES = [['any', 'Any amount'], ['equal', 'Equal to'], ['atLeast', 'At least'], ['atMost', 'At most'], ['between', 'Between']];
const COLUMNS = [
  ['date', 'Date', 'w-[92px]'], ['type', 'Type', 'w-[150px]'], ['num', 'Num', 'w-[80px]'], ['name', 'Name', 'w-[180px]'],
  ['memo', 'Memo', ''], ['account', 'Account', 'w-[200px]'], ['amount', 'Amount', 'w-[120px]'],
];
const SORT_VALUE = {
  date: (r) => r.date, type: (r) => r.typeLabel, num: (r) => r.num ?? '', name: (r) => (r.name ?? '').toLowerCase(),
  memo: (r) => (r.memo ?? '').toLowerCase(), account: (r) => r.account.toLowerCase(), amount: (r) => r.amountCents,
};
const BLANK = { text: '', number: '', types: [], names: [], accountIds: [], amountMode: 'any', amountA: null, amountB: null, includeVoided: false };

const nameKey = (n) => (typeof n === 'object' ? `${n.kind}:${n.id}` : String(n));

/** `value` (a string or number: objects would reset the timer every render), once it has stopped changing for `ms`. */
function useSettled(value, ms = 250) {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

function amountBounds(mode, a, b) {
  switch (mode) {
    case 'equal':
      return [a, a];
    case 'atLeast':
      return [a, null];
    case 'atMost':
      return [null, a];
    case 'between':
      return [a, b];
    default:
      return [null, null];
  }
}

export default function FindTransactions({ intent, navigate }) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const vendors = useLedgerQuery(api.listVendors).data ?? [];
  const accounts = useLedgerQuery(api.listAccounts).data ?? [];
  const [f, setF] = useState(() => ({ ...BLANK, text: intent?.action === 'find' ? (intent.data?.text ?? '') : '' }));
  const [period, setPeriod] = useState('all');
  const [[start, end], setRange] = useState([null, todayIso()]);
  const [sort, setSort] = useState({ key: 'date', desc: true });
  const update = (patch) => setF((x) => ({ ...x, ...patch }));

  // ⌘K's Advanced search… while Find is already open.
  useEffect(() => {
    if (intent?.action !== 'find') return;
    setF({ ...BLANK, text: intent.data?.text ?? '' });
    setPeriod('all');
    setRange([null, todayIso()]);
  }, [intent?.nonce]); // eslint-disable-line react-hooks/exhaustive-deps

  const choosePeriod = (id) => {
    setPeriod(id);
    if (id === 'all') setRange([null, todayIso()]);
    else if (id !== 'custom') setRange(resolvePeriod(id, fiscal));
  };

  const [min, max] = amountBounds(f.amountMode, f.amountA, f.amountB);
  const text = useSettled(f.text);
  const number = useSettled(f.number);
  const query = {
    startDate: start,
    endDate: end,
    text: text.trim() || null,
    number: number.trim() || null,
    filters: { transactionTypes: f.types, names: f.names, accountIds: f.accountIds, amountMinCents: min, amountMaxCents: max, excludeVoided: !f.includeVoided },
  };
  const valid = Boolean(end) && (!start || start <= end) && (min === null || max === null || min <= max);
  const queryKey = JSON.stringify(query);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.findTransactions(query) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [queryKey, reload]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows = useMemo(() => {
    const value = SORT_VALUE[sort.key];
    const sorted = [...(data?.rows ?? [])].sort((a, b) => {
      const [x, y] = [value(a), value(b)];
      return x < y ? -1 : x > y ? 1 : b.journalEntryId - a.journalEntryId;
    });
    return sort.desc ? sorted.reverse() : sorted;
  }, [data, sort]);

  const names = [
    ...customers.map((c) => ({ value: { kind: 'customer', id: c.id }, label: `${c.name} (customer)` })),
    ...vendors.map((v) => ({ value: { kind: 'vendor', id: v.id }, label: `${v.name} (vendor)` })),
  ];
  const filtered = JSON.stringify({ ...f, period }) !== JSON.stringify({ ...BLANK, period: 'all' });

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <section className="qb-panel flex flex-none flex-col gap-1.5 px-2 py-1.5" aria-label="Find filters">
        <div className="flex flex-wrap items-end gap-2">
          <label>
            <span className="qb-label">Name, memo or number contains</span>
            <input type="search" className="qb-input w-[240px]" autoFocus value={f.text} onChange={(e) => update({ text: e.target.value })} />
          </label>
          <label>
            <span className="qb-label">Dates</span>
            <select className="qb-input w-[150px]" value={period} onChange={(e) => choosePeriod(e.target.value)}>
              {RANGES.map((p) => (
                <option key={p.id} value={p.id}>{p.label}</option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">From</span>
            <input type="date" className="qb-input w-[132px]" value={start ?? ''} onChange={(e) => { setPeriod('custom'); setRange([e.target.value || null, end]); }} />
          </label>
          <label>
            <span className="qb-label">To</span>
            <input type="date" className="qb-input w-[132px]" value={end ?? ''} aria-invalid={Boolean(start && end && start > end)} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value || todayIso()]); }} />
          </label>
          <label>
            <span className="qb-label">Number</span>
            <input className="qb-input w-[90px]" value={f.number} onChange={(e) => update({ number: e.target.value })} />
          </label>
          <label>
            <span className="qb-label">Amount</span>
            <select className="qb-input w-[120px]" value={f.amountMode} onChange={(e) => update({ amountMode: e.target.value })}>
              {AMOUNT_MODES.map(([v, l]) => (
                <option key={v} value={v}>{l}</option>
              ))}
            </select>
          </label>
          {f.amountMode !== 'any' && <MoneyBound label={f.amountMode === 'between' ? 'From' : 'Amount'} cents={f.amountA} onChange={(v) => update({ amountA: v })} />}
          {f.amountMode === 'between' && <MoneyBound label="To" cents={f.amountB} onChange={(v) => update({ amountB: v })} />}
          <label className="flex h-row items-center gap-1 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={f.includeVoided} onChange={(e) => update({ includeVoided: e.target.checked })} />
            Include voided
          </label>
          {filtered && (
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => { setF(BLANK); choosePeriod('all'); }}>
              Clear filters
            </button>
          )}
        </div>
        <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
          <MultiPick label="Transaction types" options={Object.entries(SOURCE_LABEL).map(([code, l]) => ({ value: code, label: l }))} value={f.types} onChange={(v) => update({ types: v })} />
          <MultiPick label="Customers and vendors" options={names} keyOf={nameKey} value={f.names} onChange={(v) => update({ names: v })} />
          <MultiPick label="Accounts (with subaccounts)" options={accounts.map((a) => ({ value: a.id, label: `${a.accountNumber ? `${a.accountNumber} ` : ''}${a.fullName}` }))} value={f.accountIds} onChange={(v) => update({ accountIds: v })} />
        </div>
      </section>
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Find transactions</h2>
          <span className="text-xs text-qb-ink-muted">Amounts are each transaction’s total · click a row to open it</span>
          {data && (
            <span className="ml-auto text-sm">
              {data.matchCount} {data.matchCount === 1 ? 'match' : 'matches'}
              {data.matchCount > data.rows.length && <span className="text-qb-ink-muted"> (showing the newest {data.rows.length})</span>}
              {' · '}Total <span className="num font-semibold">{formatCents(data.totalCents)}</span>
            </span>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              {COLUMNS.map(([key, , width]) => (
                <col key={key} className={width} />
              ))}
            </colgroup>
            <thead>
              <tr>
                {COLUMNS.map(([key, label]) => (
                  <th key={key} className={`qb-grid-head ${key === 'amount' ? 'text-right' : ''}`} aria-sort={sort.key === key ? (sort.desc ? 'descending' : 'ascending') : 'none'}>
                    <button type="button" className="w-full text-inherit hover:underline" style={{ textAlign: 'inherit' }} onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key === 'date' || key === 'amount' }))}>
                      {label}
                      {sort.key === key && (sort.desc ? ' ▾' : ' ▴')}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {!valid && (
                <tr>
                  <td colSpan={COLUMNS.length} className="qb-grid-cell text-qb-negative">The dates or amounts are the wrong way round.</td>
                </tr>
              )}
              {valid && data && rows.length === 0 && (
                <tr>
                  <td colSpan={COLUMNS.length} className="qb-grid-cell text-qb-ink-muted">No transactions match these filters.</td>
                </tr>
              )}
              {rows.map((r) => (
                <tr
                  key={r.journalEntryId}
                  className={`qb-grid-row cursor-pointer ${r.isVoided || r.isReversal ? 'text-qb-ink-muted' : ''}`}
                  onClick={() => navigate('accounting/journal', 'open-entry', { entryId: r.journalEntryId })}
                  title="Open this transaction"
                >
                  <td className="qb-grid-cell">{formatDate(r.date)}</td>
                  <td className="qb-grid-cell">{r.typeLabel}{r.isVoided && ' (voided)'}</td>
                  <td className="qb-grid-cell">{r.num ?? ''}</td>
                  <td className="qb-grid-cell" title={r.name ?? ''}>{r.name ?? ''}</td>
                  <td className="qb-grid-cell" title={r.memo ?? ''}>{r.memo ?? ''}</td>
                  <td className="qb-grid-cell" title={r.account}>{r.account}</td>
                  <td className="qb-grid-cell num">{formatCents(r.amountCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
