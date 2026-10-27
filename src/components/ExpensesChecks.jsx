import { useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, splitRowsFrom, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { api, errorText, PAYMENT_METHODS, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatMoney, formatRate, parseRateE6, toHomeCents } from '../lib/money.js';
import CurrencyFields from './CurrencyFields.jsx';
import { CheckDocument, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import { useDirtySince, useEditIntent, useOpenFromList, useWindowContent } from '../lib/windows.js';

// Any account a cash outlay can be categorized to: not the payment side, not A/R or A/P.
const isSplitAccount = (a) => !['ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE'].includes(a.accountType) && !(a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');

// The form's kinds: a card credit is a refund or credit from a vendor onto a credit card.
export const EXPENSE_KINDS = { EXPENSE: 'Expense', CHECK: 'Check', CARD_CREDIT: 'Card credit' };

const PAY_FROM = {
  EXPENSE: (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD',
  CHECK: (a) => a.accountType === 'BANK',
  CARD_CREDIT: (a) => a.accountType === 'CREDIT_CARD',
};

/** A new expense, check or card credit, or (with `editing` = { expense, lines }) a posted one being edited. */
function ExpenseForm({ kind, setKind, accounts, vendors, expenses, onPosted, onCancel, editing = null }) {
  const payFrom = PAY_FROM[kind];
  const isCredit = kind === 'CARD_CREDIT';
  const defaultPayFrom = useMemo(
    () => accounts.find((a) => a.isActive && a.accountType === (kind === 'CARD_CREDIT' ? 'CREDIT_CARD' : 'BANK'))?.id ?? null,
    [accounts, kind],
  );
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'EXPENSE' && !a.isSystem)?.id ?? null, [accounts]);
  const nextCheck = useMemo(() => {
    const numbers = expenses.filter((e) => e.kind === 'CHECK' && /^\d+$/.test(e.checkNumber ?? '')).map((e) => Number(e.checkNumber));
    return numbers.length ? String(Math.max(...numbers) + 1) : '1001';
  }, [expenses]);

  const was = editing?.expense;
  const [payee, setPayee] = useState(was?.payeeName ?? '');
  const [accountId, setAccountId] = useState(was?.paymentAccountId ?? defaultPayFrom);
  const [date, setDate] = useState(was?.expenseDate ?? todayIso());
  const [checkNumber, setCheckNumber] = useState(was ? (was.checkNumber ?? '') : nextCheck);
  const [method, setMethod] = useState(was?.paymentMethod ?? 'CREDIT_CARD');
  const [memo, setMemo] = useState(was?.memo ?? '');
  const { homeCurrency } = useCompany();
  const [currency, setCurrency] = useState(was?.currencyCode ?? '');
  const [rate, setRate] = useState(was?.currencyCode ? formatRate(was.exchangeRateE6) : '');
  const rateE6 = currency ? parseRateE6(rate) : null;
  const [rows, setRows] = useState(() => (editing ? splitRowsFrom(editing.lines) : [blankSplit(defaultCategory), blankSplit(defaultCategory)]));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const payeeRef = useRef(null);
  const formClass = useFormClass(accounts);
  // Closing the window asks first while the entry has amounts or a payee not yet saved.
  useDirtySince('expense', { payee, date, memo, rows: rows.map((r) => [r.memo, r.amount, r.qty, r.rate]) });

  useEffect(() => payeeRef.current?.focus(), [kind]);
  useEffect(() => {
    if (!editing) setCheckNumber(nextCheck);
  }, [nextCheck, editing]);
  useEffect(() => {
    if (accountId === null && defaultPayFrom !== null) setAccountId(defaultPayFrom);
  }, [defaultPayFrom, accountId]);
  useEffect(() => {
    if (defaultCategory === null) return;
    setRows((rs) => rs.map((r) => (r.accountId === null ? { ...r, accountId: defaultCategory } : r)));
  }, [defaultCategory]);
  // A check comes from a bank account and a card credit goes to a card; drop a selection that
  // no longer fits when switching.
  useEffect(() => {
    const current = accounts.find((a) => a.id === accountId);
    if (current && !payFrom(current)) setAccountId(defaultPayFrom);
  }, [kind, accountId, accounts, defaultPayFrom, payFrom]);

  const { lines, invalid, total } = summarizeSplits(rows);

  const [memorizing, setMemorizing] = useState(null);
  const save = async (mode) => {
    if (accountId === null)
      return setError(kind === 'CHECK' ? 'Choose the bank account the check is drawn on.' : isCredit ? 'Choose the credit card the credit goes to.' : 'Choose the payment account.');
    if (invalid > 0) return setError(`${invalid} split row(s) need a category and an amount above zero.`);
    if (lines.length === 0) return setError('Add at least one split row with an amount.');
    if (lines.some((l) => l.accountId === accountId)) return setError(`A split row cannot use the ${isCredit ? 'card' : 'payment account'} itself.`);
    if (currency && rateE6 === null) return setError(`Enter the ${currency} to ${homeCurrency} exchange rate.`);
    const postedLines = formClass.apply(lines);
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const input = {
        kind,
        payeeName: payee.trim() || null,
        paymentAccountId: accountId,
        expenseDate: date,
        checkNumber: kind === 'CHECK' ? checkNumber.trim() || null : null,
        paymentMethod: kind === 'EXPENSE' ? method : null,
        memo: memo.trim() || null,
        lines: postedLines,
        currency: currency ? { code: currency, exchangeRateE6: rateE6 } : null,
      };
      if (mode === 'memorize') return setMemorizing({ ...input, checkNumber: null });
      if (editing) {
        await api.replaceExpense(was.id, input);
        onPosted(`${EXPENSE_KINDS[kind]} replaced: the original is voided and the edited one is posted.`);
        return onCancel();
      }
      const posted = await api.postExpense(input);
      const splits = `${lines.length} split${lines.length === 1 ? '' : 's'}`;
      onPosted(
        `${kind === 'CHECK' ? `Check${posted.checkNumber ? ` #${posted.checkNumber}` : ''}` : EXPENSE_KINDS[kind]} of ${formatMoney(posted.totalCents, posted.currencyCode)}${posted.currencyCode ? ` (${formatCents(posted.homeTotalCents)})` : ''} posted: ` +
          (isCredit ? `Dr ${accountLabel(accounts, accountId)} / Cr ${splits}.` : `Cr ${accountLabel(accounts, accountId)} / Dr ${splits}.`),
      );
      setPayee('');
      setMemo('');
      setRows([blankSplit(defaultCategory), blankSplit(defaultCategory)]);
      formClass.reset();
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
          {Object.entries(EXPENSE_KINDS).map(([id, label]) => (
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
          <input
            ref={payeeRef}
            className="qb-input w-[200px]"
            value={payee}
            onChange={(e) => setPayee(e.target.value)}
            list="expense-payees"
            placeholder={isCredit ? 'Who gave the credit?' : 'Who did you pay?'}
          />
          <datalist id="expense-payees">
            {vendors.map((v) => (
              <option key={v.id} value={v.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">{kind === 'CHECK' ? 'Bank account' : isCredit ? 'Credit card' : 'Payment account'}</span>
          <AccountSelect accounts={accounts} filter={payFrom} value={accountId} onChange={setAccountId} className="qb-input w-[200px]" />
        </label>
        <label>
          <span className="qb-label">{kind === 'CHECK' ? 'Check date' : isCredit ? 'Credit date' : 'Payment date'}</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {kind === 'CHECK' ? (
          <label>
            <span className="qb-label">Check no.</span>
            <input className="qb-input num w-[80px]" value={checkNumber} onChange={(e) => setCheckNumber(e.target.value)} />
          </label>
        ) : isCredit ? null : (
          <label>
            <span className="qb-label">Payment method</span>
            <select className="qb-input w-[140px]" value={method} onChange={(e) => setMethod(e.target.value)}>
              {PAYMENT_METHODS.map((m) => (
                <option key={m.id} value={m.id}>{m.label}</option>
              ))}
            </select>
          </label>
        )}
        {formClass.header}
        <CurrencyFields homeCurrency={homeCurrency} currency={currency} setCurrency={setCurrency} rate={rate} setRate={setRate} date={date} />
        <div className="ml-auto text-right">
          <div className="qb-label">{isCredit ? 'Credit amount' : 'Total'}</div>
          <div className="num text-3xl font-semibold leading-none">{formatMoney(total, currency || null)}</div>
          {currency && rateE6 !== null && total > 0 && <div className="text-xs text-qb-ink-muted">≈ {formatCents(toHomeCents(total, rateE6))} in {homeCurrency}</div>}
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={(a) => isSplitAccount(a) && a.id !== accountId} defaultAccountId={defaultCategory} withClasses withJobs currency={currency || null} />
      <div className="flex items-center gap-2 px-2 py-1">
        <input className="qb-input w-[320px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo" />
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => save('memorize')} disabled={busy} title="Save as a recurring transaction">
            Memorize…
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : editing ? 'Save changes' : `Save & Post ${kind === 'CARD_CREDIT' ? 'Credit' : EXPENSE_KINDS[kind]}`}
          </button>
        </div>
      </div>
      {memorizing && (
        <MemorizeDialog kind="EXPENSE" payload={memorizing} date={date} defaultName={payee.trim()} onClose={() => setMemorizing(null)} onSaved={(text) => { setMemorizing(null); onPosted(text); }} />
      )}
    </section>
  );
}

// "+ New" items that open this screen's form.
const INTENT_KINDS = { expense: 'EXPENSE', check: 'CHECK', 'card-credit': 'CARD_CREDIT' };

export default function ExpensesChecks({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listExpenses);
  const vendors = useLedgerQuery(api.listVendors);
  const initialKind = INTENT_KINDS[intent?.action] ?? null;
  const [kind, setKind] = useState(initialKind);
  const [notice, setNotice] = useState(null);
  const [editing, setEditing] = useState(null); // { expense, lines }
  const [printing, setPrinting] = useState(null);
  const company = useCompany();
  const startEdit = (expense) =>
    api.getDocumentLines('expense', expense.id).then(
      (lines) => {
        setEditing({ expense, lines });
        setKind(expense.kind);
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const printCheck = (expense) => {
    setPrinting(expense);
    // Let React render the check into the print root before the dialog opens.
    setTimeout(() => printDocument(), 80);
  };

  useEffect(() => {
    if (INTENT_KINDS[intent?.action]) setKind(INTENT_KINDS[intent.action]);
  }, [intent]);
  // Multiple Windows: a check or expense chosen in the list opens in a window of its own.
  const openFromList = useOpenFromList();
  const openExpense = (expense) => openFromList('expenses/expenses', 'edit', { expenseId: expense.id }, () => startEdit(expense));
  useEditIntent(intent, 'expenseId', data, startEdit, () => setNotice({ kind: 'error', text: 'That entry is no longer in the books.' }));
  const kindName = (k) => (k === 'CHECK' ? 'Check' : k === 'CARD_CREDIT' ? 'Card credit' : 'Expense');
  useWindowContent(
    editing
      ? { name: `${kindName(editing.expense.kind)}${editing.expense.checkNumber ? ` ${editing.expense.checkNumber}` : ''} · ${editing.expense.payeeName}`, action: 'edit', data: { expenseId: editing.expense.id } }
      : kind
        ? { name: kind === 'CHECK' ? 'Write Checks' : kind === 'CARD_CREDIT' ? 'Enter Credit Card Credits' : 'Enter Credit Card Charges' }
        : null,
  );

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };
  const sorter = useSort(data, {
    date: (x) => x.expenseDate,
    type: (x) => (x.kind === 'CHECK' ? 'Check' : 'Expense'),
    number: (x) => x.checkNumber,
    payee: (x) => x.payeeName,
    category: (x) => (x.categoryAccountId !== null ? accountLabel(accounts, x.categoryAccountId) : null),
    from: (x) => accountLabel(accounts, x.paymentAccountId),
    total: (x) => x.homeTotalCents ?? x.totalCents,
  });
  const rows = data ?? [];

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printing && (
        <PrintPortal>
          <CheckDocument company={company} check={printing} bankName={accountLabel(accounts, printing.paymentAccountId)} />
        </PrintPortal>
      )}
      {kind && (
        <ExpenseForm
          key={editing ? `edit-${editing.expense.id}` : 'new'}
          editing={editing}
          kind={kind}
          setKind={setKind}
          accounts={accounts}
          vendors={vendors.data ?? []}
          expenses={rows}
          onCancel={() => {
            setKind(null);
            setEditing(null);
          }}
          onPosted={(text) => setNotice({ kind: 'success', text })}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load expenses: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Expenses, checks & card credits</h2>
          <span className="text-xs text-qb-ink-muted">Bank feed approvals appear here as expenses and deposits</span>
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setKind('CARD_CREDIT')}>
              Card credit
            </button>
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
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="type">Type</SortTh>
                <SortTh sorter={sorter} col="number">No.</SortTh>
                <SortTh sorter={sorter} col="payee">Payee</SortTh>
                <SortTh sorter={sorter} col="category">Category</SortTh>
                <SortTh sorter={sorter} col="from">Paid from / memo</SortTh>
                <SortTh sorter={sorter} col="total" className="text-right">Total</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No expenses, checks or card credits yet.</td>
                </tr>
              )}
              {sorter.sorted.map((x) => (
                <tr key={x.id} className={`qb-grid-row ${x.isVoided ? 'text-qb-ink-faint' : ''}`}>
                  <td className="qb-grid-cell">{formatDate(x.expenseDate)}</td>
                  <td className="qb-grid-cell">{EXPENSE_KINDS[x.kind] ?? x.kind}</td>
                  <td className="qb-grid-cell">{x.checkNumber ?? ''}</td>
                  <td className="qb-grid-cell font-semibold" title={x.payeeName ?? ''}>{x.payeeName ?? <span className="font-normal text-qb-ink-faint">—</span>}</td>
                  <td className="qb-grid-cell">
                    {x.categoryAccountId !== null ? accountLabel(accounts, x.categoryAccountId) : <span className="text-qb-ink-muted">Split ({x.lineCount} lines)</span>}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={x.memo ?? ''}>
                    {accountLabel(accounts, x.paymentAccountId)}
                    {x.memo ? ` · ${x.memo}` : ''}
                  </td>
                  <td
                    className={`qb-grid-cell num ${x.isVoided ? 'line-through' : ''}`}
                    title={x.currencyCode ? `${formatCents(x.homeTotalCents)} at ${formatRate(x.exchangeRateE6)}` : undefined}
                  >
                    {/* A card credit gives money back, so it shows as a negative amount. */}
                    {formatMoney(x.kind === 'CARD_CREDIT' ? -x.totalCents : x.totalCents, x.currencyCode)}
                  </td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      items={[
                        { label: 'Edit…', disabled: x.isVoided, onSelect: () => openExpense(x) },
                        ...(x.kind === 'CHECK' ? [{ label: 'Print check', disabled: x.isVoided, onSelect: () => printCheck(x) }] : []),
                        voidItem({
                          journalEntryId: x.journalEntryId,
                          voided: x.isVoided,
                          description: `${x.kind === 'CHECK' ? `Check${x.checkNumber ? ` #${x.checkNumber}` : ''}` : EXPENSE_KINDS[x.kind]} of ${formatMoney(x.totalCents, x.currencyCode)}${x.payeeName ? ` ${x.kind === 'CARD_CREDIT' ? 'from' : 'to'} ${x.payeeName}` : ''}. If it came from the bank feed, the bank line returns to "For review".`,
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
