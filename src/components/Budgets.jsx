import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { fiscalMonthLabel } from './MoreReports.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const PL_TYPES = new Set(['INCOME', 'COST_OF_GOODS_SOLD', 'EXPENSE', 'OTHER_INCOME', 'OTHER_EXPENSE']);
const MONTHS = Array.from({ length: 12 }, (_, i) => i + 1);
const key = (accountId, period) => `${accountId}:${period}`;

/**
 * Accounting › Budgets: a budget per fiscal year, entered month by month per account. Income and
 * expense accounts take each month's amount; balance sheet accounts take the balance expected at
 * each month end.
 */
export default function Budgets({ accounts, navigate }) {
  const budgets = useLedgerQuery(api.listBudgets);
  const [budgetId, setBudgetId] = useState(null);
  const [cells, setCells] = useState({}); // key -> text as typed
  const [saved, setSaved] = useState({}); // key -> cents as stored
  const [notice, setNotice] = useState(null);
  const [creating, setCreating] = useState(null); // { name, year }
  const [side, setSide] = useState('pl'); // pl | bs
  const list = budgets.data ?? [];
  const budget = list.find((b) => b.id === budgetId) ?? list[0] ?? null;
  const rows = useMemo(() => accounts.filter((a) => PL_TYPES.has(a.accountType) === (side === 'pl') && a.isActive), [accounts, side]);

  useEffect(() => {
    if (!budget) return;
    let cancelled = false;
    api
      .getBudgetAmounts(budget.id)
      .then((amounts) => {
        if (cancelled) return;
        const stored = Object.fromEntries(amounts.map((a) => [key(a.accountId, a.period), a.amountCents]));
        setSaved(stored);
        setCells(Object.fromEntries(Object.entries(stored).map(([k, c]) => [k, centsToInput(c)])));
      })
      .catch((err) => setNotice({ kind: 'error', text: errorText(err) }));
    return () => {
      cancelled = true;
    };
  }, [budget?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const cents = (k) => {
    const text = cells[k];
    if (text === undefined || text.trim() === '') return 0;
    return parseMoneyToCents(text);
  };
  const invalid = Object.keys(cells).filter((k) => cents(k) === null);
  const changed = Object.keys({ ...cells, ...saved }).filter((k) => cents(k) !== null && cents(k) !== (saved[k] ?? 0));
  const rowTotal = (id) => MONTHS.reduce((s, m) => s + (cents(key(id, m)) ?? 0), 0);

  const save = async () => {
    try {
      const amounts = changed.map((k) => {
        const [accountId, period] = k.split(':').map(Number);
        return { accountId, period, amountCents: cents(k) };
      });
      await api.setBudgetAmounts(budget.id, amounts);
      setSaved(Object.fromEntries(Object.keys(cells).filter((k) => cents(k)).map((k) => [k, cents(k)])));
      setNotice({ kind: 'success', text: `Saved ${amounts.length} budget amount${amounts.length === 1 ? '' : 's'}.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const fillRow = (id) => {
    const first = cells[key(id, 1)] ?? '';
    setCells((c) => ({ ...c, ...Object.fromEntries(MONTHS.map((m) => [key(id, m), first])) }));
  };
  const create = async () => {
    try {
      const made = await api.createBudget(creating.name, `${creating.year}-06-30`);
      setCreating(null);
      await budgets.reload();
      setBudgetId(made.id);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const remove = async () => {
    if (!window.confirm(`Delete the budget “${budget.name}”? Its amounts are removed; transactions are not affected.`)) return;
    try {
      await api.deleteBudget(budget.id);
      setBudgetId(null);
      budgets.reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const thisYear = Number(todayIso().slice(0, 4));
  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {list.length > 0 && (
          <label>
            <span className="qb-label">Budget</span>
            <select className="qb-input w-[240px]" value={budget?.id ?? ''} onChange={(e) => setBudgetId(Number(e.target.value))} disabled={changed.length > 0}>
              {list.map((b) => (
                <option key={b.id} value={b.id}>{b.name} (FY from {formatDate(b.fiscalYearStart)})</option>
              ))}
            </select>
          </label>
        )}
        {budget && !creating && (
          <label>
            <span className="qb-label">Accounts</span>
            <select className="qb-input w-[170px]" value={side} onChange={(e) => setSide(e.target.value)}>
              <option value="pl">Profit and Loss</option>
              <option value="bs">Balance Sheet</option>
            </select>
          </label>
        )}
        {creating ? (
          <>
            <label>
              <span className="qb-label">Name</span>
              <input className="qb-input w-[200px]" value={creating.name} onChange={(e) => setCreating({ ...creating, name: e.target.value })} autoFocus />
            </label>
            <label>
              <span className="qb-label">Fiscal year containing</span>
              <select className="qb-input w-[90px]" value={creating.year} onChange={(e) => setCreating({ ...creating, year: Number(e.target.value) })}>
                {[thisYear - 1, thisYear, thisYear + 1, thisYear + 2].map((y) => (
                  <option key={y} value={y}>{y}</option>
                ))}
              </select>
            </label>
            <button type="button" className="qb-btn qb-btn-primary" onClick={create} disabled={!creating.name.trim()}>Create budget</button>
            <button type="button" className="qb-btn" onClick={() => setCreating(null)}>Cancel</button>
          </>
        ) : (
          <button type="button" className="qb-btn" onClick={() => setCreating({ name: `FY${thisYear} Budget`, year: thisYear })}>New budget…</button>
        )}
        {budget && !creating && (
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn" onClick={() => navigate?.('reports', 'open', { report: side === 'pl' ? 'bva' : 'bs-bva' })}>Budget vs. Actual</button>
            <button type="button" className="qb-btn" onClick={remove}>Delete</button>
            <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={changed.length === 0 || invalid.length > 0}>
              Save{changed.length ? ` (${changed.length})` : ''}
            </button>
          </div>
        )}
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {budgets.error && <Notice kind="error" text={budgets.error} />}
      {invalid.length > 0 && <Notice kind="error" text="Some amounts are not valid numbers; they are outlined in red." />}
      {budgets.data && list.length === 0 && !creating && (
        <Notice kind="info" text="Budgets set expected income and expenses (and, if you like, month-end balances) per account for each month of a fiscal year. Choose New budget to start one, then compare it with your books in Reports › Profit and Loss Budget vs. Actual or Balance Sheet Budget vs. Actual." />
      )}
      {budget && side === 'bs' && (
        <Notice kind="info" text="For balance sheet accounts, enter the balance you expect at the end of each month (not the change). The Balance Sheet Budget vs. Actual report compares them with the actual balances." />
      )}
      {budget && (
        <div className="qb-panel min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head sticky left-0 z-10 min-w-[220px] bg-qb-canvas">Account</th>
                {MONTHS.map((m) => (
                  <th key={m} className="qb-grid-head num min-w-[92px]">{fiscalMonthLabel(budget.fiscalYearStart, m)}</th>
                ))}
                <th className="qb-grid-head num min-w-[100px]">{side === 'pl' ? 'Total' : ''}</th>
                <th className="qb-grid-head" />
              </tr>
            </thead>
            <tbody>
              {rows.map((a) => (
                <tr key={a.id} className="qb-grid-row">
                  <td className="qb-grid-cell sticky left-0 bg-white" style={{ paddingLeft: 6 + a.depth * 12 }} title={a.accountTypeName}>
                    {a.accountNumber ? `${a.accountNumber} · ` : ''}
                    {a.name}
                  </td>
                  {MONTHS.map((m) => {
                    const k = key(a.id, m);
                    return (
                      <td key={m} className="qb-grid-cell p-0">
                        <input
                          className="qb-input num h-full w-full rounded-none border-transparent text-right"
                          value={cells[k] ?? ''}
                          aria-label={`${a.name}, ${fiscalMonthLabel(budget.fiscalYearStart, m)}`}
                          aria-invalid={cents(k) === null}
                          onChange={(e) => setCells((c) => ({ ...c, [k]: e.target.value }))}
                        />
                      </td>
                    );
                  })}
                  <td className="qb-grid-cell num font-semibold">{side === 'pl' ? formatCents(rowTotal(a.id)) : ''}</td>
                  <td className="qb-grid-cell">
                    <button type="button" className="text-xs text-qb-link hover:underline" onClick={() => fillRow(a.id)} title="Copy the first month across the year">
                      Copy across
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
