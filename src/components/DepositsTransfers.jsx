import { useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { api, errorText, PAYMENT_METHODS, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const isBank = (a) => a.accountType === 'BANK';
const TRANSFERABLE = ['BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY'];
const isTransferable = (a) => !a.isSystem && TRANSFERABLE.includes(a.accountType);
const isOtherFunds = (a) => !['ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE', 'BANK'].includes(a.accountType);
const methodLabel = (id) => PAYMENT_METHODS.find((m) => m.id === id)?.label ?? id;

function DepositForm({ accounts, onPosted, onCancel }) {
  const { data: waiting, error: waitingError } = useLedgerQuery(api.listUndeposited);
  const defaultBank = useMemo(() => accounts.find((a) => a.isActive && isBank(a))?.id ?? null, [accounts]);
  const [bankId, setBankId] = useState(defaultBank);
  const [date, setDate] = useState(todayIso());
  const [memo, setMemo] = useState('');
  const [picked, setPicked] = useState(() => new Set());
  const [rows, setRows] = useState(() => [blankSplit(null)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const bankRef = useRef(null);
  const formClass = useFormClass(accounts);

  useEffect(() => bankRef.current?.focus(), []);
  useEffect(() => {
    if (bankId === null && defaultBank !== null) setBankId(defaultBank);
  }, [defaultBank, bankId]);

  const items = waiting ?? [];
  const key = (i) => `${i.kind}-${i.id}`;
  const selectedTotal = items.filter((i) => picked.has(key(i))).reduce((s, i) => s + i.amountCents, 0);
  const other = summarizeSplits(rows);
  const total = selectedTotal + other.total;

  const toggle = (item) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(key(item))) next.delete(key(item));
      else next.add(key(item));
      return next;
    });

  const save = async () => {
    if (bankId === null) return setError('Choose the bank account to deposit into.');
    if (other.invalid > 0) return setError(`${other.invalid} other-funds row(s) need an account and an amount above zero.`);
    if (picked.size === 0 && other.lines.length === 0) return setError('Select payments to deposit or add other funds.');
    const otherLines = formClass.apply(other.lines);
    const warning = formClass.missing(otherLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      await api.postBankDeposit({
        bankAccountId: bankId,
        depositDate: date,
        memo: memo.trim() || null,
        items: items.filter((i) => picked.has(key(i))).map((i) => ({ kind: i.kind, id: i.id })),
        otherLines,
      });
      onPosted(`Deposited ${formatCents(total)} into ${accountLabel(accounts, bankId)}.`);
      setPicked(new Set());
      setRows([blankSplit(null)]);
      setMemo('');
      formClass.reset();
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Bank deposit</h2>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect ref={bankRef} accounts={accounts} filter={isBank} value={bankId} onChange={setBankId} className="qb-input w-[200px]" />
        </label>
        {formClass.header}
        <label>
          <span className="qb-label">Date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[220px]" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        <div className="ml-auto text-right">
          <div className="qb-label">Total deposit</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
        </div>
      </div>
      <div className="border-b border-qb-border bg-qb-canvas px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        Select the payments included in this deposit ({items.length} in Undeposited Funds)
      </div>
      {waitingError && <Notice kind="error" text={waitingError} />}
      <table className="qb-grid">
        <colgroup>
          <col className="w-[32px]" />
          <col className="w-[92px]" />
          <col className="w-[110px]" />
          <col />
          <col className="w-[130px]" />
          <col className="w-[110px]" />
          <col className="w-[120px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head text-center">
              <input
                type="checkbox"
                className="accent-qb-green"
                checked={items.length > 0 && picked.size === items.length}
                onChange={(e) => setPicked(e.target.checked ? new Set(items.map(key)) : new Set())}
                aria-label="Select all payments"
              />
            </th>
            <th className="qb-grid-head">Date</th>
            <th className="qb-grid-head">Type</th>
            <th className="qb-grid-head">Received from</th>
            <th className="qb-grid-head">Method</th>
            <th className="qb-grid-head">Ref no.</th>
            <th className="qb-grid-head text-right">Amount</th>
          </tr>
        </thead>
        <tbody>
          {items.length === 0 && (
            <tr>
              <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No payments are waiting in Undeposited Funds.</td>
            </tr>
          )}
          {items.map((i) => (
            <tr key={key(i)} className={`qb-grid-row cursor-pointer ${picked.has(key(i)) ? 'is-active' : ''}`} onClick={() => toggle(i)}>
              <td className="qb-grid-cell text-center">
                <input type="checkbox" className="accent-qb-green" checked={picked.has(key(i))} onClick={(e) => e.stopPropagation()} onChange={() => toggle(i)} aria-label={`Deposit ${formatCents(i.amountCents)}`} />
              </td>
              <td className="qb-grid-cell">{formatDate(i.date)}</td>
              <td className="qb-grid-cell">{i.kind === 'PAYMENT' ? 'Payment' : 'Sales Receipt'}</td>
              <td className="qb-grid-cell">{i.customerName ?? ''}</td>
              <td className="qb-grid-cell">{methodLabel(i.paymentMethod)}</td>
              <td className="qb-grid-cell">{i.reference ?? ''}</td>
              <td className="qb-grid-cell num">{formatCents(i.amountCents)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="border-y border-qb-border bg-qb-canvas px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Add funds to this deposit</div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isOtherFunds} categoryLabel="Account" withClasses />
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : 'Save & Post Deposit'}
          </button>
        </div>
      </div>
    </section>
  );
}

function TransferForm({ accounts, onPosted, onCancel }) {
  const [fromId, setFromId] = useState(null);
  const [toId, setToId] = useState(null);
  const [date, setDate] = useState(todayIso());
  const [amount, setAmount] = useState('');
  const [memo, setMemo] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const fromRef = useRef(null);

  useEffect(() => fromRef.current?.focus(), []);
  const cents = parseMoneyToCents(amount);

  const save = async (e) => {
    e.preventDefault();
    if (fromId === null || toId === null) return setError('Choose both accounts.');
    if (fromId === toId) return setError('Choose two different accounts.');
    if (cents === null || cents <= 0) return setError('Enter an amount above zero.');
    setBusy(true);
    try {
      await api.postTransfer({ fromAccountId: fromId, toAccountId: toId, transferDate: date, amountCents: cents, memo: memo.trim() || null });
      onPosted(`Transferred ${formatCents(cents)} from ${accountLabel(accounts, fromId)} to ${accountLabel(accounts, toId)}.`);
      setAmount('');
      setMemo('');
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <form className="flex flex-wrap items-end gap-2 px-2 py-1" onSubmit={save}>
        <h2 className="mr-2 self-center text-md font-semibold">Transfer</h2>
        <label>
          <span className="qb-label">Transfer funds from</span>
          <AccountSelect ref={fromRef} accounts={accounts} filter={isTransferable} value={fromId} onChange={setFromId} placeholder="Select account" className="qb-input w-[210px]" />
        </label>
        <label>
          <span className="qb-label">Transfer funds to</span>
          <AccountSelect accounts={accounts} filter={(a) => isTransferable(a) && a.id !== fromId} value={toId} onChange={setToId} placeholder="Select account" className="qb-input w-[210px]" />
        </label>
        <label>
          <span className="qb-label">Amount</span>
          <input className="qb-input num w-[120px]" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" inputMode="decimal" aria-invalid={amount !== '' && (cents === null || cents <= 0)} />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[200px]" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        <div className="ml-auto flex items-center gap-2 self-center">
          {error && <span className="text-sm text-qb-negative">{error}</span>}
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="submit" className="qb-btn qb-btn-primary" disabled={busy}>
            {busy ? 'Posting…' : 'Save & Post Transfer'}
          </button>
        </div>
      </form>
    </section>
  );
}

export default function DepositsTransfers({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listBankingActivity);
  const fromIntent = intent?.action === 'bank-deposit' ? 'deposit' : intent?.action === 'transfer' ? 'transfer' : null;
  const [mode, setMode] = useState(fromIntent);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (intent?.action === 'bank-deposit') setMode('deposit');
    if (intent?.action === 'transfer') setMode('transfer');
  }, [intent]);

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };
  const rows = data ?? [];
  const posted = (text) => setNotice({ kind: 'success', text });

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {mode === 'deposit' && <DepositForm accounts={accounts} onPosted={posted} onCancel={() => setMode(null)} />}
      {mode === 'transfer' && <TransferForm accounts={accounts} onPosted={posted} onCancel={() => setMode(null)} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load activity: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Deposits & transfers</h2>
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setMode('transfer')}>
              Transfer
            </button>
            <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setMode('deposit')}>
              Bank deposit
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[84px]" />
              <col className="w-[220px]" />
              <col className="w-[220px]" />
              <col />
              <col className="w-[112px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head">Account</th>
                <th className="qb-grid-head">From / lines</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No deposits or transfers yet.</td>
                </tr>
              )}
              {rows.map((r) => (
                <tr key={`${r.kind}-${r.id}`} className={`qb-grid-row ${r.isVoided ? 'text-qb-ink-faint' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(r.date)}</td>
                  <td className="qb-grid-cell">{r.kind === 'DEPOSIT' ? 'Deposit' : 'Transfer'}</td>
                  <td className="qb-grid-cell">{r.kind === 'TRANSFER' ? accountLabel(accounts, r.otherAccountId) : accountLabel(accounts, r.accountId)}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">
                    {r.kind === 'TRANSFER' ? `from ${accountLabel(accounts, r.accountId)}` : `${r.lineCount} line${r.lineCount === 1 ? '' : 's'}`}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={r.memo ?? ''}>{r.isVoided ? 'Voided' : (r.memo ?? '')}</td>
                  <td className={`qb-grid-cell num ${r.isVoided ? 'line-through' : ''}`}>{formatCents(r.amountCents)}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      items={[
                        voidItem({
                          journalEntryId: r.journalEntryId,
                          voided: r.isVoided,
                          description:
                            r.kind === 'DEPOSIT'
                              ? `Deposit of ${formatCents(r.amountCents)} on ${formatDate(r.date)}. Any customer payments in it return to Undeposited Funds.`
                              : `Transfer of ${formatCents(r.amountCents)} on ${formatDate(r.date)}.`,
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
