import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { useOpenFromList } from '../lib/windows.js';
import { opensOwnForm, transactionTarget } from '../lib/openTransaction.js';

/** How many transactions the panel lists; the count and total still cover them all. */
const LIMIT = 500;

/**
 * Every transaction with one customer or vendor, newest first, under the Customers or Vendors list
 * (as a customer or vendor center shows it). Double-click a transaction to open it.
 */
export default function PartyTransactions({ kind, party, onClose }) {
  const [includeVoided, setIncludeVoided] = useState(false);
  const openFromList = useOpenFromList();
  const query = { filters: { names: [{ kind, id: party.id }], excludeVoided: !includeVoided }, limit: LIMIT };
  const { data, error, reload } = useLedgerQuery(() => api.findTransactions(query));
  useEffect(() => {
    reload();
  }, [kind, party.id, includeVoided, reload]);

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

  return (
    <section className="qb-panel flex h-[40%] min-h-[160px] flex-none flex-col" aria-label={`Transactions with ${party.name}`}>
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="truncate text-md font-semibold">Transactions · {party.name}</h2>
        {data && (
          <span className="text-xs text-qb-ink-muted">
            {data.matchCount} transaction{data.matchCount === 1 ? '' : 's'}
            {data.matchCount > data.rows.length && ` · newest ${data.rows.length} shown`} · double-click to open
          </span>
        )}
        <label className="ml-auto flex items-center gap-1 text-sm">
          <input type="checkbox" className="accent-qb-green" checked={includeVoided} onChange={(e) => setIncludeVoided(e.target.checked)} />
          Show voided
        </label>
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
                  No transactions with {party.name} yet.
                </td>
              </tr>
            )}
            {sorter.sorted.map((r) => (
              <tr
                key={r.journalEntryId}
                className={`qb-grid-row cursor-pointer ${r.isVoided || r.isReversal ? 'text-qb-ink-faint' : ''}`}
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
                <td className="qb-grid-cell font-semibold" colSpan={5}>
                  Total
                </td>
                <td className="qb-grid-cell num font-semibold">{formatCents(data.totalCents)}</td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </section>
  );
}
