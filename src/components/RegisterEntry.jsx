import { useEffect, useRef, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import NameField from './NameField.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { useTransactionDate } from '../lib/myPreferences.js';
import { useDirtySince } from '../lib/windows.js';
import DateBox from './DateBox.jsx';

/** Accounts a register row can go to: not A/R or A/P (those need an invoice or bill) or Undeposited Funds. */
const isEntryAccount = (a) => !['ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE'].includes(a.accountType) && !(a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');

/** The two amount columns of each register that takes typed entries, in the register's order. */
const SIDES = {
  BANK: [
    { side: 'PAYMENT', records: 'check' },
    { side: 'DEPOSIT', records: 'deposit' },
  ],
  CREDIT_CARD: [
    { side: 'CHARGE', records: 'credit card charge' },
    { side: 'CREDIT', records: 'credit card credit' },
  ],
};

/** Whether a register can take typed entries (bank and credit card registers, without subaccounts). */
export const takesEntries = (account, includeSubaccounts) => Boolean(account && SIDES[account.accountType] && !includeSubaccounts);

/**
 * The blank row at the bottom of a bank or credit card register: date, number, payee, account,
 * memo and an amount in one of the two columns. Record posts it through the form it stands for
 * (a check, deposit, card charge or card credit; `registers/entry.rs`), Restore clears it, and
 * Enter records it once it is complete. Splits… spreads it over several accounts.
 */
export default function RegisterEntry({ account, accounts, heads, onRecorded }) {
  const sides = SIDES[account.accountType];
  const isBank = account.accountType === 'BANK';
  const [date, setDate] = useTransactionDate();
  const [number, setNumber] = useState('');
  const [payee, setPayee] = useState('');
  const [otherId, setOtherId] = useState(null);
  const [memo, setMemo] = useState('');
  const [amounts, setAmounts] = useState(['', '']);
  // Which column the amount is in; set by typing in one, and kept while the row is split.
  const [sideIndex, setSideIndex] = useState(0);
  const [splits, setSplits] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const dateRef = useRef(null);
  const vendors = useLedgerQuery(api.listVendors).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  useDirtySince(`register-entry-${account.id}`, { payee, otherId, memo, amounts, splits: splits?.map((r) => [r.accountId, r.amount, r.memo]) ?? null });

  // A bank register suggests the next check number, again after each row is recorded.
  const [recorded, setRecorded] = useState(0);
  useEffect(() => {
    if (!isBank) return undefined;
    let cancelled = false;
    api.nextCheckNumber(account.id).then(
      (n) => !cancelled && setNumber(n ?? ''),
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [account.id, isBank, recorded]);

  const { side, records } = sides[sideIndex];
  const isDeposit = side === 'DEPOSIT';
  const split = splits ? summarizeSplits(splits) : null;
  // What the split lines add up to as typed, including lines still waiting for an account.
  const typedTotal = splits ? splits.reduce((sum, r) => sum + Math.max(parseMoneyToCents(r.amount) ?? 0, 0), 0) : 0;
  const amountCents = split ? typedTotal : parseMoneyToCents(amounts[sideIndex]);
  // What still keeps the row from being recorded, or null when it is complete.
  const missing = !/^\d{4}-\d{2}-\d{2}$/.test(date ?? '')
    ? 'a date'
    : split
      ? split.invalid > 0
        ? 'an account and an amount on every split line'
        : split.lines.length === 0
          ? 'a split line'
          : null
      : amountCents === null || amountCents <= 0
        ? `an amount (${heads[0]} or ${heads[1]})`
        : otherId === null
          ? 'the account it goes to'
          : null;

  const typeAmount = (index, text) => {
    setAmounts(index === 0 ? [text, ''] : ['', text]);
    if (text.trim() !== '') setSideIndex(index);
    setError(null);
  };

  const restore = () => {
    setPayee('');
    setOtherId(null);
    setMemo('');
    setAmounts(['', '']);
    setSideIndex(0);
    setSplits(null);
    setError(null);
  };

  const record = async () => {
    if (busy) return;
    if (missing) {
      setError(`To record this ${records}, add ${missing}.`);
      return;
    }
    const lines = split
      ? split.lines.map((l) => ({ accountId: l.accountId, amountCents: l.amountCents, memo: l.memo, classId: l.classId, customerId: isDeposit ? null : l.customerId }))
      : [{ accountId: otherId, amountCents, memo: null, classId: null }];
    setBusy(true);
    try {
      const posted = await api.postRegisterEntry({
        accountId: account.id,
        entryDate: date,
        number: isBank && side === 'PAYMENT' ? number.trim() || null : null,
        payeeName: payee.trim() || null,
        memo: memo.trim() || null,
        side,
        lines,
      });
      restore();
      setRecorded((n) => n + 1);
      onRecorded?.(posted, `${records[0].toUpperCase()}${records.slice(1)}${isBank && side === 'PAYMENT' && number.trim() ? ` #${number.trim()}` : ''} recorded: ${formatCents(amountCents)}.`);
      dateRef.current?.focus();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const openSplits = () => {
    const first = { ...blankSplit(otherId), amount: amountCents ? centsToInput(amountCents) : '' };
    setSplits([first, blankSplit()]);
  };
  const closeSplits = () => {
    // Back to one line: keep the first account and the total.
    setOtherId(splits.find((r) => r.accountId !== null)?.accountId ?? otherId);
    const total = typedTotal;
    setAmounts(sideIndex === 0 ? [total ? centsToInput(total) : '', ''] : ['', total ? centsToInput(total) : '']);
    setSplits(null);
  };

  // Enter records a complete row (with "Enter moves between fields" on too); on an incomplete row
  // it moves on, or says what is missing.
  const onKeyDown = (e) => {
    if (e.key !== 'Enter' || e.defaultPrevented || e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target instanceof HTMLButtonElement || e.target instanceof HTMLTextAreaElement) return;
    if (!missing) {
      e.preventDefault();
      record();
    }
  };

  const payeeNames = [...customers, ...vendors];
  return (
    <form
      className="flex-none border-t-2 border-qb-border bg-qb-canvas"
      aria-label={`New ${heads[0].toLowerCase()} or ${heads[1].toLowerCase()} in this register`}
      onKeyDown={onKeyDown}
      onSubmit={(e) => {
        e.preventDefault();
        record();
      }}
    >
      <div className="flex flex-wrap items-end gap-1 px-2 py-1">
        <label>
          <span className="qb-label">Date</span>
          <DateBox ref={dateRef} className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} required />
        </label>
        {isBank && (
          <label>
            <span className="qb-label">Number</span>
            <input className="qb-input w-[72px]" value={number} onChange={(e) => setNumber(e.target.value)} disabled={side !== 'PAYMENT'} title={side === 'PAYMENT' ? 'Check number' : 'Deposits have no number'} />
          </label>
        )}
        <label>
          <span className="qb-label">{isDeposit ? 'Received from' : 'Payee'}</span>
          {isDeposit ? (
            <>
              <input className="qb-input w-[190px]" list={`register-payees-${account.id}`} value={payee} onChange={(e) => setPayee(e.target.value)} title="A customer or vendor; a name on neither list is kept in the deposit's memo" />
              <datalist id={`register-payees-${account.id}`}>
                {payeeNames.map((p) => (
                  <option key={`${p.id}-${p.name}`} value={p.name} />
                ))}
              </datalist>
            </>
          ) : (
            <NameField kind="vendor" names={vendors} value={payee} onChange={setPayee} className="qb-input w-[190px]" placeholder="Who was paid?" />
          )}
        </label>
        <label>
          <span className="qb-label">Account</span>
          {splits ? (
            <div className="qb-input flex w-[230px] items-center text-qb-ink-muted">-Split- ({split.lines.length} lines)</div>
          ) : (
            <AccountSelect
              accounts={accounts}
              value={otherId}
              onChange={setOtherId}
              filter={(a) => isEntryAccount(a) && a.id !== account.id}
              placeholder="Choose an account…"
              className="qb-input w-[230px]"
            />
          )}
        </label>
        <label className="min-w-[120px] flex-1">
          <span className="qb-label">Memo</span>
          <input className="qb-input w-full" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        {heads.map((head, index) => (
          <label key={head}>
            <span className="qb-label">{head}</span>
            <input
              className="qb-input num w-[104px]"
              inputMode="decimal" data-money
              value={splits ? (index === sideIndex && typedTotal ? centsToInput(typedTotal) : '') : amounts[index]}
              onChange={(e) => typeAmount(index, e.target.value)}
              readOnly={Boolean(splits)}
              onFocus={() => splits && setSideIndex(index)}
              title={splits ? 'The total of the split lines; click to move it to this column' : undefined}
              aria-label={head}
            />
          </label>
        ))}
        <button type="submit" className="qb-btn qb-btn-primary h-row-lg" disabled={busy}>
          Record
        </button>
        <button type="button" className="qb-btn qb-btn-secondary h-row-lg" onClick={restore} disabled={busy}>
          Restore
        </button>
        <button type="button" className={`qb-btn h-row-lg ${splits ? 'qb-btn-primary' : 'qb-btn-secondary'}`} onClick={splits ? closeSplits : openSplits} aria-expanded={Boolean(splits)}>
          Splits…
        </button>
      </div>
      {splits && (
        <div className="max-h-[200px] overflow-auto border-t border-qb-border bg-qb-panel scrollbar-thin">
          <SplitGrid rows={splits} setRows={setSplits} accounts={accounts} filter={(a) => isEntryAccount(a) && a.id !== account.id} categoryLabel="Account" withClasses withJobs={!isDeposit} />
        </div>
      )}
      <div className="flex items-center gap-2 px-2 pb-1 text-xs text-qb-ink-muted">
        {error ? (
          <span role="alert" className="text-qb-negative">{error}</span>
        ) : (
          <span>
            Records a {records}
            {isDeposit && payee.trim() && !payeeNames.some((p) => p.name.toLocaleLowerCase() === payee.trim().toLocaleLowerCase()) ? ` (a name on neither list is kept in its memo)` : ''} · Enter records a complete row · Restore clears it
          </span>
        )}
      </div>
    </form>
  );
}
