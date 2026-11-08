import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { useMyPreferences, useTransactionDate } from '../lib/myPreferences.js';
import { useAttachmentRows } from './Attachments.jsx';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import FormToolbar, { fileButtons, navButtons } from './FormToolbar.jsx';
import { useDirtySince, useOpenFromList } from '../lib/windows.js';
import SavedDocView, { useSavedDoc } from './SavedDocView.jsx';
import { byDateThenNumber } from '../lib/formStep.js';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { kindLabel } from './ProfileFields.jsx';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';

const isBank = (a) => a.accountType === 'BANK';
const TRANSFERABLE = ['BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY'];
const isTransferable = (a) => !a.isSystem && TRANSFERABLE.includes(a.accountType);
const isOtherFunds = (a) => !['ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE', 'BANK'].includes(a.accountType);

function DepositForm({ accounts, onPosted, onCancel, savedStep }) {
  const { data: waiting, error: waitingError } = useLedgerQuery(api.listUndeposited);
  // Deposits open on the account chosen in Preferences › Checking (My Preferences), else the first bank.
  const { depositAccountId } = useMyPreferences();
  const defaultBank = useMemo(
    () => accounts.find((a) => a.id === depositAccountId && a.isActive && isBank(a))?.id ?? accounts.find((a) => a.isActive && isBank(a))?.id ?? null,
    [accounts, depositAccountId],
  );
  const [bankId, setBankId] = useState(defaultBank);
  const [date, setDate] = useTransactionDate();
  const [memo, setMemo] = useState('');
  const [picked, setPicked] = useState(() => new Set());
  const [rows, setRows] = useState(() => [blankSplit(null)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const bankRef = useRef(null);
  // Opening a saved deposit (Previous) asks for the entries to be saved or cleared first.
  // Cash back: part of the deposit kept as cash (petty cash, say) instead of banked.
  const [cashAccountId, setCashAccountId] = useState(null);
  const [cashMemo, setCashMemo] = useState('');
  const [cashAmount, setCashAmount] = useState('');
  const dirty = useDirtySince('deposit', { memo, picked: [...picked], rows: rows.map((r) => [r.memo, r.amount]), cashAmount, cashMemo });
  const openFromList = useOpenFromList();
  const formClass = useFormClass(accounts);

  useEffect(() => bankRef.current?.focus(), []);
  useEffect(() => {
    if (bankId === null && defaultBank !== null) setBankId(defaultBank);
  }, [defaultBank, bankId]);

  const items = waiting ?? [];
  const key = (i) => `${i.kind}-${i.id}`;
  const selectedTotal = items.filter((i) => picked.has(key(i))).reduce((s, i) => s + i.amountCents, 0);
  const other = summarizeSplits(rows);
  const subtotal = selectedTotal + other.total;
  const cashCents = cashAmount.trim() === '' ? 0 : parseMoneyToCents(cashAmount);
  const total = subtotal - (cashCents ?? 0);

  const toggle = (item) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(key(item))) next.delete(key(item));
      else next.add(key(item));
      return next;
    });

  const clear = () => {
    setPicked(new Set());
    setRows([blankSplit(null)]);
    setMemo('');
    setCashAccountId(null);
    setCashMemo('');
    setCashAmount('');
    formClass.reset();
    setError(null);
    bankRef.current?.focus();
  };

  const save = async () => {
    if (bankId === null) return setError('Choose the bank account to deposit into.');
    if (other.invalid > 0) return setError(`${other.invalid} other-funds row(s) need an account and an amount above zero.`);
    if (picked.size === 0 && other.lines.length === 0) return setError('Select payments to deposit or add other funds.');
    if (cashCents === null || cashCents < 0) return setError('Enter the cash back as an amount, such as 50.00.');
    if (cashCents > 0 && cashAccountId === null) return setError('Choose the account the cash back goes to, such as Petty Cash.');
    if (cashCents > 0 && cashCents >= subtotal) return setError(`Cash back must be less than the deposit subtotal, ${formatCents(subtotal)}.`);
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
        cashBack: cashCents > 0 ? { accountId: cashAccountId, amountCents: cashCents, memo: cashMemo.trim() || null } : null,
      });
      onPosted(`Deposited ${formatCents(total)} into ${accountLabel(accounts, bankId)}${cashCents > 0 ? `, with ${formatCents(cashCents)} cash back to ${accountLabel(accounts, cashAccountId)}` : ''}.`);
      clear();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save, clear, onClose: onCancel, busy });

  return (
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel flex-none">
      <FormToolbar
        label="Bank deposit"
        groups={[navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...(savedStep ? savedStep(busy || dirty) : {}) }), fileButtons({ onNew: clear, onSave: flow.saveAndNew, busy })]}
      />
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Bank deposit</h2>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect ref={bankRef} accounts={accounts} filter={isBank} value={bankId} onChange={setBankId} className="qb-input w-[200px]" />
        </label>
        {formClass.header}
        <label>
          <span className="qb-label">Date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
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
              <td className="qb-grid-cell">{i.paymentMethodName ?? kindLabel(i.paymentMethod)}</td>
              <td className="qb-grid-cell">{i.reference ?? ''}</td>
              <td className="qb-grid-cell num">{formatCents(i.amountCents)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="border-y border-qb-border bg-qb-canvas px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Add funds to this deposit</div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isOtherFunds} categoryLabel="Account" withClasses />
      <div className="flex flex-wrap items-end gap-2 border-t border-qb-border px-2 py-1">
        <label>
          <span className="qb-label">Cash back goes to</span>
          <AccountSelect accounts={accounts} filter={(a) => a.id !== bankId} value={cashAccountId} onChange={setCashAccountId} className="qb-input w-[200px]" placeholder="No cash back" />
        </label>
        <label>
          <span className="qb-label">Cash back memo</span>
          <input className="qb-input w-[200px]" value={cashMemo} onChange={(e) => setCashMemo(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Cash back amount</span>
          <input className="qb-input num w-[110px]" inputMode="decimal" value={cashAmount} onChange={(e) => setCashAmount(e.target.value)} aria-invalid={cashCents === null || undefined} />
        </label>
        <div className="ml-auto text-right text-sm">
          <div>
            Deposit subtotal <span className="num inline-block w-[110px]">{formatCents(subtotal)}</span>
          </div>
          {cashCents > 0 && (
            <div>
              Less cash back <span className="num inline-block w-[110px]">−{formatCents(cashCents)}</span>
            </div>
          )}
        </div>
      </div>
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <SaveButtons flow={flow} busyLabel="Posting…" />
        </div>
      </div>
    </section>
  );
}

function TransferForm({ accounts, onPosted, onCancel, savedStep }) {
  const [fromId, setFromId] = useState(null);
  const [toId, setToId] = useState(null);
  const [date, setDate] = useTransactionDate();
  const [amount, setAmount] = useState('');
  const [memo, setMemo] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const fromRef = useRef(null);
  // Opening a saved transfer (Previous) asks for the entries to be saved or cleared first.
  const dirty = useDirtySince('transfer', { fromId, toId, amount, memo });
  const openFromList = useOpenFromList();

  useEffect(() => fromRef.current?.focus(), []);
  const cents = parseMoneyToCents(amount);

  const clear = () => {
    setFromId(null);
    setToId(null);
    setAmount('');
    setMemo('');
    setError(null);
    fromRef.current?.focus();
  };

  const save = async () => {
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
      fromRef.current?.focus();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save, clear, onClose: onCancel, busy });

  return (
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel flex-none">
      <FormToolbar
        label="Transfer"
        groups={[navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...(savedStep ? savedStep(busy || dirty) : {}) }), fileButtons({ onNew: clear, onSave: flow.saveAndNew, busy })]}
      />
      <form
        className="flex flex-wrap items-end gap-2 px-2 py-1"
        onSubmit={(e) => {
          e.preventDefault();
          flow.saveAndNew();
        }}
      >
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
          <input className="qb-input num w-[120px]" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" inputMode="decimal" data-money aria-invalid={amount !== '' && (cents === null || cents <= 0)} />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
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
          <SaveButtons flow={flow} busyLabel="Posting…" />
        </div>
      </form>
    </section>
  );
}

/**
 * Pay Down Credit Card: pays a card's balance from a bank account. It posts as a transfer
 * (Dr the card, Cr the bank), so it shows here and in both registers.
 */
function CardPaydownForm({ accounts, onPosted, onCancel }) {
  const cards = accounts.filter((a) => a.isActive && a.accountType === 'CREDIT_CARD' && !a.isSystem);
  const banks = accounts.filter((a) => a.isActive && isBank(a) && !a.isSystem);
  const [cardId, setCardId] = useState(cards[0]?.id ?? null);
  const [bankId, setBankId] = useState(banks[0]?.id ?? null);
  const [date, setDate] = useTransactionDate();
  const card = accounts.find((a) => a.id === cardId);
  const owed = Math.max(0, card?.balanceCents ?? 0);
  const [amount, setAmount] = useState('');
  const [memo, setMemo] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const cardRef = useRef(null);

  useEffect(() => cardRef.current?.focus(), []);
  // The amount starts at what the card owes now, and follows the card chosen.
  useEffect(() => setAmount(owed > 0 ? centsToInput(owed) : ''), [cardId, owed]);
  const cents = parseMoneyToCents(amount);

  const save = async (e) => {
    e.preventDefault();
    if (cardId === null) return setError('Choose the credit card to pay.');
    if (bankId === null) return setError('Choose the bank account to pay from.');
    if (cents === null || cents <= 0) return setError('Enter an amount above zero.');
    setBusy(true);
    try {
      await api.postTransfer({
        fromAccountId: bankId,
        toAccountId: cardId,
        transferDate: date,
        amountCents: cents,
        memo: memo.trim() || `Payment to ${accountLabel(accounts, cardId)}`,
      });
      onPosted(`Paid ${formatCents(cents)} to ${accountLabel(accounts, cardId)} from ${accountLabel(accounts, bankId)}: Dr the card / Cr the bank.`);
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
        <h2 className="mr-2 self-center text-md font-semibold">Pay down credit card</h2>
        <label>
          <span className="qb-label">Credit card</span>
          <AccountSelect
            ref={cardRef}
            accounts={accounts}
            filter={(a) => a.accountType === 'CREDIT_CARD' && !a.isSystem}
            value={cardId}
            onChange={setCardId}
            placeholder="Select card"
            className="qb-input w-[210px]"
          />
        </label>
        <span className="self-center text-sm text-qb-ink-muted">Balance owed {formatCents(owed)}</span>
        <label>
          <span className="qb-label">Pay from</span>
          <AccountSelect accounts={accounts} filter={(a) => isBank(a) && !a.isSystem} value={bankId} onChange={setBankId} placeholder="Select bank" className="qb-input w-[210px]" />
        </label>
        <label>
          <span className="qb-label">Amount</span>
          <input className="qb-input num w-[120px]" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" inputMode="decimal" data-money aria-invalid={amount !== '' && (cents === null || cents <= 0)} />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[200px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder={cardId ? `Payment to ${accountLabel(accounts, cardId)}` : ''} />
        </label>
        <div className="ml-auto flex items-center gap-2 self-center">
          {error && <span className="text-sm text-qb-negative">{error}</span>}
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="submit" className="qb-btn qb-btn-primary" disabled={busy}>
            {busy ? 'Posting…' : 'Save & Pay'}
          </button>
        </div>
      </form>
    </section>
  );
}

const INTENT_MODES = { 'bank-deposit': 'deposit', transfer: 'transfer', 'pay-card': 'pay-card' };

export default function DepositsTransfers({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listBankingActivity);
  const [mode, setMode] = useState(INTENT_MODES[intent?.action] ?? null);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (INTENT_MODES[intent?.action]) setMode(INTENT_MODES[intent.action]);
  }, [intent]);

  const attachments = useAttachmentRows();
  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };
  const rows = data ?? [];
  const sorter = useSort(rows, {
    date: (r) => r.date,
    type: (r) => (r.kind === 'DEPOSIT' ? 'Deposit' : 'Transfer'),
    account: (r) => accountLabel(accounts, r.kind === 'TRANSFER' ? r.otherAccountId : r.accountId),
    from: (r) => (r.kind === 'TRANSFER' ? accountLabel(accounts, r.accountId) : r.lineCount),
    memo: (r) => (r.isVoided ? 'Voided' : r.memo),
    amount: (r) => r.amountCents,
  }, { storageKey: 'deposits' });
  const posted = (text) => setNotice({ kind: 'success', text });
  // Saved deposits (or transfers, on that form) open read-only in the form's place.
  const viewKind = mode === 'transfer' ? 'TRANSFER' : 'DEPOSIT';
  const ordered = useMemo(() => byDateThenNumber(rows.filter((r) => r.kind === viewKind), (r) => r.date, () => null), [rows, viewKind]);
  const saved = useSavedDoc(ordered);
  const viewing = mode === 'deposit' || mode === 'transfer' ? saved.current : null;
  const openFromList = useOpenFromList();
  const openRow = (r) => {
    setMode(r.kind === 'TRANSFER' ? 'transfer' : 'deposit');
    saved.open(r);
  };
  const viewLabel = viewing?.kind === 'TRANSFER' ? 'Transfer' : 'Deposit';

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {viewing && (
        <SavedDocView
          label={viewLabel}
          title={`${viewLabel} of ${formatCents(viewing.amountCents)} · ${formatDate(viewing.date)} · ${accountLabel(accounts, viewing.kind === 'TRANSFER' ? viewing.otherAccountId : viewing.accountId)}${viewing.isVoided ? ' (voided)' : ''}`}
          step={saved.step}
          onFind={() => openFromList('accounting/find', 'find', { text: '' })}
          onNew={saved.close}
          entryId={viewing.journalEntryId}
          voucher={{
            entryId: viewing.journalEntryId,
            title: viewLabel,
            date: viewing.date,
            name: accountLabel(accounts, viewing.kind === 'TRANSFER' ? viewing.otherAccountId : viewing.accountId),
            nameLabel: viewing.kind === 'TRANSFER' ? 'To' : 'Deposited to',
            memo: viewing.memo,
            amountCents: viewing.amountCents,
            voided: viewing.isVoided,
            footer: viewing.kind === 'TRANSFER' ? `From ${accountLabel(accounts, viewing.accountId)}` : null,
          }}
          onNotice={setNotice}
          void={{
            onVoid: () => voidEntry(viewing.journalEntryId),
            disabled: viewing.isVoided || !viewing.journalEntryId,
            confirm: {
              title: 'Void this transaction?',
              body:
                viewing.kind === 'DEPOSIT'
                  ? `Deposit of ${formatCents(viewing.amountCents)} on ${formatDate(viewing.date)}. Any customer payments in it return to Undeposited Funds. This cannot be undone.`
                  : `Transfer of ${formatCents(viewing.amountCents)} on ${formatDate(viewing.date)}. This cannot be undone.`,
              confirmLabel: 'Yes, void it',
            },
          }}
        />
      )}
      {mode === 'deposit' && !viewing && <DepositForm accounts={accounts} savedStep={(blocked) => saved.fromNew(blocked)} onPosted={posted} onCancel={() => setMode(null)} />}
      {mode === 'transfer' && !viewing && <TransferForm accounts={accounts} savedStep={(blocked) => saved.fromNew(blocked)} onPosted={posted} onCancel={() => setMode(null)} />}
      {mode === 'pay-card' && <CardPaydownForm accounts={accounts} onPosted={posted} onCancel={() => setMode(null)} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load activity: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Deposits & transfers</h2>
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setMode('pay-card')}>
              Pay down card
            </button>
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
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="type">Type</SortTh>
                <SortTh sorter={sorter} col="account">Account</SortTh>
                <SortTh sorter={sorter} col="from">From / lines</SortTh>
                <SortTh sorter={sorter} col="memo">Memo</SortTh>
                <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No deposits or transfers yet.</td>
                </tr>
              )}
              {sorter.sorted.map((r) => (
                <Fragment key={`${r.kind}-${r.id}`}>
                <tr className={`qb-grid-row cursor-pointer ${r.isVoided ? 'text-qb-ink-faint' : ''}`} onDoubleClick={() => openRow(r)}>
                  <td className="qb-grid-cell">{formatDate(r.date)}</td>
                  <td className="qb-grid-cell">
                    {r.kind === 'DEPOSIT' ? 'Deposit' : 'Transfer'}
                    {attachments.clip(r.journalEntryId)}
                  </td>
                  <td className="qb-grid-cell">{r.kind === 'TRANSFER' ? accountLabel(accounts, r.otherAccountId) : accountLabel(accounts, r.accountId)}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">
                    {r.kind === 'TRANSFER' ? `from ${accountLabel(accounts, r.accountId)}` : `${r.lineCount} line${r.lineCount === 1 ? '' : 's'}`}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={r.memo ?? ''}>{r.isVoided ? 'Voided' : (r.memo ?? '')}</td>
                  <td className={`qb-grid-cell num ${r.isVoided ? 'line-through' : ''}`}>{formatCents(r.amountCents)}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      items={[
                        { label: 'Open', onSelect: () => openRow(r) },
                        attachments.menuItem(r.journalEntryId),
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
                {attachments.row(r.journalEntryId, 7)}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
