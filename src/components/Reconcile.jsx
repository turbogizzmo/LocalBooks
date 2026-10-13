import { useCallback, useEffect, useMemo, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const RECONCILABLE = ['BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY'];
const isReconcilable = (a) => !a.isSystem && RECONCILABLE.includes(a.accountType);
const SOURCE_LABEL = {
  JOURNAL: 'Journal', INVOICE: 'Invoice', RECEIVE_PAYMENT: 'Payment', SALES_RECEIPT: 'Sales Receipt',
  CREDIT_MEMO: 'Credit Memo', BILL: 'Bill', BILL_PAYMENT: 'Bill Payment', VENDOR_CREDIT: 'Vendor Credit',
  CHECK: 'Check', EXPENSE: 'Expense', DEPOSIT: 'Deposit', TRANSFER: 'Transfer', OPENING_BALANCE: 'Opening Bal.',
};

function StatementForm({ initial, lastDate, onSubmit, busy, submitLabel, onCancel }) {
  const [date, setDate] = useState(initial?.date ?? todayIso());
  const [balance, setBalance] = useState(initial?.balance ?? '');
  const cents = parseMoneyToCents(balance);
  const dateBad = lastDate && date <= lastDate;
  return (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (cents !== null && !dateBad) onSubmit(date, cents);
      }}
    >
      <label>
        <span className="qb-label">Statement ending date</span>
        <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} aria-invalid={Boolean(dateBad)} />
      </label>
      <label>
        <span className="qb-label">Ending balance</span>
        <input className="qb-input num w-[130px]" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0.00" inputMode="decimal" aria-invalid={balance !== '' && cents === null} autoFocus />
      </label>
      <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || cents === null || Boolean(dateBad)}>
        {submitLabel}
      </button>
      {onCancel && (
        <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel}>
          Cancel
        </button>
      )}
      {dateBad && <span className="self-center text-sm text-qb-negative">Must be after {formatDate(lastDate)}, the last reconciled statement.</span>}
    </form>
  );
}

function HeaderFigure({ label, cents, sub, tone }) {
  const toneClass = tone === 'good' ? 'text-qb-green-active' : tone === 'bad' ? 'text-qb-negative' : '';
  return (
    <div className="flex-1 px-3 py-1">
      <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{label}</div>
      <div className={`num text-left text-2xl font-semibold ${toneClass}`}>{formatCents(cents)}</div>
      {sub && <div className="text-xs text-qb-ink-muted">{sub}</div>}
    </div>
  );
}

export default function Reconcile({ accounts }) {
  const options = useMemo(() => accounts.filter((a) => a.isActive && isReconcilable(a)), [accounts]);
  const [accountId, setAccountId] = useState(null);
  const [state, setState] = useState(null);
  const [lines, setLines] = useState([]);
  const [history, setHistory] = useState([]);
  const [cleared, setCleared] = useState(() => new Set());
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [filter, setFilter] = useState('all');
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (accountId === null && options.length > 0) setAccountId(options[0].id);
  }, [accountId, options]);

  const load = useCallback(async (id) => {
    if (id === null) return;
    try {
      const [s, l, h] = await Promise.all([api.getReconcileState(id), api.getUnreconciledLines(id), api.listReconciliations(id)]);
      setState(s);
      setLines(l);
      setHistory(h);
      setCleared(new Set(l.filter((x) => s.inProgress && x.reconciliationId === s.inProgress.id).map((x) => x.lineId)));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  }, []);

  useEffect(() => {
    setState(null);
    setEditing(false);
    load(accountId);
  }, [accountId, load]);

  const rec = state?.inProgress ?? null;
  const account = accounts.find((a) => a.id === accountId);
  const creditNormal = state?.normalBalance === 'CREDIT';
  const labels = creditNormal ? { inc: 'Charge', dec: 'Payment', incs: 'charges', decs: 'payments' } : { inc: 'Deposit', dec: 'Payment', incs: 'deposits', decs: 'payments' };

  const eligible = rec ? lines.filter((l) => l.entryDate <= rec.statementEndDate) : [];
  const hiddenLater = rec ? lines.length - eligible.length : 0;
  const shown = eligible.filter((l) => (filter === 'inc' ? l.amountCents > 0 : filter === 'dec' ? l.amountCents < 0 : true));

  const clearedLines = eligible.filter((l) => cleared.has(l.lineId));
  const incSum = clearedLines.filter((l) => l.amountCents > 0).reduce((s, l) => s + l.amountCents, 0);
  const decSum = clearedLines.filter((l) => l.amountCents < 0).reduce((s, l) => s - l.amountCents, 0);
  const incCount = clearedLines.filter((l) => l.amountCents > 0).length;
  const decCount = clearedLines.length - incCount;
  const clearedBalance = rec ? rec.beginningBalanceCents + incSum - decSum : 0;
  const difference = rec ? rec.statementEndingBalanceCents - clearedBalance : 0;

  const start = async (date, cents) => {
    setBusy(true);
    try {
      await api.startReconciliation(accountId, date, cents);
      setEditing(false);
      await load(accountId);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (ids, value) => {
    if (!rec || ids.length === 0) return;
    const previous = cleared;
    const next = new Set(cleared);
    for (const id of ids) {
      if (value) next.add(id);
      else next.delete(id);
    }
    setCleared(next);
    try {
      await api.setLinesCleared(rec.id, ids, value);
    } catch (err) {
      setCleared(previous);
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const finish = async () => {
    if (!rec || difference !== 0) return;
    setBusy(true);
    try {
      const done = await api.lockReconciliation(rec.id);
      setNotice({ kind: 'success', text: `${account?.fullName ?? 'Account'} reconciled through ${formatDate(done.statementEndDate)}. Cleared transactions are now locked.` });
      await load(accountId);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (!rec) return;
    setBusy(true);
    try {
      await api.cancelReconciliation(rec.id);
      setNotice({ kind: 'info', text: 'Reconciliation discarded. No transactions were changed.' });
      await load(accountId);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const allShownCleared = shown.length > 0 && shown.every((l) => cleared.has(l.lineId));

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-end gap-3 px-2 py-1">
        <label>
          <span className="qb-label">Account</span>
          <AccountSelect accounts={accounts} filter={isReconcilable} value={accountId} onChange={setAccountId} className="qb-input w-[240px]" />
        </label>
        {state && (
          <div className="self-center text-sm text-qb-ink-muted">
            {state.lastStatementEndDate ? (
              <>Last reconciled <span className="font-semibold text-qb-ink">{formatDate(state.lastStatementEndDate)}</span> · </>
            ) : (
              <>Never reconciled · </>
            )}
            Beginning balance <span className="num font-semibold text-qb-ink">{formatCents(state.reconciledBalanceCents)}</span>
          </div>
        )}
        {state && !rec && (
          <div className="ml-auto">
            <StatementForm lastDate={state.lastStatementEndDate} onSubmit={start} busy={busy} submitLabel="Start reconciling" />
          </div>
        )}
        {rec && !editing && (
          <button type="button" className="qb-btn qb-btn-secondary ml-auto" onClick={() => setEditing(true)}>
            Edit statement info
          </button>
        )}
        {rec && editing && (
          <div className="ml-auto">
            <StatementForm
              initial={{ date: rec.statementEndDate, balance: centsToInput(rec.statementEndingBalanceCents) }}
              lastDate={state.lastStatementEndDate}
              onSubmit={start}
              busy={busy}
              submitLabel="Save"
              onCancel={() => setEditing(false)}
            />
          </div>
        )}
      </div>

      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}

      {options.length === 0 && <Notice kind="info" text="Add a bank or credit card account to reconcile." />}

      {rec && (
        <>
          <div className="qb-panel flex flex-none items-stretch divide-x divide-qb-border">
            <HeaderFigure label="Statement ending balance" cents={rec.statementEndingBalanceCents} sub={`Statement ending ${formatDate(rec.statementEndDate)}`} />
            <div className="flex items-center px-2 text-2xl text-qb-ink-faint" aria-hidden="true">−</div>
            <HeaderFigure
              label="Cleared balance"
              cents={clearedBalance}
              sub={`${formatCents(rec.beginningBalanceCents)} beginning · ${incCount} ${labels.incs} ${formatCents(incSum)} · ${decCount} ${labels.decs} ${formatCents(decSum)}`}
            />
            <div className="flex items-center px-2 text-2xl text-qb-ink-faint" aria-hidden="true">=</div>
            <div className={`flex-1 px-3 py-1 ${difference === 0 ? 'bg-qb-green-tint' : ''}`}>
              <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Difference</div>
              <div className={`num text-left text-2xl font-semibold ${difference === 0 ? 'text-qb-green-active' : 'text-qb-negative'}`}>{formatCents(difference)}</div>
              <div className={`text-xs ${difference === 0 ? 'font-semibold text-qb-green-active' : 'text-qb-ink-muted'}`}>
                {difference === 0 ? '✓ Balanced. You can finish now.' : 'Tick cleared transactions until this is $0.00'}
              </div>
            </div>
          </div>

          <section className="qb-panel flex min-h-0 flex-1 flex-col">
            <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
              <div className="flex overflow-hidden rounded-xs border border-qb-border-strong text-xs" role="group" aria-label="Show">
                {[
                  ['all', 'All'],
                  ['dec', `${labels.dec}s`],
                  ['inc', `${labels.inc}s`],
                ].map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    aria-pressed={filter === id}
                    onClick={() => setFilter(id)}
                    className={`px-2 py-0.5 ${filter === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <span className="text-xs text-qb-ink-muted">
                {clearedLines.length} of {eligible.length} cleared
                {hiddenLater > 0 && ` · ${hiddenLater} dated after the statement hidden`}
              </span>
              <div className="ml-auto flex gap-2">
                <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={cancel} disabled={busy}>
                  Cancel reconciliation
                </button>
                <button type="button" className="qb-btn qb-btn-primary h-row" onClick={finish} disabled={busy || difference !== 0}>
                  Finish now
                </button>
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
              <table className="qb-grid">
                <colgroup>
                  <col className="w-[92px]" />
                  <col className="w-[110px]" />
                  <col className="w-[64px]" />
                  <col />
                  <col className="w-[120px]" />
                  <col className="w-[120px]" />
                  <col className="w-[36px]" />
                </colgroup>
                <thead>
                  <tr>
                    <th className="qb-grid-head">Date</th>
                    <th className="qb-grid-head">Type</th>
                    <th className="qb-grid-head">Ref no.</th>
                    <th className="qb-grid-head">Memo</th>
                    <th className="qb-grid-head text-right">{labels.dec}</th>
                    <th className="qb-grid-head text-right">{labels.inc}</th>
                    <th className="qb-grid-head text-center">
                      <input
                        type="checkbox"
                        className="accent-qb-green"
                        checked={allShownCleared}
                        onChange={(e) => toggle(shown.filter((l) => cleared.has(l.lineId) !== e.target.checked).map((l) => l.lineId), e.target.checked)}
                        aria-label="Clear all shown"
                      />
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {shown.length === 0 && (
                    <tr>
                      <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No unreconciled transactions through {formatDate(rec.statementEndDate)}.</td>
                    </tr>
                  )}
                  {shown.map((l) => {
                    const isCleared = cleared.has(l.lineId);
                    return (
                      <tr
                        key={l.lineId}
                        className={`qb-grid-row cursor-pointer ${isCleared ? 'bg-qb-green-tint/60' : ''}`}
                        onClick={() => toggle([l.lineId], !isCleared)}
                      >
                        <td className="qb-grid-cell">{formatDate(l.entryDate)}</td>
                        <td className="qb-grid-cell">{SOURCE_LABEL[l.sourceType] ?? l.sourceType}</td>
                        <td className="qb-grid-cell text-qb-ink-muted">{l.entryNumber}</td>
                        <td className="qb-grid-cell" title={l.memo ?? ''}>{l.memo ?? ''}</td>
                        <td className="qb-grid-cell num">{l.amountCents < 0 ? formatCents(-l.amountCents) : ''}</td>
                        <td className="qb-grid-cell num">{l.amountCents > 0 ? formatCents(l.amountCents) : ''}</td>
                        <td className="qb-grid-cell text-center">
                          <input
                            type="checkbox"
                            className="accent-qb-green"
                            checked={isCleared}
                            onClick={(e) => e.stopPropagation()}
                            onChange={(e) => toggle([l.lineId], e.target.checked)}
                            aria-label={`Cleared ${formatDate(l.entryDate)} ${l.memo ?? ''}`}
                          />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>
        </>
      )}

      {state && !rec && (
        <section className="qb-panel flex min-h-0 flex-1 flex-col">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">Reconciliation history</h2>
            <span className="ml-2 text-xs text-qb-ink-muted">{lines.length} unreconciled transactions in this account</span>
          </div>
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head">Statement ending</th>
                  <th className="qb-grid-head text-right">Beginning balance</th>
                  <th className="qb-grid-head text-right">Ending balance</th>
                  <th className="qb-grid-head">Completed</th>
                </tr>
              </thead>
              <tbody>
                {history.length === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">No completed reconciliations yet. Enter your statement details above to start.</td>
                  </tr>
                )}
                {history.map((h) => (
                  <tr key={h.id} className="qb-grid-row">
                    <td className="qb-grid-cell">{formatDate(h.statementEndDate)}</td>
                    <td className="qb-grid-cell num">{formatCents(h.beginningBalanceCents)}</td>
                    <td className="qb-grid-cell num">{formatCents(h.statementEndingBalanceCents)}</td>
                    <td className="qb-grid-cell text-qb-ink-muted">🔒 {formatDate(h.lockedAt?.slice(0, 10))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </div>
  );
}
