import { useEffect, useMemo, useState } from 'react';
import { rowKeys } from '../lib/rowKeys.js';
import { useTransactionDate } from '../lib/myPreferences.js';
import { layoutMessage } from '../lib/itemTypes.js';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import LineItemGrid, { blankLine, linesFromDocument, summarizeLines } from './LineItemGrid.jsx';
import Notice from './Notice.jsx';
import FormToolbar, { attachLaterButton, fileButtons, navButtons } from './FormToolbar.jsx';
import { attachPicked, PendingAttachments } from './Attachments.jsx';
import { useDirtySince, useOpenFromList } from '../lib/windows.js';
import SavedDocView, { useSavedDoc } from './SavedDocView.jsx';
import { byDateThenNumber } from '../lib/formStep.js';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { documentTotals, useFormTax } from './SalesTax.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import NameField from './NameField.jsx';
import DocNumberBox, { useDocNumber } from './DocNumberBox.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { FormInfoPanel } from './InfoPanel.jsx';

const isMoneyAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
const isIncomeAccount = (a) => a.isActive && (a.accountType === 'INCOME' || a.accountType === 'OTHER_INCOME');
// An unapplied payment (an overpayment or prepayment kept as credit) is listed with credit memos.
const kindOf = (m) => (m.isPayment ? 'Unapplied payment' : m.refundAccountId !== null ? 'Refund receipt' : 'Credit memo');
const nameOf = (m) => (m.isPayment ? `unapplied payment${m.paymentReference ? ` (${m.paymentReference})` : ''}` : `credit memo #${m.creditNumber}`);

/** Credit memo (credit kept for the customer) or refund receipt (money paid back now). */
/** `copyOf` (from Duplicate) starts the credit memo or refund receipt as a copy of a saved one, dated today. */
function CreditForm({ accounts, refund, onPosted, onCancel, savedStep, copyOf = null }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  const defaultBank = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const [customer, setCustomer] = useState(copyOf?.name ?? '');
  const [date, setDate] = useTransactionDate();
  const [memo, setMemo] = useState(copyOf?.memo ?? '');
  const [refundFrom, setRefundFrom] = useState(copyOf?.accountId ?? defaultBank);
  const [lines, setLines] = useState(() => (copyOf ? linesFromDocument(copyOf.lines, copyOf.formLines ?? [], defaultIncome) : Array.from({ length: 3 }, () => blankLine(defaultIncome))));
  const [busy, setBusy] = useState(false);
  // Credit #: the next number, shown before saving; the person may type another.
  const docNumber = useDocNumber('CREDIT_MEMO', 'Credit');
  const [error, setError] = useState(null);
  // Files picked before posting; attached once it posts.
  const [files, setFiles] = useState([]);
  const openFromList = useOpenFromList();
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);
  // Only its Bill To / Ship To / P.O. No. / FOB row is used here.
  const sales = useSalesDetails(customer, customers.data);
  // A copy keeps the original's tax rate.
  useEffect(() => {
    if (copyOf) tax.set(copyOf.taxRateId ?? null);
    // Once, for the copy the form was opened with.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (defaultIncome !== null) setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);
  useEffect(() => {
    if (refundFrom === null && defaultBank !== null) setRefundFrom(defaultBank);
  }, [defaultBank, refundFrom]);

  // Subtotals, discounts, percent charges and groups lay out as they do on an invoice.
  const { filled, invalidCount, payload, formLines, layoutError } = summarizeLines(lines);
  const { tax: taxCents, total } = documentTotals(payload, tax.rate, 0, formLines);

  const clear = () => {
    docNumber.reset();
    setCustomer('');
    setMemo('');
    setLines(Array.from({ length: 3 }, () => blankLine(defaultIncome)));
    setFiles([]);
    formClass.reset();
    tax.reset();
    sales.reset();
    setError(null);
  };

  const save = async () => {
    if (!customer.trim()) return setError('Enter a customer.');
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a quantity and a rate above zero.`);
    if (layoutError) return setError(layoutMessage(layoutError));
    if (total <= 0) return setError('The credit must be more than zero after discounts.');
    if (refund && refundFrom === null) return setError('Choose the account the refund is paid from.');
    const postedLines = formClass.apply(payload);
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const chosen = await docNumber.resolve();
      if (chosen.cancelled) return;
      if (chosen.error) return setError(chosen.error);
      const memoRow = await api.createCreditMemo({
        creditNumber: chosen.value,
        customerName: customer.trim(),
        creditDate: date,
        memo: memo.trim() || null,
        refundAccountId: refund ? refundFrom : null,
        lines: postedLines,
        formLines,
        taxRateId: tax.rateId,
        ...(({ billTo, shipTo, poNumber, fob }) => ({ billTo, shipTo, poNumber, fob }))(sales.payload()),
      });
      const attachError = files.length > 0 ? await attachPicked(memoRow.journalEntryId, files) : null;
      onPosted(
        (refund
          ? `Refund receipt #${memoRow.creditNumber} posted: Dr income / Cr ${accountLabel(accounts, refundFrom)} ${formatCents(memoRow.totalCents)}.`
          : `Credit memo #${memoRow.creditNumber} for ${memoRow.customerName} posted: ${formatCents(memoRow.totalCents)} of credit to apply to invoices or refund.`) +
          (attachError ? ` ${attachError}` : ''),
      );
      clear();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save, clear, onClose: onCancel, busy });
  // Opening a saved one (Previous) asks for the entries to be saved or cleared first.
  const dirty = useDirtySince(refund ? 'refund-receipt' : 'credit-memo', { customer, memo, lines: lines.map((l) => [l.productId, l.description, l.qty, l.rate]) });

  return (
    <div className="flex flex-none gap-2">
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel min-w-0 flex-1">
      <FormToolbar
        label={refund ? 'Refund receipt' : 'Credit memo'}
        groups={[
          navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...savedStep(busy || dirty) }),
          fileButtons({ onNew: clear, onSave: flow.saveAndNew, busy }),
          [attachLaterButton({ setFiles, busy, what: refund ? 'this refund' : 'this credit memo' })],
        ]}
      />
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">{refund ? 'Refund receipt' : 'Credit memo'}</h2>
        <label className="w-[220px]">
          <span className="qb-label">Customer</span>
          <NameField kind="customer" autoFocus className="qb-input w-full" names={customers.data ?? []} value={customer} onChange={setCustomer} placeholder="Customer name" />
        </label>
        <label>
          <span className="qb-label">Date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <DocNumberBox number={docNumber} />
        {refund && (
          <label>
            <span className="qb-label">Refund from</span>
            <AccountSelect accounts={accounts} filter={isMoneyAccount} value={refundFrom} onChange={setRefundFrom} className="qb-input w-[200px]" />
          </label>
        )}
        <label>
          <span className="qb-label">Memo</span>
          <input className="qb-input w-[220px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Reason for the credit" />
        </label>
        {formClass.header}
        {tax.picker}
        <div className="ml-auto text-right">
          <div className="qb-label">{refund ? 'Refunded' : 'Credit'}</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
          {taxCents > 0 && <div className="text-xs text-qb-ink-muted">incl. {formatCents(taxCents)} sales tax</div>}
        </div>
      </div>
      {sales.addresses}
      <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={(products.data ?? []).filter((p) => p.qtyOnHandE4 == null)} defaultAccountId={defaultIncome} withTax={tax.enabled} customer={customer} customers={customers.data} itemTypes />
      <div className="flex items-center gap-2 px-2 py-1">
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
          Add line
        </button>
        {files.length > 0 && <PendingAttachments paths={files} onChange={setFiles} label="Attach another…" />}
        <span className="text-xs text-qb-ink-muted">Stock items are returned with Products › Adjust stock; credit them here as a service line.</span>
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <SaveButtons flow={flow} busyLabel="Posting…" />
        </div>
      </div>
    </section>
    {/* The chosen customer's information panel (T56); it can be hidden. */}
    <FormInfoPanel kind="customer" name={customer} names={customers.data} />
    </div>
  );
}

/** Applies a credit memo's remaining credit to the customer's open invoices, and lists what is applied. */
function ApplyDialog({ memo, onClose, onDone }) {
  const invoices = useLedgerQuery(api.listInvoices);
  const applied = useLedgerQuery(() => api.listCustomerCreditApplications(memo.id));
  const open = (invoices.data ?? []).filter((i) => i.customerId === memo.customerId && !i.isVoided && (i.status === 'UNPAID' || i.status === 'PARTIAL') && !i.currencyCode);
  const [amounts, setAmounts] = useState({});
  const [date, setDate] = useState(todayIso());
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  // Suggest filling the oldest invoices first with the credit available.
  useEffect(() => {
    if (!invoices.data) return;
    let left = memo.remainingCents;
    const next = {};
    for (const inv of [...open].sort((a, b) => a.dueDate.localeCompare(b.dueDate))) {
      const take = Math.min(left, inv.balanceCents);
      if (take > 0) next[inv.id] = centsToInput(take);
      left -= take;
    }
    setAmounts(next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [invoices.data, memo.remainingCents]);

  const parsed = Object.entries(amounts)
    .map(([id, text]) => [Number(id), text.trim() === '' ? 0 : parseMoneyToCents(text)])
    .filter(([, cents]) => cents !== 0);
  const total = parsed.reduce((s, [, c]) => s + (c ?? 0), 0);

  const apply = async () => {
    if (parsed.some(([, c]) => c === null || c < 0)) return setError('Enter valid amounts.');
    if (total > memo.remainingCents) return setError(`Only ${formatCents(memo.remainingCents)} of credit is left.`);
    setBusy(true);
    try {
      for (const [invoiceId, cents] of parsed) {
        await api.applyCustomerCredit({ creditId: memo.id, documentId: invoiceId, amountCents: cents, appliedDate: date });
      }
      onDone(`Applied ${formatCents(total)} of ${nameOf(memo)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const remove = async (application) => {
    try {
      await api.removeCustomerCredit(application.id);
      applied.reload();
      invoices.reload();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Dialog title={`Apply ${nameOf(memo)} · ${memo.customerName}`} onClose={onClose}>
      <div className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="flex items-end gap-3">
          <span className="text-sm">
            Credit left <span className="num font-semibold">{formatCents(memo.remainingCents)}</span>
          </span>
          <label className="ml-auto flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date applied</span>
            <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
        </div>
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Invoice</th>
              <th className="qb-grid-head">Due</th>
              <th className="qb-grid-head text-right">Open balance</th>
              <th className="qb-grid-head text-right">Apply</th>
            </tr>
          </thead>
          <tbody>
            {open.length === 0 && (
              <tr>
                <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">{invoices.data ? `${memo.customerName} has no open home-currency invoices.` : 'Loading…'}</td>
              </tr>
            )}
            {open.map((inv) => (
              <tr key={inv.id} className="qb-grid-row">
                <td className="qb-grid-cell">#{inv.invoiceNumber}</td>
                <td className="qb-grid-cell">{formatDate(inv.dueDate)}</td>
                <td className="qb-grid-cell num">{formatCents(inv.balanceCents)}</td>
                <td className="qb-grid-cell-edit w-[120px]">
                  <input className="num" value={amounts[inv.id] ?? ''} onChange={(e) => setAmounts((a) => ({ ...a, [inv.id]: e.target.value }))} inputMode="decimal" data-money aria-label={`Apply to invoice ${inv.invoiceNumber}`} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {(applied.data ?? []).length > 0 && (
          <div>
            <div className="mb-0.5 font-semibold text-qb-ink-muted">Already applied</div>
            {(applied.data ?? []).map((a) => (
              <div key={a.id} className="flex items-center gap-2 border-b border-qb-border py-0.5">
                <span>{a.documentLabel}</span>
                <span className="text-qb-ink-muted">{formatDate(a.appliedDate)}</span>
                <span className="num ml-auto font-semibold">{formatCents(a.amountCents)}</span>
                <button type="button" className="text-qb-info hover:underline" onClick={() => remove(a)}>
                  Remove
                </button>
              </div>
            ))}
          </div>
        )}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Close</button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={apply} disabled={busy || total === 0}>
            Apply {total > 0 ? formatCents(total) : ''}
          </button>
        </div>
      </div>
    </Dialog>
  );
}

/**
 * Pays credit left over back to the customer (or, for a vendor credit, records the vendor's refund).
 * With `writeOff` it writes the credit off to an income account instead.
 */
export function RefundDialog({ title, remainingCents, accounts, direction, onRefund, onClose, writeOff = false }) {
  // The first account the box offers: a bank or card account for a refund, income for a write-off.
  const [accountId, setAccountId] = useState(() => accounts.find((a) => a.isActive && (writeOff ? isIncomeAccount(a) : isMoneyAccount(a)))?.id ?? null);
  const [date, setDate] = useState(todayIso());
  const [amount, setAmount] = useState(centsToInput(remainingCents));
  const [reference, setReference] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const cents = parseMoneyToCents(amount);

  const submit = async (e) => {
    e.preventDefault();
    if (accountId === null) return setError('Choose an account.');
    if (cents === null || cents <= 0) return setError('Enter the amount.');
    setBusy(true);
    try {
      await onRefund({ accountId, refundDate: date, amountCents: cents, reference: reference.trim() || null });
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={title} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <p className="text-qb-ink-muted">Credit left: {formatCents(remainingCents)}.</p>
        <div className="grid grid-cols-2 gap-2">
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{writeOff ? 'Write off to (income)' : direction === 'out' ? 'Pay from' : 'Deposit to'}</span>
            <AccountSelect accounts={accounts} filter={writeOff ? isIncomeAccount : isMoneyAccount} value={accountId} onChange={setAccountId} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date</span>
            <DateBox className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Amount</span>
            <input className="qb-input num" value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" data-money />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{writeOff ? 'Reference' : 'Reference (check no.)'}</span>
            <input className="qb-input" value={reference} onChange={(e) => setReference(e.target.value)} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>{writeOff ? 'Write off' : 'Record refund'}</button>
        </div>
      </form>
    </Dialog>
  );
}

export default function CreditMemos({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listCreditMemos);
  const sorter = useSort(data, {
    number: (m) => m.creditNumber,
    date: (m) => m.creditDate,
    customer: (m) => m.customerName,
    type: kindOf,
    total: (m) => m.totalCents,
    left: (m) => (m.refundAccountId !== null || m.isVoided ? null : m.remainingCents),
    status: (m) => (m.isVoided ? 'Voided' : m.refundAccountId !== null ? 'Refunded' : m.remainingCents === 0 ? 'Used' : m.remainingCents < m.totalCents ? 'Partly used' : 'Open'),
  }, { storageKey: 'credit-memos' });
  const [composing, setComposing] = useState(intent?.action === 'credit-memo' ? 'credit' : intent?.action === 'refund-receipt' ? 'refund' : null);
  const [dialog, setDialog] = useState(null); // { mode: 'apply' | 'refund' | 'writeOff', memo }
  const [notice, setNotice] = useState(null);
  // Duplicate: the saved credit memo or refund receipt the new one starts from.
  const [copyOf, setCopyOf] = useState(null);
  const [formKey, setFormKey] = useState(0);

  useEffect(() => {
    if (intent?.action === 'credit-memo') setComposing('credit');
    if (intent?.action === 'refund-receipt') setComposing('refund');
  }, [intent]);

  const done = (text) => {
    setDialog(null);
    setNotice({ kind: 'success', text });
  };
  // Saved credit memos (or refund receipts, on that form) open read-only in the form's place.
  const ordered = useMemo(
    () => byDateThenNumber((data ?? []).filter((m) => !m.isPayment && (m.refundAccountId !== null) === (composing === 'refund')), (m) => m.creditDate, (m) => m.creditNumber),
    [data, composing],
  );
  const saved = useSavedDoc(ordered);
  const viewing = saved.current;
  const openFromList = useOpenFromList();
  const openMemo = (m) => {
    setComposing(m.refundAccountId !== null ? 'refund' : 'credit');
    saved.open(m);
  };
  const duplicate = (m) =>
    api.getCopySource('CREDIT_MEMO', m.id).then(
      (copy) => {
        saved.close();
        setComposing(m.refundAccountId !== null ? 'refund' : 'credit');
        setCopyOf(copy);
        setFormKey((k) => k + 1);
        setNotice({ kind: 'info', text: `A copy of ${m.refundAccountId !== null ? 'refund receipt' : 'credit memo'} #${m.creditNumber}, dated today: it gets the next number when you save it.` });
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const viewingRefund = viewing?.refundAccountId !== null;
  const viewingLabel = viewingRefund ? 'Refund receipt' : 'Credit memo';

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {composing && viewing && (
        <SavedDocView
          label={viewingLabel}
          title={`${viewingLabel} #${viewing.creditNumber} · ${viewing.customerName}${viewing.isVoided ? ' (voided)' : ''}`}
          step={saved.step}
          onFind={() => openFromList('accounting/find', 'find', { text: '' })}
          onNew={saved.close}
          onDuplicate={() => duplicate(viewing)}
          entryId={viewing.journalEntryId}
          // Drawn, printed and emailed as its credit memo form (Settings › Custom Forms › Credit Memo).
          form={{ kind: 'CREDIT_MEMO', id: viewing.id }}
          notes={[!viewingRefund && !viewing.isVoided && { text: `${formatCents(viewing.remainingCents)} of ${formatCents(viewing.totalCents)} is left to apply.` }]}
          onNotice={setNotice}
          void={{
            onVoid: async () => {
              await api.voidTransaction(viewing.journalEntryId);
              setNotice({ kind: 'success', text: `Voided #${viewing.creditNumber}.` });
            },
            disabled: viewing.isVoided || !viewing.journalEntryId,
            confirm: {
              title: 'Void this transaction?',
              body: `${viewingLabel} #${viewing.creditNumber} for ${viewing.customerName} (${formatCents(viewing.totalCents)}). A reversing journal entry dated the same day will zero out its effect. The original stays in the books and the audit log. This cannot be undone.`,
              confirmLabel: 'Yes, void it',
            },
          }}
          extra={
            viewingRefund
              ? []
              : [
                  [
                    { id: 'apply', label: 'Apply', icon: 'ReceivePayment', hint: 'Apply the credit to open invoices', disabled: viewing.isVoided || viewing.remainingCents === 0, onClick: () => setDialog({ mode: 'apply', memo: viewing }) },
                    { id: 'refund', label: 'Refund', icon: 'HomeRefund', hint: 'Pay the credit back to the customer', disabled: viewing.isVoided || viewing.remainingCents === 0, onClick: () => setDialog({ mode: 'refund', memo: viewing }) },
                    { id: 'write-off', label: 'Write Off', icon: 'Void', hint: 'Keep the credit left as income', disabled: viewing.isVoided || viewing.remainingCents === 0, onClick: () => setDialog({ mode: 'writeOff', memo: viewing }) },
                  ],
                ]
          }
        />
      )}
      {composing && !viewing && (
        <CreditForm
          key={`${composing}-${formKey}`}
          copyOf={copyOf}
          accounts={accounts}
          refund={composing === 'refund'}
          savedStep={(blocked) => saved.fromNew(blocked)}
          onCancel={() => {
            setCopyOf(null);
            setComposing(null);
          }}
          onPosted={(text) => {
            setCopyOf(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {dialog?.mode === 'apply' && <ApplyDialog memo={dialog.memo} onClose={() => setDialog(null)} onDone={done} />}
      {dialog?.mode === 'refund' && (
        <RefundDialog
          title={`Refund ${nameOf(dialog.memo)} · ${dialog.memo.customerName}`}
          remainingCents={dialog.memo.remainingCents}
          accounts={accounts}
          direction="out"
          onClose={() => setDialog(null)}
          onRefund={async (refund) => {
            await api.refundCustomerCredit({ creditId: dialog.memo.id, ...refund });
            done(`Refunded ${formatCents(refund.amountCents)} to ${dialog.memo.customerName}: Dr Accounts Receivable / Cr ${accountLabel(accounts, refund.accountId)}.`);
          }}
        />
      )}
      {dialog?.mode === 'writeOff' && (
        <RefundDialog
          writeOff
          title={`Write off ${nameOf(dialog.memo)} · ${dialog.memo.customerName}`}
          remainingCents={dialog.memo.remainingCents}
          accounts={accounts}
          direction="out"
          onClose={() => setDialog(null)}
          onRefund={async (writeOff) => {
            await api.writeOffCustomerCredit({ creditId: dialog.memo.id, ...writeOff });
            done(`Wrote off ${formatCents(writeOff.amountCents)} of ${dialog.memo.customerName}'s credit: Dr Accounts Receivable / Cr ${accountLabel(accounts, writeOff.accountId)}.`);
          }}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Credit memos &amp; refunds</h2>
          <span className="text-xs text-qb-ink-muted">A credit memo keeps credit for the customer to apply to invoices; a refund receipt pays the money back now. Money received but not applied to an invoice is listed as an unapplied payment.</span>
          <div className="ml-auto flex gap-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setComposing('refund')}>
              New refund receipt
            </button>
            <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setComposing('credit')}>
              New credit memo
            </button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[64px]" />
              <col className="w-[92px]" />
              <col />
              <col className="w-[130px]" />
              <col className="w-[120px]" />
              <col className="w-[120px]" />
              <col className="w-[110px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="number">No.</SortTh>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="customer">Customer</SortTh>
                <SortTh sorter={sorter} col="type">Type</SortTh>
                <SortTh sorter={sorter} col="total" className="text-right">Total</SortTh>
                <SortTh sorter={sorter} col="left" className="text-right">Credit left</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && data.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No credit memos or refund receipts yet.</td>
                </tr>
              )}
              {sorter.sorted.map((m) => {
                const refund = m.refundAccountId !== null;
                const status = m.isVoided ? 'Voided' : refund ? 'Refunded' : m.remainingCents === 0 ? 'Used' : m.remainingCents < m.totalCents ? 'Partly used' : 'Open';
                return (
                  <tr key={m.id} className={`qb-grid-row ${m.isPayment ? '' : 'cursor-pointer'} ${m.isVoided ? 'text-qb-ink-faint' : ''}`} {...rowKeys({ first: m === sorter.sorted[0], open: () => !m.isPayment && openMemo(m) })}>
                    <td className="qb-grid-cell">{m.isPayment ? m.paymentReference ?? '' : m.creditNumber}</td>
                    <td className="qb-grid-cell">{formatDate(m.creditDate)}</td>
                    <td className="qb-grid-cell font-semibold">{m.customerName}</td>
                    <td className="qb-grid-cell">{kindOf(m)}</td>
                    <td className="qb-grid-cell num">{formatCents(m.totalCents)}</td>
                    <td className="qb-grid-cell num font-semibold">{refund || m.isVoided ? '' : formatCents(m.remainingCents)}</td>
                    <td className="qb-grid-cell text-sm">{status}</td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for credit ${m.creditNumber}`}
                        items={[
                          { label: 'Open', disabled: m.isPayment, onSelect: () => openMemo(m) },
                          { label: 'Duplicate', hint: 'A new one copied from this one, dated today', disabled: m.isPayment, onSelect: () => duplicate(m) },
                          { label: 'Apply to invoices…', disabled: refund || m.isVoided, onSelect: () => setDialog({ mode: 'apply', memo: m }) },
                          { label: 'Refund credit…', disabled: refund || m.isVoided || m.remainingCents === 0, onSelect: () => setDialog({ mode: 'refund', memo: m }) },
                          { label: 'Write off credit…', disabled: refund || m.isVoided || m.remainingCents === 0, onSelect: () => setDialog({ mode: 'writeOff', memo: m }) },
                          voidItem({
                            journalEntryId: m.journalEntryId,
                            voided: m.isVoided,
                            description: `${kindOf(m)}${m.isPayment ? '' : ` #${m.creditNumber}`} for ${m.customerName} (${formatCents(m.totalCents)}).`,
                            onVoid: async (id) => {
                              await api.voidTransaction(id);
                              setNotice({ kind: 'success', text: `Voided #${m.creditNumber}.` });
                            },
                          }),
                        ]}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
