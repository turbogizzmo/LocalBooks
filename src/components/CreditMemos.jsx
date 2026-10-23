import { useEffect, useMemo, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { taxOn, useFormTax } from './SalesTax.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const isMoneyAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';

/** Credit memo (credit kept for the customer) or refund receipt (money paid back now). */
function CreditForm({ accounts, refund, onPosted, onCancel }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  const defaultBank = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const [customer, setCustomer] = useState('');
  const [date, setDate] = useState(todayIso());
  const [memo, setMemo] = useState('');
  const [refundFrom, setRefundFrom] = useState(defaultBank);
  const [lines, setLines] = useState(() => Array.from({ length: 3 }, () => blankLine(defaultIncome)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);

  useEffect(() => {
    if (defaultIncome !== null) setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);
  useEffect(() => {
    if (refundFrom === null && defaultBank !== null) setRefundFrom(defaultBank);
  }, [defaultBank, refundFrom]);

  const { filled, invalidCount, total: subtotal, payload } = summarizeLines(lines);
  const taxCents = taxOn(payload, tax.rate, (l) => l.amountCents);
  const total = subtotal + taxCents;

  const save = async () => {
    if (!customer.trim()) return setError('Enter a customer.');
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a quantity and a rate above zero.`);
    if (refund && refundFrom === null) return setError('Choose the account the refund is paid from.');
    const postedLines = formClass.apply(payload);
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const memoRow = await api.createCreditMemo({
        customerName: customer.trim(),
        creditDate: date,
        memo: memo.trim() || null,
        refundAccountId: refund ? refundFrom : null,
        lines: postedLines,
        taxRateId: tax.rateId,
      });
      onPosted(
        refund
          ? `Refund receipt #${memoRow.creditNumber} posted: Dr income / Cr ${accountLabel(accounts, refundFrom)} ${formatCents(memoRow.totalCents)}.`
          : `Credit memo #${memoRow.creditNumber} for ${memoRow.customerName} posted: ${formatCents(memoRow.totalCents)} of credit to apply to invoices or refund.`,
      );
      setCustomer('');
      setMemo('');
      setLines(Array.from({ length: 3 }, () => blankLine(defaultIncome)));
      formClass.reset();
      tax.reset();
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
        <h2 className="mr-2 self-center text-md font-semibold">{refund ? 'Refund receipt' : 'Credit memo'}</h2>
        <label className="w-[220px]">
          <span className="qb-label">Customer</span>
          <input autoFocus className="qb-input w-full" value={customer} onChange={(e) => setCustomer(e.target.value)} list="credit-customers" placeholder="Customer name" />
          <datalist id="credit-customers">
            {(customers.data ?? []).filter((c) => c.isActive).map((c) => (
              <option key={c.id} value={c.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">Date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
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
      <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={(products.data ?? []).filter((p) => p.qtyOnHandE4 == null)} defaultAccountId={defaultIncome} withTax={tax.enabled} />
      <div className="flex items-center gap-2 px-2 py-1">
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
          Add line
        </button>
        <span className="text-xs text-qb-ink-muted">Stock items are returned with Products › Adjust stock; credit them here as a service line.</span>
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
            {busy ? 'Posting…' : 'Save & Post'}
          </button>
        </div>
      </div>
    </section>
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
      onDone(`Applied ${formatCents(total)} of credit memo #${memo.creditNumber}.`);
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
    <Dialog title={`Apply credit memo #${memo.creditNumber} · ${memo.customerName}`} onClose={onClose}>
      <div className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="flex items-end gap-3">
          <span className="text-sm">
            Credit left <span className="num font-semibold">{formatCents(memo.remainingCents)}</span>
          </span>
          <label className="ml-auto flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date applied</span>
            <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
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
                  <input className="num" value={amounts[inv.id] ?? ''} onChange={(e) => setAmounts((a) => ({ ...a, [inv.id]: e.target.value }))} inputMode="decimal" aria-label={`Apply to invoice ${inv.invoiceNumber}`} />
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

/** Pays credit left over back to the customer (or, for a vendor credit, records the vendor's refund). */
export function RefundDialog({ title, remainingCents, accounts, direction, onRefund, onClose }) {
  const [accountId, setAccountId] = useState(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null);
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
            <span className="font-semibold text-qb-ink-muted">{direction === 'out' ? 'Pay from' : 'Deposit to'}</span>
            <AccountSelect accounts={accounts} filter={isMoneyAccount} value={accountId} onChange={setAccountId} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date</span>
            <input type="date" className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Amount</span>
            <input className="qb-input num" value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Reference (check no.)</span>
            <input className="qb-input" value={reference} onChange={(e) => setReference(e.target.value)} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Record refund</button>
        </div>
      </form>
    </Dialog>
  );
}

export default function CreditMemos({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listCreditMemos);
  const [composing, setComposing] = useState(intent?.action === 'credit-memo' ? 'credit' : intent?.action === 'refund-receipt' ? 'refund' : null);
  const [dialog, setDialog] = useState(null); // { mode: 'apply' | 'refund', memo }
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (intent?.action === 'credit-memo') setComposing('credit');
    if (intent?.action === 'refund-receipt') setComposing('refund');
  }, [intent]);

  const done = (text) => {
    setDialog(null);
    setNotice({ kind: 'success', text });
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {composing && (
        <CreditForm key={composing} accounts={accounts} refund={composing === 'refund'} onCancel={() => setComposing(null)} onPosted={(text) => setNotice({ kind: 'success', text })} />
      )}
      {dialog?.mode === 'apply' && <ApplyDialog memo={dialog.memo} onClose={() => setDialog(null)} onDone={done} />}
      {dialog?.mode === 'refund' && (
        <RefundDialog
          title={`Refund credit memo #${dialog.memo.creditNumber} · ${dialog.memo.customerName}`}
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
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Credit memos &amp; refunds</h2>
          <span className="text-xs text-qb-ink-muted">A credit memo keeps credit for the customer to apply to invoices; a refund receipt pays the money back now.</span>
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
                <th className="qb-grid-head">No.</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Type</th>
                <th className="qb-grid-head text-right">Total</th>
                <th className="qb-grid-head text-right">Credit left</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && data.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No credit memos or refund receipts yet.</td>
                </tr>
              )}
              {(data ?? []).map((m) => {
                const refund = m.refundAccountId !== null;
                const status = m.isVoided ? 'Voided' : refund ? 'Refunded' : m.remainingCents === 0 ? 'Used' : m.remainingCents < m.totalCents ? 'Partly used' : 'Open';
                return (
                  <tr key={m.id} className={`qb-grid-row ${m.isVoided ? 'text-qb-ink-faint' : ''}`}>
                    <td className="qb-grid-cell">{m.creditNumber}</td>
                    <td className="qb-grid-cell">{formatDate(m.creditDate)}</td>
                    <td className="qb-grid-cell font-semibold">{m.customerName}</td>
                    <td className="qb-grid-cell">{refund ? 'Refund receipt' : 'Credit memo'}</td>
                    <td className="qb-grid-cell num">{formatCents(m.totalCents)}</td>
                    <td className="qb-grid-cell num font-semibold">{refund || m.isVoided ? '' : formatCents(m.remainingCents)}</td>
                    <td className="qb-grid-cell text-sm">{status}</td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for credit ${m.creditNumber}`}
                        items={[
                          { label: 'Apply to invoices…', disabled: refund || m.isVoided, onSelect: () => setDialog({ mode: 'apply', memo: m }) },
                          { label: 'Refund credit…', disabled: refund || m.isVoided || m.remainingCents === 0, onSelect: () => setDialog({ mode: 'refund', memo: m }) },
                          voidItem({
                            journalEntryId: m.journalEntryId,
                            voided: m.isVoided,
                            description: `${refund ? 'Refund receipt' : 'Credit memo'} #${m.creditNumber} for ${m.customerName} (${formatCents(m.totalCents)}).`,
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
