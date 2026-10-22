import { useEffect, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import ClassSelect, { useClasses } from './ClassSelect.jsx';
import Notice from './Notice.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, periodRange } from '../lib/dates.js';

const isPl = (a) => a.classification === 'INCOME' || a.classification === 'EXPENSE';

/**
 * Moves posted income and expense lines to another class. Posted lines never change, so each move
 * posts a reclassifying journal entry on the line's own date: the same account, out of the old
 * class and into the new one. Voiding that entry undoes the move.
 */
export default function Reclassify({ accounts }) {
  const classes = useClasses();
  const [[start, end], setRange] = useState(() => periodRange('thisYearToDate'));
  const [accountId, setAccountId] = useState(null);
  // '' = any class, 'none' = lines with no class, otherwise a class id (with its subclasses).
  const [current, setCurrent] = useState('');
  const [picked, setPicked] = useState(() => new Set());
  const [target, setTarget] = useState(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const query = {
    startDate: start,
    endDate: end,
    accountId,
    classId: current === '' || current === 'none' ? null : Number(current),
    unclassified: current === 'none',
  };
  const { data, error, reload } = useLedgerQuery(() => api.listReclassLines(query));
  useEffect(() => {
    if (start && end && start <= end) reload();
    setPicked(new Set());
  }, [start, end, accountId, current, reload]);

  const lines = data ?? [];
  // Amounts on each account's normal side: income as a positive credit, expenses as a positive debit.
  const natural = (l) => (accounts.find((a) => a.id === l.accountId)?.classification === 'INCOME' ? -l.amountCents : l.amountCents);
  const allPicked = lines.length > 0 && lines.every((l) => picked.has(l.lineId));
  const toggle = (id) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const move = async () => {
    if (picked.size === 0) return setNotice({ kind: 'error', text: 'Tick the lines to move.' });
    setBusy(true);
    try {
      const moved = await api.reclassifyLines([...picked], target);
      const name = target === null ? 'no class' : classes.find((c) => c.id === target)?.fullName;
      setNotice({
        kind: 'success',
        text: moved === 0 ? `Nothing to move: the ticked lines are already in ${name}.` : `Moved ${moved} line${moved === 1 ? '' : 's'} to ${name} with a reclassifying journal entry on each line's date.`,
      });
      setPicked(new Set());
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">From</span>
          <input type="date" className="qb-input w-[132px]" value={start} onChange={(e) => setRange([e.target.value, end])} />
        </label>
        <label>
          <span className="qb-label">To</span>
          <input type="date" className="qb-input w-[132px]" value={end} onChange={(e) => setRange([start, e.target.value])} aria-invalid={end < start} />
        </label>
        <label>
          <span className="qb-label">Account</span>
          <AccountSelect accounts={accounts} filter={isPl} value={accountId} onChange={setAccountId} placeholder="All income and expense accounts" className="qb-input w-[240px]" />
        </label>
        <label>
          <span className="qb-label">Current class</span>
          <select className="qb-input w-[180px]" value={current} onChange={(e) => setCurrent(e.target.value)}>
            <option value="">Any class</option>
            {classes.map((c) => (
              <option key={c.id} value={c.id}>
                {'  '.repeat(c.depth)}
                {c.name}
              </option>
            ))}
            <option value="none">No class</option>
          </select>
        </label>
        <span className="ml-auto self-center text-xs text-qb-ink-muted">Posted lines never change: each move is a reclassifying journal entry you can void.</span>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Income and expense lines</h2>
          <span className="text-xs text-qb-ink-muted">{picked.size > 0 ? `${picked.size} ticked` : `${lines.length} line${lines.length === 1 ? '' : 's'}`}</span>
          <span className="ml-auto text-sm">Move ticked lines to</span>
          <ClassSelect className="qb-input h-row w-[170px] text-sm" classes={classes} value={target} onChange={setTarget} aria-label="New class" />
          <button type="button" className="qb-btn qb-btn-primary h-row" onClick={move} disabled={busy || picked.size === 0}>
            {busy ? 'Moving…' : 'Reclassify'}
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[30px]" />
              <col className="w-[92px]" />
              <col className="w-[120px]" />
              <col className="w-[56px]" />
              <col className="w-[230px]" />
              <col />
              <col className="w-[180px]" />
              <col className="w-[120px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input type="checkbox" checked={allPicked} onChange={(e) => setPicked(e.target.checked ? new Set(lines.map((l) => l.lineId)) : new Set())} aria-label="Tick all lines" />
                </th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Account</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head">Class</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data && lines.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No income or expense lines match.</td>
                </tr>
              )}
              {lines.map((l) => (
                <tr key={l.lineId} className={`qb-grid-row cursor-pointer ${picked.has(l.lineId) ? 'is-active' : ''}`} onClick={() => toggle(l.lineId)}>
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" checked={picked.has(l.lineId)} onChange={() => toggle(l.lineId)} onClick={(e) => e.stopPropagation()} aria-label={`Tick line ${l.lineId}`} />
                  </td>
                  <td className="qb-grid-cell">{formatDate(l.entryDate)}</td>
                  <td className="qb-grid-cell">{SOURCE_LABEL[l.sourceType] ?? l.sourceType}</td>
                  <td className="qb-grid-cell">{l.entryNumber}</td>
                  <td className="qb-grid-cell" title={l.accountName}>{l.accountName}</td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={l.memo ?? ''}>{l.memo ?? ''}</td>
                  <td className="qb-grid-cell">{l.className ?? <span className="text-qb-ink-faint">No class</span>}</td>
                  <td className="qb-grid-cell num">{formatCents(natural(l))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
