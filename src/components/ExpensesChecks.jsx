import { useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { api, errorText, PAYMENT_METHODS, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

// Any account a cash outlay can be categorized to: not the payment side, not A/R or A/P.
const isSplitAccount = (a) => !['ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE'].includes(a.accountType) && !(a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');

function ExpenseForm({ kind, setKind, accounts, vendors, expenses, onPosted, onCancel }) {
  const payFrom = kind === 'CHECK' ? (a) => a.accountType === 'BANK' : (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
  const defaultPayFrom = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'EXPENSE' && !a.isSystem)?.id ?? null, [accounts]);
  const nextCheck = useMemo(() => {
    const numbers = expenses.filter((e) => e.kind === 'CHECK' && /^\d+$/.test(e.checkNumber ?? '')).map((e) => Number(e.checkNumber));
    return numbers.length ? String(Math.max(...numbers) + 1) : '1001';
  }, [expenses]);

  const [payee, setPayee] = useState('');
  const [accountId, setAccountId] = useState(defaultPayFrom);
  const [date, setDate] = useState(todayIso());
  const [checkNumber, setCheckNumber] = useState(nextCheck);
  const [method, setMethod] = useState('CREDIT_CARD');
  const [memo, setMemo] = useState('');
  const [rows, setRows] = useState(() => [blankSplit(defaultCategory), blankSplit(defaultCategory)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const payeeRef = useRef(null);

  useEffect(() => payeeRef.current?.focus(), [kind]);
  useEffect(() => setCheckNumber(nextCheck), [nextCheck]);
  useEffect(() => {
    if (accountId === null && defaultPayFrom !== null) setAccountId(defaultPayFrom);
  }, [defaultPayFrom, accountId]);
  useEffect(() => {
    if (defaultCategory === null) return;
    setRows((rs) => rs.map((r) => (r.accountId === null ? { ...r, accountId: defaultCategory } : r)));
  }, [defaultCategory]);
  // A check must come from a bank account; drop a credit card selection when switching.
  useEffect(() => {
    const current = accounts.find((a) => a.id === accountId);
    if (kind === 'CHECK' && current && current.accountType !== 'BANK') setAccountId(defaultPayFrom);
  }, [kind, accountId, accounts, defaultPayFrom]);

  const { lines, invalid, total } = summarizeSplits(rows);

  const save = async () => {
    if (accountId === null) return setError(kind === 'CHECK' ? 'Choose the bank account the check is drawn on.' : 'Choose the payment account.');
    if (invalid > 0) return setError(`${invalid} split row(s) need a category and an amount above zero.`);
    if (lines.length === 0) return setError('Add at least one split row with an amount.');
    if (lines.some((l) => l.accountId === accountId)) return setError('A split row cannot use the payment account itself.');
    setBusy(true);
    try {
      const posted = await api.postExpense({
        kind,
        payeeName: payee.trim() || null,
        paymentAccountId: accountId,
        expenseDate: date,
        checkNumber: kind === 'CHECK' ? checkNumber.trim() || null : null,
        paymentMethod: kind === 'EXPENSE' ? method : null,
        memo: memo.trim() || null,
        lines,
      });
      onPosted(
        `${kind === 'CHECK' ? `Check${posted.checkNumber ? ` #${posted.checkNumber}` : ''}` : 'Expense'} of ${formatCents(posted.totalCents)} posted: ` +
          `Cr ${accountLabel(accounts, accountId)} / Dr ${lines.length} split${lines.length === 1 ? '' : 's'}.`,
      );
      setPayee('');
      setMemo('');
      setRows([blankSplit(defaultCategory), blankSplit(defaultCategory)]);
      setError(null);
      payeeRef.current?.focus();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <div className="mr-1 flex self-center overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Type">
          {[
            ['EXPENSE', 'Expense'],
            ['CHECK', 'Check'],
          ].map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={kind === id}
              onClick={() => setKind(id)}
              className={`px-2.5 py-0.5 font-semibold ${kind === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
            >
              {label}
            </button>
          ))}
        </div>
        <label>
          <span className="qb-label">Payee</span>
          <input ref={payeeRef} className="qb-input w-[200px]" value={payee} onChange={(e) => setPayee(e.target.value)} list="expense-payees" placeholder="Who did you pay?" />
          <datalist id="expense-payees">
            {vendors.map((v) => (
              <option key={v.id} value={v.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">{kind === 'CHECK' ? 'Bank account' : 'Payment account'}</span>
          <AccountSelect accounts={accounts} filter={payFrom} value={accountId} onChange={setAccountId} className="qb-input w-[200px]" />
        </label>
        <label>
          <span className="qb-label">{kind === 'CHECK' ? 'Check date' : 'Payment date'}</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {kind === 'CHECK' ? (
          <label>
            <span className="qb-label">Check no.</span>
            <input className="qb-input num w-[80px]" value={checkNumber} onChange={(e) => setCheckNumber(e.target.value)} />
          </label>
        ) : (
          <label>
            <span className="qb-label">Payment method</span>
            <select className="qb-input w-[140px]" value={method} onChange={(e) => setMethod(e.target.value)}>
              {PAYMENT_METHODS.map((m) => (
                <option key={m.id} value={m.id}>{m.label}</option>
              ))}
            </select>
          </label>
        )}
        <div className="ml-auto text-right">
          <div className="qb-label">Total</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={(a) => isSplitAccount(a) && a.id !== accountId} defaultAccountId={defaultCategory} />
      <div className="flex items-center gap-2 px-2 py-1">
        <input className="qb-input w-[320px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo" />
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : `Save & Post ${kind === 'CHECK' ? 'Check' : 'Expense'}`}
          </button>
        </div>
      </div>
    </section>
  );
}

export default function ExpensesChecks({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listExpenses);
  const vendors = useLedgerQuery(api.listVendors);
  const initialKind = intent?.action === 'check' ? 'CHECK' : intent?.action === 'expense' ? 'EXPENSE' : null;
  const [kind, setKind] = useState(initialKind);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (intent?.action === 'check') setKind('CHECK');
    if (intent?.action === 'expense') setKind('EXPENSE');
  }, [intent]);

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };
  const rows = data ?? [];

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {kind && (
        <ExpenseForm
          kind={kind}
          setKind={setKind}
          accounts={accounts}
          vendors={vendors.data ?? []}
          expenses={rows}
          onCancel={() => setKind(null)}
          onPosted={(text) => setNotice({ kind: 'success', text })}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load expenses: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Expenses & checks</h2>
          <span className="text-xs text-qb-ink-muted">Bank feed approvals appear here as expenses and deposits</span>
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setKind('CHECK')}>
              Write check
            </button>
            <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setKind('EXPENSE')}>
              New expense
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[84px]" />
              <col className="w-[70px]" />
              <col className="w-[180px]" />
              <col className="w-[200px]" />
              <col />
              <col className="w-[112px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Payee</th>
                <th className="qb-grid-head">Category</th>
                <th className="qb-grid-head">Paid from / memo</th>
                <th className="qb-grid-head text-right">Total</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No expenses or checks yet.</td>
                </tr>
              )}
              {rows.map((x) => (
                <tr key={x.id} className={`qb-grid-row ${x.isVoided ? 'text-qb-ink-faint' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(x.expenseDate)}</td>
                  <td className="qb-grid-cell">{x.kind === 'CHECK' ? 'Check' : 'Expense'}</td>
                  <td className="qb-grid-cell">{x.checkNumber ?? ''}</td>
                  <td className="qb-grid-cell font-semibold" title={x.payeeName ?? ''}>{x.payeeName ?? <span className="font-normal text-qb-ink-faint">—</span>}</td>
                  <td className="qb-grid-cell">
                    {x.categoryAccountId !== null ? accountLabel(accounts, x.categoryAccountId) : <span className="text-qb-ink-muted">Split ({x.lineCount} lines)</span>}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={x.memo ?? ''}>
                    {accountLabel(accounts, x.paymentAccountId)}
                    {x.memo ? ` · ${x.memo}` : ''}
                  </td>
                  <td className={`qb-grid-cell num ${x.isVoided ? 'line-through' : ''}`}>{formatCents(x.totalCents)}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      items={[
                        voidItem({
                          journalEntryId: x.journalEntryId,
                          voided: x.isVoided,
                          description: `${x.kind === 'CHECK' ? `Check${x.checkNumber ? ` #${x.checkNumber}` : ''}` : 'Expense'} of ${formatCents(x.totalCents)}${x.payeeName ? ` to ${x.payeeName}` : ''}. If it came from the bank feed, the bank line returns to "For review".`,
                          onVoid: voidEntry,
                        }),
                      ]}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
