import { useCallback, useEffect, useMemo, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import DateBox from './DateBox.jsx';
import ClassSelect, { useClassTracking } from './ClassSelect.jsx';
import { CHARGE_KINDS, reconcileCharges } from '../lib/reconcileCharges.js';
import { askConfirm, askText } from '../lib/dialogs.jsx';
import { pickAttachments } from './Attachments.jsx';
import ColumnsMenu, { useColumns } from './ColumnsMenu.jsx';

/** The longest undo reason the Audit Log keeps (`reconciliation_undos.reason`, migration 0085). */
const REASON_MAX = 500;

const RECONCILABLE = ['BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY'];
const isReconcilable = (a) => !a.isSystem && RECONCILABLE.includes(a.accountType);
const SOURCE_LABEL = {
  JOURNAL: 'Journal', INVOICE: 'Invoice', RECEIVE_PAYMENT: 'Payment', SALES_RECEIPT: 'Sales Receipt',
  CREDIT_MEMO: 'Credit Memo', BILL: 'Bill', BILL_PAYMENT: 'Bill Payment', VENDOR_CREDIT: 'Vendor Credit',
  CHECK: 'Check', EXPENSE: 'Expense', CARD_CREDIT: 'Credit Card Credit', DEPOSIT: 'Deposit', TRANSFER: 'Transfer', OPENING_BALANCE: 'Opening Bal.',
};

/**
 * The statement's ending date and balance. When starting (`accounts` given), also the service
 * charge and interest earned from the statement: each posts with the reconciliation (Dr expense /
 * Cr the account for a charge, the other way for interest) and is cleared at once.
 */
/**
 * The reconcile lines' columns in their usual order (Customize Columns hides and reorders them; the
 * amounts and the cleared tick always show). `dec` and `inc` are titled by the account's kind.
 */
const RECONCILE_COLUMNS = [
  { id: 'date', label: 'Date', width: 'w-[92px]' },
  { id: 'type', label: 'Type', width: 'w-[110px]' },
  { id: 'number', label: 'Ref no.', width: 'w-[64px]' },
  { id: 'memo', label: 'Memo', width: '' },
  { id: 'dec', label: 'Payments', width: 'w-[120px]', num: true, required: true },
  { id: 'inc', label: 'Deposits or charges', width: 'w-[120px]', num: true, required: true },
  { id: 'cleared', label: 'Cleared', width: 'w-[36px]', required: true },
];

function StatementForm({ initial, lastDate, onSubmit, busy, submitLabel, onCancel, accounts = null }) {
  const [date, setDate] = useState(initial?.date ?? todayIso());
  const [balance, setBalance] = useState(initial?.balance ?? '');
  const [rows, setRows] = useState(() => CHARGE_KINDS.map(({ kind }) => ({ kind, amount: '', date: '', accountId: null, classId: null })));
  const tracking = useClassTracking();
  const cents = parseMoneyToCents(balance);
  const dateBad = lastDate && date <= lastDate;
  // A charge's date follows the statement date until it is typed.
  const withDates = rows.map((r) => ({ ...r, date: r.date || date }));
  const charges = accounts ? reconcileCharges(withDates, date) : { charges: [] };
  const update = (i, patch) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (cents !== null && !dateBad && !charges.error) onSubmit(date, cents, charges.charges);
      }}
    >
      <label>
        <span className="qb-label">Statement ending date</span>
        <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} aria-invalid={Boolean(dateBad)} />
      </label>
      <label>
        <span className="qb-label">Ending balance</span>
        <input className="qb-input num w-[130px]" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0.00" inputMode="decimal" data-money aria-invalid={balance !== '' && cents === null} autoFocus />
      </label>
      {accounts && (
        <div className="flex w-full flex-col gap-1">
          {CHARGE_KINDS.map(({ kind, label, accountHint }, i) => (
            <div key={kind} className="flex flex-wrap items-end gap-2">
              <label>
                <span className="qb-label">{label}</span>
                <input
                  className="qb-input num w-[110px]"
                  value={rows[i].amount}
                  onChange={(e) => update(i, { amount: e.target.value })}
                  placeholder="0.00"
                  inputMode="decimal"
                  data-money
                  aria-describedby={charges.error ? 'reconcile-charge-error' : undefined}
                />
              </label>
              <label>
                <span className="qb-label">Date</span>
                <DateBox className="qb-input w-[132px]" value={withDates[i].date} onChange={(e) => update(i, { date: e.target.value })} />
              </label>
              <label>
                <span className="qb-label">{accountHint}</span>
                <AccountSelect
                  accounts={accounts}
                  filter={(a) => !a.isSystem && (kind === 'SERVICE_CHARGE' ? a.classification === 'EXPENSE' : a.classification === 'INCOME')}
                  value={rows[i].accountId}
                  onChange={(accountId) => update(i, { accountId })}
                  placeholder="Choose…"
                  className="qb-input w-[220px]"
                />
              </label>
              {tracking.enabled && (
                <label>
                  <span className="qb-label">Class</span>
                  <ClassSelect classes={tracking.classes} value={rows[i].classId} onChange={(classId) => update(i, { classId })} className="qb-input w-[150px]" />
                </label>
              )}
            </div>
          ))}
          {charges.error && (
            <span id="reconcile-charge-error" role="alert" className="text-sm text-qb-negative">
              {charges.error}
            </span>
          )}
        </div>
      )}
      <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || cents === null || Boolean(dateBad) || Boolean(charges.error)}>
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

/**
 * The bank or card statement kept with a reconciliation, in progress or finished: each file with
 * Save a copy and ×, and Attach statement… to add one (a PDF or a picture, up to 25 MB). The files
 * are attachments of kind RECONCILIATION, so they are in the Attachments Library too, and × deletes
 * reversibly (the library can restore them).
 */
function StatementFiles({ reconciliationId, onNotice, compact = false }) {
  const [files, setFiles] = useState([]);
  const load = useCallback(() => {
    api.listRecordAttachments('RECONCILIATION', reconciliationId).then(setFiles, (err) => onNotice({ kind: 'error', text: errorText(err) }));
  }, [reconciliationId, onNotice]);
  useEffect(() => load(), [load]);
  const attach = async () => {
    try {
      for (const path of await pickAttachments()) await api.addRecordAttachment('RECONCILIATION', reconciliationId, path);
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
    load();
  };
  const run = async (action) => {
    try {
      await action();
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
    load();
  };
  return (
    <span className="flex flex-wrap items-center gap-1 text-sm">
      {!compact && <span className="text-qb-ink-muted">Statement:</span>}
      {files.map((f) => (
        <span key={f.id} className="flex items-center gap-0.5 rounded-sm border border-qb-border bg-white px-1">
          <button type="button" className="text-qb-link hover:underline" title="Save a copy" onClick={() => run(() => api.saveLibraryAttachment({ source: 'RECORD', id: f.id }))}>
            📎 {f.fileName}
          </button>
          <button
            type="button"
            className="px-0.5 text-qb-ink-faint hover:text-qb-negative"
            aria-label={`Delete ${f.fileName}`}
            title="Delete (Accounting › Attachments Library can restore it)"
            onClick={async () => {
              if (await askConfirm(`Delete ${f.fileName} from this reconciliation? You can restore it from Accounting › Attachments Library.`, { confirmLabel: 'Delete', danger: true })) {
                run(() => api.deleteLibraryAttachment({ source: 'RECORD', id: f.id }));
              }
            }}
          >
            ×
          </button>
        </span>
      ))}
      <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={attach}>
        {files.length ? 'Attach another…' : 'Attach statement…'}
      </button>
    </span>
  );
}

function HeaderFigure({ label, cents, sub, tone }) {
  const toneClass = tone === 'good' ? 'text-qb-green-active' : tone === 'bad' ? 'text-qb-negative' : '';
  return (
    <div className="flex-1 px-3 py-1">
      <div className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{label}</div>
      <div className={`num text-2xl font-semibold ${toneClass}`}>{formatCents(cents)}</div>
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
  const filtered = useMemo(
    () => eligible.filter((l) => (filter === 'inc' ? l.amountCents > 0 : filter === 'dec' ? l.amountCents < 0 : true)),
    // `eligible` is rebuilt every render; what it depends on is listed instead.
    [lines, rec?.statementEndDate, filter],
  );
  // Click a heading to sort: by date, number, amount or cleared. Ticking a line doesn't re-sort the
  // list under the pointer; the cleared order is taken when its heading is clicked.
  const sorter = useSort(
    filtered,
    {
      date: (l) => l.entryDate,
      type: (l) => SOURCE_LABEL[l.sourceType] ?? l.sourceType,
      number: (l) => l.entryNumber,
      memo: (l) => l.memo,
      dec: (l) => (l.amountCents < 0 ? -l.amountCents : null),
      inc: (l) => (l.amountCents > 0 ? l.amountCents : null),
      cleared: (l) => cleared.has(l.lineId),
    },
    { initial: null, storageKey: 'reconcile' },
  );
  const shown = sorter.sorted;
  const cols = useColumns('reconcile', RECONCILE_COLUMNS);

  const clearedLines = eligible.filter((l) => cleared.has(l.lineId));
  const incSum = clearedLines.filter((l) => l.amountCents > 0).reduce((s, l) => s + l.amountCents, 0);
  const decSum = clearedLines.filter((l) => l.amountCents < 0).reduce((s, l) => s - l.amountCents, 0);
  const incCount = clearedLines.filter((l) => l.amountCents > 0).length;
  const decCount = clearedLines.length - incCount;
  const clearedBalance = rec ? rec.beginningBalanceCents + incSum - decSum : 0;
  const difference = rec ? rec.statementEndingBalanceCents - clearedBalance : 0;

  const start = async (date, cents, charges = []) => {
    setBusy(true);
    try {
      await api.startReconciliation(accountId, date, cents, charges);
      if (charges.length > 0) {
        const what = CHARGE_KINDS.filter((k) => charges.some((c) => c.kind === k.kind)).map((k) => k.label.toLowerCase());
        setNotice({ kind: 'success', text: `The ${what.join(' and ')} ${what.length === 1 ? 'is' : 'are'} posted and already cleared below.` });
      }
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

  // Undo Last Reconciliation (docs/plans/undo-reconciliation.md): the reconciliation stays on
  // record, an audited undo releases its transactions to be cleared again.
  const undoLast = async (h) => {
    const ok = await askConfirm(
      `Undo the reconciliation of ${account?.fullName ?? 'this account'} through ${formatDate(h.statementEndDate)}? Its transactions become uncleared, so you can reconcile that statement again. The undo is recorded in the Audit Log and can't itself be undone. Service charges and interest posted with it stay posted.`,
      { confirmLabel: 'Undo reconciliation', danger: true },
    );
    if (!ok) return;
    const reason = await askText('Why are you undoing it? (optional, up to 500 characters, kept in the Audit Log)');
    if (reason === null) return;
    if (reason.trim().length > REASON_MAX) {
      setNotice({ kind: 'error', text: `Keep the reason to ${REASON_MAX} characters (it has ${reason.trim().length}); nothing was undone.` });
      return;
    }
    setBusy(true);
    try {
      const released = await api.undoLastReconciliation(h.id, reason.trim() || null);
      setNotice({ kind: 'success', text: `Reconciliation through ${formatDate(h.statementEndDate)} undone: ${released} transaction${released === 1 ? ' is' : 's are'} uncleared again.` });
      await load(accountId);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
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
            <StatementForm lastDate={state.lastStatementEndDate} onSubmit={start} busy={busy} submitLabel="Start reconciling" accounts={accounts} />
          </div>
        )}
        {rec && !editing && (
          <span className="ml-auto flex items-center gap-2">
            <StatementFiles reconciliationId={rec.id} onNotice={setNotice} />
            <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setEditing(true)}>
              Edit statement info
            </button>
          </span>
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
              <div className={`num text-2xl font-semibold ${difference === 0 ? 'text-qb-green-active' : 'text-qb-negative'}`}>{formatCents(difference)}</div>
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
                <ColumnsMenu cols={cols} />
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
                  {cols.columns.map((c) => (
                    <col key={c.id} className={c.width} />
                  ))}
                </colgroup>
                <thead>
                  <tr>
                    {cols.columns.map((c) =>
                      c.id === 'cleared' ? (
                        <th key={c.id} className="qb-grid-head text-center">
                          <button
                            type="button"
                            className="mr-0.5 align-middle text-[9px] text-qb-ink-muted hover:text-qb-ink"
                            onClick={() => sorter.toggle('cleared')}
                            title="Sort by cleared"
                            aria-label="Sort by cleared"
                          >
                            {sorter.sort?.key === 'cleared' ? (sorter.sort.dir === 'desc' ? '▼' : '▲') : '⇅'}
                          </button>
                          <input
                            type="checkbox"
                            className="accent-qb-green"
                            checked={allShownCleared}
                            onChange={(e) => toggle(shown.filter((l) => cleared.has(l.lineId) !== e.target.checked).map((l) => l.lineId), e.target.checked)}
                            aria-label="Clear all shown"
                          />
                        </th>
                      ) : (
                        <SortTh key={c.id} sorter={sorter} col={c.id} className={c.num ? 'text-right' : undefined}>
                          {c.id === 'dec' ? labels.dec : c.id === 'inc' ? labels.inc : c.label}
                        </SortTh>
                      ),
                    )}
                  </tr>
                </thead>
                <tbody>
                  {shown.length === 0 && (
                    <tr>
                      <td colSpan={cols.columns.length} className="qb-grid-cell text-qb-ink-muted">No unreconciled transactions through {formatDate(rec.statementEndDate)}.</td>
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
                        {cols.columns.map((c) => {
                          switch (c.id) {
                            case 'date':
                              return <td key={c.id} className="qb-grid-cell">{formatDate(l.entryDate)}</td>;
                            case 'type':
                              return <td key={c.id} className="qb-grid-cell">{SOURCE_LABEL[l.sourceType] ?? l.sourceType}</td>;
                            case 'number':
                              return <td key={c.id} className="qb-grid-cell text-qb-ink-muted">{l.entryNumber}</td>;
                            case 'memo':
                              return (
                                <td key={c.id} className="qb-grid-cell" title={l.memo ?? ''}>
                                  {l.memo ?? ''}
                                </td>
                              );
                            case 'dec':
                              return <td key={c.id} className="qb-grid-cell num">{l.amountCents < 0 ? formatCents(-l.amountCents) : ''}</td>;
                            case 'inc':
                              return <td key={c.id} className="qb-grid-cell num">{l.amountCents > 0 ? formatCents(l.amountCents) : ''}</td>;
                            default:
                              return (
                                <td key={c.id} className="qb-grid-cell text-center">
                                  <input
                                    type="checkbox"
                                    className="accent-qb-green"
                                    checked={isCleared}
                                    onClick={(e) => e.stopPropagation()}
                                    onChange={(e) => toggle([l.lineId], e.target.checked)}
                                    aria-label={`Cleared ${formatDate(l.entryDate)} ${l.memo ?? ''}`}
                                  />
                                </td>
                              );
                          }
                        })}
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
                  <th className="qb-grid-head">Statement</th>
                  <th className="qb-grid-head">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {history.length === 0 && (
                  <tr>
                    <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">No completed reconciliations yet. Enter your statement details above to start.</td>
                  </tr>
                )}
                {history.map((h, i) => (
                  <tr key={h.id} className="qb-grid-row">
                    <td className="qb-grid-cell">{formatDate(h.statementEndDate)}</td>
                    <td className="qb-grid-cell num">{formatCents(h.beginningBalanceCents)}</td>
                    <td className="qb-grid-cell num">{formatCents(h.statementEndingBalanceCents)}</td>
                    <td className="qb-grid-cell text-qb-ink-muted">🔒 {formatDate(h.lockedAt?.slice(0, 10))}</td>
                    <td className="qb-grid-cell">
                      <StatementFiles reconciliationId={h.id} onNotice={setNotice} compact />
                    </td>
                    <td className="qb-grid-cell cell-tight text-right">
                      {/* Newest first: only the last reconciliation can be undone. */}
                      {i === 0 && (
                        <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => undoLast(h)} disabled={busy}>
                          Undo Last Reconciliation…
                        </button>
                      )}
                    </td>
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
