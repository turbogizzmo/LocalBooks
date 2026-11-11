import { useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import DateBox from './DateBox.jsx';
import { enteredText, skippedLines } from '../lib/recurringReport.js';

const KIND_LABEL = { JOURNAL: 'Journal entry', EXPENSE: 'Expense / check', BILL: 'Bill', INVOICE: 'Invoice' };
const FREQUENCIES = [
  ['WEEKLY', 'Week'],
  ['MONTHLY', 'Month'],
  ['QUARTERLY', 'Quarter'],
  ['YEARLY', 'Year'],
];

/** The next scheduled date after `iso` (matching the backend's month arithmetic: the same day, or the month's last day when it is shorter). */
export function nextOccurrence(iso, frequency, every = 1) {
  const [y, m, d] = iso.split('-').map(Number);
  if (frequency === 'WEEKLY') {
    const t = new Date(Date.UTC(y, m - 1, d + 7 * every));
    return t.toISOString().slice(0, 10);
  }
  const months = (frequency === 'QUARTERLY' ? 3 : frequency === 'YEARLY' ? 12 : 1) * every;
  const total = m - 1 + months;
  const ny = y + Math.floor(total / 12);
  const nm = total % 12;
  const last = new Date(Date.UTC(ny, nm + 1, 0)).getUTCDate();
  return `${ny}-${String(nm + 1).padStart(2, '0')}-${String(Math.min(d, last)).padStart(2, '0')}`;
}

export const scheduleText = (frequency, every) => {
  const unit = FREQUENCIES.find(([f]) => f === frequency)?.[1].toLowerCase() ?? frequency;
  return every === 1 ? `Every ${unit}` : `Every ${every} ${unit}s`;
};

/**
 * Memorize Transaction: saves the form's transaction as a recurring template. `date` is the form's
 * date; the first scheduled occurrence defaults to one period after it, since the form usually
 * posts this one.
 */
export function MemorizeDialog({ kind, payload, date, defaultName, onClose, onSaved }) {
  const [name, setName] = useState(defaultName ?? '');
  const [frequency, setFrequency] = useState('MONTHLY');
  const [every, setEvery] = useState('1');
  const [nextDate, setNextDate] = useState(() => nextOccurrence(date ?? todayIso(), 'MONTHLY'));
  const [endDate, setEndDate] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const n = Number(every);
  const save = async () => {
    setBusy(true);
    try {
      // The memorized transaction's own day of the month: a Jan 31 rent first repeats on Feb 28 and
      // then goes back to the 31st (the backend keeps it only when Next date is that day or month end).
      const anchorDay = frequency === 'WEEKLY' ? null : Number((date ?? todayIso()).slice(8, 10));
      await api.saveRecurring(null, { name: name.trim(), kind, payload, frequency, every: n, nextDate, endDate: endDate || null, anchorDay });
      onSaved(`“${name.trim()}” memorized: ${scheduleText(frequency, n).toLowerCase()}, next on ${formatDate(nextDate)}. Enter it from Accounting › Recurring Transactions.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Memorize transaction">
      <div className="qb-panel w-[440px] p-3 shadow-panel">
        <h2 className="mb-2 text-md font-semibold">Memorize {KIND_LABEL[kind]?.toLowerCase()}</h2>
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2">
            <span className="qb-label">Name</span>
            <input className="qb-input w-full" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </label>
          <label>
            <span className="qb-label">Repeat every</span>
            <div className="flex gap-1">
              <input
                className="qb-input num w-[52px]"
                value={every}
                onChange={(e) => {
                  const text = e.target.value.replace(/\D/g, '');
                  setEvery(text);
                  // The next date moves with the number of periods, as it does with the unit.
                  if (Number(text) >= 1) setNextDate(nextOccurrence(date ?? todayIso(), frequency, Number(text)));
                }}
                aria-label="Number of periods"
              />
              <select
                className="qb-input flex-1"
                value={frequency}
                onChange={(e) => {
                  setFrequency(e.target.value);
                  setNextDate(nextOccurrence(date ?? todayIso(), e.target.value, n || 1));
                }}
              >
                {FREQUENCIES.map(([f, label]) => (
                  <option key={f} value={f}>{label}{n > 1 ? 's' : ''}</option>
                ))}
              </select>
            </div>
          </label>
          <label>
            <span className="qb-label">Next date</span>
            <DateBox className="qb-input w-full" value={nextDate} onChange={(e) => setNextDate(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">End date (optional)</span>
            <DateBox className="qb-input w-full" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
          </label>
        </div>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>Cancel</button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy || !name.trim() || !(n >= 1) || !nextDate}>
            Memorize
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Edit… on Recurring Transactions: a memorized transaction's name and schedule. The transaction it
 * enters stays as memorized (to change that, enter a new one and memorize it).
 */
function ScheduleDialog({ template, onClose, onSaved }) {
  const [name, setName] = useState(template.name);
  const [frequency, setFrequency] = useState(template.frequency);
  const [every, setEvery] = useState(String(template.every));
  const [nextDate, setNextDate] = useState(template.nextDate);
  const [endDate, setEndDate] = useState(template.endDate ?? '');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      await api.editRecurringSchedule(template.id, { name: name.trim(), frequency, every: Number(every), nextDate, endDate: endDate || null });
      onSaved(`“${name.trim()}” saved: ${scheduleText(frequency, Number(every)).toLowerCase()}, next on ${formatDate(nextDate)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Edit memorized transaction">
      <div className="qb-panel w-[440px] p-3 shadow-panel">
        <h2 className="mb-2 text-md font-semibold">Edit {KIND_LABEL[template.kind]?.toLowerCase() ?? 'memorized transaction'}</h2>
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2">
            <span className="qb-label">Name</span>
            <input className="qb-input w-full" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </label>
          <label>
            <span className="qb-label">Repeat every</span>
            <div className="flex gap-1">
              <input className="qb-input num w-[52px]" value={every} onChange={(e) => setEvery(e.target.value.replace(/\D/g, ''))} aria-label="Number of periods" />
              <select className="qb-input flex-1" value={frequency} onChange={(e) => setFrequency(e.target.value)}>
                {FREQUENCIES.map(([f, label]) => (
                  <option key={f} value={f}>{label}{Number(every) > 1 ? 's' : ''}</option>
                ))}
              </select>
            </div>
          </label>
          <label>
            <span className="qb-label">Next date</span>
            <DateBox className="qb-input w-full" value={nextDate} onChange={(e) => setNextDate(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">End date (optional)</span>
            <DateBox className="qb-input w-full" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
          </label>
        </div>
        <p className="mt-2 text-xs text-qb-ink-muted">The transaction it enters stays as it was memorized.</p>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose}>Cancel</button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy || !name.trim() || !(Number(every) >= 1)}>Save</button>
        </div>
      </div>
    </div>
  );
}

/** How many active templates are due on or before today (for the reminder in the header). */
export function useDueRecurring() {
  const { data } = useLedgerQuery(api.listRecurring);
  const today = todayIso();
  return (data ?? []).filter((t) => t.isActive && t.nextDate <= today && (!t.endDate || t.nextDate <= t.endDate)).length;
}

/** Accounting › Recurring Transactions: memorized transactions and their schedules. */
export default function Recurring() {
  const { data, error, reload } = useLedgerQuery(api.listRecurring);
  const [notice, setNotice] = useState(null);
  const [asOf, setAsOf] = useState(todayIso);
  const list = data ?? [];
  const sorter = useSort(list, {
    name: (t) => t.name,
    type: (t) => KIND_LABEL[t.kind] ?? t.kind,
    payee: (t) => t.payee,
    schedule: (t) => scheduleText(t.frequency, t.every),
    next: (t) => t.nextDate,
    end: (t) => t.endDate,
    last: (t) => t.lastRunDate,
    amount: (t) => t.amountCents,
  }, { storageKey: 'recurring' });
  const due = list.filter((t) => t.isActive && t.nextDate <= asOf && (!t.endDate || t.nextDate <= t.endDate));

  const act = async (work, success) => {
    try {
      const result = await work();
      setNotice({ kind: 'success', text: typeof success === 'function' ? success(result) : success });
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  // Copies Enter all due couldn't enter (a closed period, say): listed until dismissed.
  const [skipped, setSkipped] = useState([]);
  const entered = (report) => {
    setSkipped(skippedLines(report, formatDate));
    return enteredText(report);
  };
  const [editing, setEditing] = useState(null);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Enter transactions due through</span>
          <DateBox className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
        <button type="button" className="qb-btn qb-btn-primary" disabled={due.length === 0} onClick={() => act(() => api.enterRecurring(asOf), entered)}>
          Enter all due ({due.length})
        </button>
        <span className="self-center text-xs text-qb-ink-muted">
          To add one, fill in an expense, check, bill, invoice or journal entry and choose Memorize.
        </span>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {skipped.length > 0 && (
        <div className="qb-panel flex-none border-qb-negative/40 px-2 py-1 text-sm" role="alert">
          <div className="flex items-center">
            <span className="flex-1 font-semibold text-qb-negative">Not entered (each stays due until it can be):</span>
            <button type="button" className="px-1 font-semibold opacity-70 hover:opacity-100" aria-label="Dismiss" onClick={() => setSkipped([])}>×</button>
          </div>
          <ul className="list-disc pl-5">
            {skipped.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      )}
      {editing && (
        <ScheduleDialog
          template={editing}
          onClose={() => setEditing(null)}
          onSaved={(text) => {
            setEditing(null);
            setNotice({ kind: 'success', text });
            reload();
          }}
        />
      )}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <SortTh sorter={sorter} col="name">Name</SortTh>
              <SortTh sorter={sorter} col="type">Type</SortTh>
              <SortTh sorter={sorter} col="payee">Payee / customer</SortTh>
              <SortTh sorter={sorter} col="schedule">Schedule</SortTh>
              <SortTh sorter={sorter} col="next">Next date</SortTh>
              <SortTh sorter={sorter} col="end">End</SortTh>
              <SortTh sorter={sorter} col="last">Last entered</SortTh>
              <SortTh sorter={sorter} col="amount" className="num">Amount</SortTh>
              <th className="qb-grid-head w-[36px]" />
            </tr>
          </thead>
          <tbody>
            {data && list.length === 0 && (
              <tr>
                <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">No memorized transactions yet.</td>
              </tr>
            )}
            {sorter.sorted.map((t) => {
              const isDue = due.includes(t);
              return (
                <tr key={t.id} className={`qb-grid-row ${t.isActive ? '' : 'text-qb-ink-muted'}`}>
                  <td className="qb-grid-cell font-semibold">
                    {t.name}
                    {!t.isActive && <span className="ml-1 rounded-sm border border-qb-border px-1 text-2xs uppercase">Paused</span>}
                    {isDue && <span className="ml-1 rounded-sm bg-qb-green-tint px-1 text-2xs font-semibold uppercase text-qb-green-active">Due</span>}
                  </td>
                  <td className="qb-grid-cell">{KIND_LABEL[t.kind] ?? t.kind}</td>
                  <td className="qb-grid-cell">{t.payee ?? ''}</td>
                  <td className="qb-grid-cell">{scheduleText(t.frequency, t.every)}</td>
                  <td className="qb-grid-cell">{formatDate(t.nextDate)}</td>
                  <td className="qb-grid-cell">{t.endDate ? formatDate(t.endDate) : ''}</td>
                  <td className="qb-grid-cell">{t.lastRunDate ? `${formatDate(t.lastRunDate)} (${t.runCount})` : 'Never'}</td>
                  <td className="qb-grid-cell num">{formatCents(t.amountCents)}</td>
                  <td className="qb-grid-cell">
                    <RowMenu
                      label={`Actions for ${t.name}`}
                      items={[
                        { label: 'Enter now (through date above)', disabled: !isDue, onSelect: () => act(() => api.enterRecurring(asOf, t.id), entered) },
                        { label: 'Edit…', onSelect: () => setEditing(t) },
                        t.isActive
                          ? { label: 'Pause', onSelect: () => act(() => api.setRecurringActive(t.id, false), `“${t.name}” paused.`) }
                          : { label: 'Resume', onSelect: () => act(() => api.setRecurringActive(t.id, true), `“${t.name}” resumed.`) },
                        {
                          label: 'Delete',
                          danger: true,
                          confirm: { title: `Delete “${t.name}”?`, body: 'Transactions already entered from it stay posted.', confirmLabel: 'Delete' },
                          onSelect: async () => {
                            await api.deleteRecurring(t.id);
                            setNotice({ kind: 'success', text: `“${t.name}” deleted.` });
                            reload();
                          },
                        },
                      ]}
                    />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
    </div>
  );
}
