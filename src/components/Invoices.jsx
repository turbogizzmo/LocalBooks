import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { DiscountField, documentTotals, parseDiscount, useFormTax } from './SalesTax.jsx';
import Notice from './Notice.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { EmailMarks, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { api, errorText, PAYMENT_METHODS, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatMoney, formatQuantity, formatRate, parseMoneyToCents, parseRateE6, toHomeCents } from '../lib/money.js';
import { foreignPaymentEffect } from '../lib/fx.js';
import CurrencyFields from './CurrencyFields.jsx';
import { useSuggestedRate } from './RateSuggestion.jsx';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const TERMS = [
  { id: 'receipt', label: 'Due on receipt', days: 0 },
  { id: 'net15', label: 'Net 15', days: 15 },
  { id: 'net30', label: 'Net 30', days: 30 },
  { id: 'net60', label: 'Net 60', days: 60 },
];
const INITIAL_ROWS = 4;

export const depositFilter = (a) => a.accountType === 'BANK' || (a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');

export function StatusBadge({ status, overdue, voided }) {
  if (voided) return <span className="font-semibold text-qb-ink-faint line-through decoration-1">Voided</span>;
  if (status === 'PAID') return <span className="font-semibold text-qb-green-active">● Paid</span>;
  if (overdue) return <span className="font-semibold text-qb-negative">● Overdue</span>;
  if (status === 'PARTIAL') return <span className="font-semibold text-qb-info">◐ Partial</span>;
  return <span className="text-qb-ink">○ Open</span>;
}

function PaymentRow({ invoice, accounts, onDone, colSpan }) {
  const undeposited = accounts.find((a) => a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');
  const [accountId, setAccountId] = useState(undeposited?.id ?? null);
  const [date, setDate] = useState(todayIso());
  const [amount, setAmount] = useState(centsToInput(invoice.balanceCents));
  const foreign = invoice.currencyCode;
  const money = (c) => formatMoney(c, foreign);
  const [rate, setRate] = useState(foreign ? formatRate(invoice.exchangeRateE6) : '');
  const rateE6 = foreign ? parseRateE6(rate) : null;
  // The invoice's rate is only a starting point: with lookup on, the payment date's rate replaces it.
  const suggested = useSuggestedRate({ currency: foreign, date, rate, setRate, initialIsDefault: true });
  const [method, setMethod] = useState('CHECK');
  const [reference, setReference] = useState('');
  const [busy, setBusy] = useState(false);

  const cents = parseMoneyToCents(amount);
  const amountError = cents === null || cents <= 0 ? 'Enter an amount' : cents > invoice.balanceCents ? 'More than the open balance' : null;
  const remaining = cents !== null && !amountError ? invoice.balanceCents - cents : null;
  const effect = foreign && !amountError ? foreignPaymentEffect(invoice, cents, rateE6) : null;
  const rateError = foreign && rateE6 === null ? 'Enter the exchange rate' : null;

  const record = async () => {
    if (accountId === null || amountError || rateError) return;
    setBusy(true);
    try {
      const updated = await api.recordInvoicePayment({
        invoiceId: invoice.id,
        depositAccountId: accountId,
        paymentDate: date,
        amountCents: cents,
        paymentMethod: method,
        reference: reference || null,
        exchangeRateE6: foreign ? rateE6 : null,
      });
      onDone({
        kind: 'success',
        text:
          updated.status === 'PAID'
            ? `Invoice #${updated.invoiceNumber} is paid in full.`
            : `Recorded ${money(cents)} on invoice #${updated.invoiceNumber}; ${money(updated.balanceCents)} remains open.`,
      });
    } catch (err) {
      onDone({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <tr className="bg-qb-row-selected">
      <td colSpan={colSpan} className="border-b border-qb-border px-2 py-1">
        <div className="flex flex-wrap items-end gap-2">
          <span className="self-center text-sm font-semibold">Receive payment · #{invoice.invoiceNumber}</span>
          <label>
            <span className="qb-label">Amount</span>
            <input
              className="qb-input num w-[110px]"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              aria-invalid={Boolean(amountError)}
              inputMode="decimal"
              autoFocus
            />
          </label>
          {foreign && (
            <label>
              <span className="qb-label">1 {foreign} = ? {invoice.homeCurrency}</span>
              <span className="flex items-center gap-1.5">
                <input className="qb-input num w-[96px]" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" aria-invalid={Boolean(rateError)} />
                {suggested.note}
              </span>
            </label>
          )}
          <label>
            <span className="qb-label">Method</span>
            <select className="qb-input w-[140px]" value={method} onChange={(e) => setMethod(e.target.value)}>
              {PAYMENT_METHODS.map((m) => (
                <option key={m.id} value={m.id}>{m.label}</option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">Reference no.</span>
            <input className="qb-input w-[100px]" value={reference} onChange={(e) => setReference(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">Deposit to</span>
            <AccountSelect accounts={accounts} filter={depositFilter} value={accountId} onChange={setAccountId} className="qb-input w-[190px]" />
          </label>
          <label>
            <span className="qb-label">Payment date</span>
            <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <div className="ml-auto flex items-center gap-3 self-center text-sm">
            {amountError || rateError ? (
              <span className="text-qb-negative">{amountError || rateError}</span>
            ) : (
              <span className="text-qb-ink-muted">
                Remaining <span className="num font-semibold text-qb-ink">{money(remaining)}</span>
                {effect && (
                  <>
                    {' · '}received <span className="num font-semibold text-qb-ink">{formatCents(effect.homeReceived)}</span>
                    {effect.gain !== 0 && (
                      <span className={effect.gain > 0 ? 'text-qb-green-active' : 'text-qb-negative'}>
                        {' · '}
                        {effect.gain > 0 ? 'exchange gain' : 'exchange loss'} {formatCents(Math.abs(effect.gain))}
                      </span>
                    )}
                  </>
                )}
              </span>
            )}
            <button type="button" className="qb-btn qb-btn-primary" disabled={busy || accountId === null || Boolean(amountError) || Boolean(rateError) || !date} onClick={record}>
              {busy ? 'Recording…' : 'Record payment'}
            </button>
          </div>
        </div>
      </td>
    </tr>
  );
}

/** Receive Payment: one customer payment applied across that customer's open invoices, oldest first. */
function ReceivePaymentPanel({ invoices, accounts, onDone, onCancel }) {
  const { homeCurrency } = useCompany();
  const open = invoices.filter((i) => !i.isVoided && (i.status === 'UNPAID' || i.status === 'PARTIAL'));
  const customers = [...new Set(open.map((i) => i.customerName))].sort((a, b) => a.localeCompare(b));
  const undeposited = accounts.find((a) => a.isSystem && a.accountType === 'OTHER_CURRENT_ASSET');
  const [customer, setCustomer] = useState('');
  // One payment is in one currency; the customer's open invoices are filtered to it.
  const [currency, setCurrency] = useState('');
  const [rate, setRate] = useState('');
  const [received, setReceived] = useState('');
  const [applied, setApplied] = useState({});
  const [accountId, setAccountId] = useState(undeposited?.id ?? null);
  const [date, setDate] = useState(todayIso());
  const [method, setMethod] = useState('CHECK');
  const [reference, setReference] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const customerRef = useRef(null);

  useEffect(() => customerRef.current?.focus(), []);

  const customerInvoices = open.filter((i) => i.customerName === customer);
  const currencies = [...new Set(customerInvoices.map((i) => i.currencyCode ?? ''))];
  const rows = customerInvoices
    .filter((i) => (i.currencyCode ?? '') === currency)
    .sort((a, b) => a.dueDate.localeCompare(b.dueDate) || a.invoiceNumber - b.invoiceNumber);
  const money = (c) => formatMoney(c, currency || null);
  const rateE6 = currency ? parseRateE6(rate) : null;
  const suggested = useSuggestedRate({ currency, date, rate, setRate });
  const pickCustomer = (name) => {
    setCustomer(name);
    const first = open.find((i) => i.customerName === name);
    const code = first?.currencyCode ?? '';
    setCurrency(code);
    suggested.adopt(first?.currencyCode ? formatRate(first.exchangeRateE6) : '');
    setApplied({});
    setReceived('');
  };
  const appliedCents = (inv) => parseMoneyToCents(applied[inv.id] ?? '') ?? 0;
  const totalApplied = rows.reduce((s, inv) => s + Math.max(0, appliedCents(inv)), 0);
  const receivedCents = parseMoneyToCents(received);
  const badRows = rows.filter((inv) => {
    const c = parseMoneyToCents(applied[inv.id] ?? '');
    return (applied[inv.id] ?? '').trim() !== '' && (c === null || c < 0 || c > inv.balanceCents);
  });

  // Typing the amount received auto-applies it to the oldest invoices first.
  const autoApply = (text) => {
    setReceived(text);
    let remaining = parseMoneyToCents(text) ?? 0;
    const next = {};
    for (const inv of rows) {
      const take = Math.max(0, Math.min(remaining, inv.balanceCents));
      next[inv.id] = take > 0 ? centsToInput(take) : '';
      remaining -= take;
    }
    setApplied(next);
  };

  const save = async () => {
    const inputs = rows
      .filter((inv) => appliedCents(inv) > 0)
      .map((inv) => ({
        invoiceId: inv.id,
        depositAccountId: accountId,
        paymentDate: date,
        amountCents: appliedCents(inv),
        paymentMethod: method,
        reference: reference || null,
        exchangeRateE6: currency ? rateE6 : null,
      }));
    if (inputs.length === 0) return setError('Enter a payment amount for at least one invoice.');
    if (currency && rateE6 === null) return setError('Enter the exchange rate for this payment.');
    if (badRows.length > 0) return setError('A payment amount is more than that invoice\'s open balance.');
    if (receivedCents !== null && received.trim() !== '' && receivedCents !== totalApplied) {
      return setError(`Amount received ${money(receivedCents)} does not match the ${money(totalApplied)} applied to invoices.`);
    }
    setBusy(true);
    try {
      await api.recordInvoicePayments(inputs);
      onDone(`Received ${money(totalApplied)} from ${customer} across ${inputs.length} invoice${inputs.length === 1 ? '' : 's'}.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Receive payment</h2>
        <label>
          <span className="qb-label">Customer</span>
          <select ref={customerRef} className="qb-input w-[220px]" value={customer} onChange={(e) => pickCustomer(e.target.value)}>
            <option value="">Choose a customer…</option>
            {customers.map((c) => (
              <option key={c} value={c}>{c}</option>
            ))}
          </select>
        </label>
        {currencies.length > 1 && (
          <label>
            <span className="qb-label">Currency</span>
            <select
              className="qb-input w-[86px]"
              value={currency}
              onChange={(e) => {
                setCurrency(e.target.value);
                const first = customerInvoices.find((i) => (i.currencyCode ?? '') === e.target.value);
                suggested.adopt(first?.currencyCode ? formatRate(first.exchangeRateE6) : '');
                setApplied({});
                setReceived('');
              }}
            >
              {currencies.map((c) => (
                <option key={c || 'home'} value={c}>{c || homeCurrency}</option>
              ))}
            </select>
          </label>
        )}
        {currency && (
          <label>
            <span className="qb-label">1 {currency} = ? {homeCurrency}</span>
            <span className="flex items-center gap-1.5">
              <input className="qb-input num w-[96px]" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" aria-invalid={rate !== '' && rateE6 === null} />
              {suggested.note}
            </span>
          </label>
        )}
        <label>
          <span className="qb-label">Payment date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Method</span>
          <select className="qb-input w-[140px]" value={method} onChange={(e) => setMethod(e.target.value)}>
            {PAYMENT_METHODS.map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Reference no.</span>
          <input className="qb-input w-[100px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect accounts={accounts} filter={depositFilter} value={accountId} onChange={setAccountId} className="qb-input w-[190px]" />
        </label>
        <label>
          <span className="qb-label">Amount received</span>
          <input className="qb-input num w-[120px]" value={received} onChange={(e) => autoApply(e.target.value)} placeholder="0.00" inputMode="decimal" disabled={!customer} />
        </label>
        <div className="ml-auto text-right">
          <div className="qb-label">Amount to apply</div>
          <div className="num text-3xl font-semibold leading-none">{money(totalApplied)}</div>
          {currency && rateE6 !== null && totalApplied > 0 && (
            <div className="text-xs text-qb-ink-muted">≈ {formatCents(toHomeCents(totalApplied, rateE6))} received</div>
          )}
        </div>
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[80px]" />
          <col className="w-[100px]" />
          <col className="w-[100px]" />
          <col />
          <col className="w-[120px]" />
          <col className="w-[120px]" />
          <col className="w-[130px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Invoice</th>
            <th className="qb-grid-head">Date</th>
            <th className="qb-grid-head">Due</th>
            <th className="qb-grid-head" />
            <th className="qb-grid-head text-right">Original</th>
            <th className="qb-grid-head text-right">Open balance</th>
            <th className="qb-grid-head text-right">Payment</th>
          </tr>
        </thead>
        <tbody>
          {!customer && (
            <tr>
              <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">
                {customers.length ? 'Choose a customer to see their open invoices.' : 'There are no open invoices to receive payment against.'}
              </td>
            </tr>
          )}
          {rows.map((inv) => {
            const bad = badRows.includes(inv);
            return (
              <tr key={inv.id} className="qb-grid-row">
                <td className="qb-grid-cell">#{inv.invoiceNumber}</td>
                <td className="qb-grid-cell">{formatDate(inv.issueDate)}</td>
                <td className={`qb-grid-cell ${inv.dueDate < todayIso() ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(inv.dueDate)}</td>
                <td className="qb-grid-cell" />
                <td className="qb-grid-cell num">{money(inv.totalCents)}</td>
                <td className="qb-grid-cell num">{money(inv.balanceCents)}</td>
                <td className="qb-grid-cell-edit">
                  <input
                    className="num"
                    value={applied[inv.id] ?? ''}
                    onChange={(e) => setApplied((a) => ({ ...a, [inv.id]: e.target.value }))}
                    placeholder="0.00"
                    inputMode="decimal"
                    aria-invalid={bad}
                    aria-label={`Payment on invoice ${inv.invoiceNumber}`}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy || !customer || accountId === null}>
            {busy ? 'Posting…' : 'Save & Post Payment'}
          </button>
        </div>
      </div>
    </section>
  );
}

function InvoiceList({ accounts, onNotice, data, error, onEdit }) {
  const { homeCurrency } = useCompany();
  const [activeId, setActiveId] = useState(null);
  const today = todayIso();
  // The Rep column shows once any invoice has a rep.
  const withRep = (data ?? []).some((inv) => inv.salesRep);
  const COLS = withRep ? 10 : 9;
  const [printPortal, forms] = useFormPrinter(onNotice);
  const emailed = useEmailStatuses('INVOICE');
  const [unsentOnly, setUnsentOnly] = useState(false);
  const sorter = useSort(
    (data ?? []).filter((inv) => !unsentOnly || emailed.get(inv.id)?.queued),
    {
      number: (inv) => inv.invoiceNumber,
      customer: (inv) => inv.customerName,
      rep: (inv) => inv.salesRep,
      date: (inv) => inv.issueDate,
      due: (inv) => inv.dueDate,
      total: (inv) => inv.homeTotalCents ?? inv.totalCents,
      paid: (inv) => inv.amountPaidCents,
      balance: (inv) => (inv.isVoided ? null : inv.homeBalanceCents ?? inv.balanceCents),
      status: (inv) => (inv.isVoided ? 'VOIDED' : inv.status),
    },
    { storageKey: 'invoices' },
  );
  const shown = sorter.sorted;
  const voidInvoice = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    onNotice({ kind: 'success', text: 'Invoice voided: a reversing entry now offsets it.' });
  };

  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      {printPortal}
      <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Invoices</h2>
        <span className="ml-2 text-xs text-qb-ink-muted">Click an open invoice to receive a full or partial payment</span>
        <label className="ml-auto flex items-center gap-1 text-xs" title="Invoices marked Email later that have not been emailed yet">
          <input type="checkbox" checked={unsentOnly} onChange={(e) => setUnsentOnly(e.target.checked)} />
          Unsent only
        </label>
      </div>
      {error && <Notice kind="error" text={error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <colgroup>
            <col className="w-[136px]" />
            <col />
            {withRep && <col className="w-[56px]" />}
            <col className="w-[92px]" />
            <col className="w-[92px]" />
            <col className="w-[112px]" />
            <col className="w-[112px]" />
            <col className="w-[112px]" />
            <col className="w-[96px]" />
            <col className="w-[30px]" />
          </colgroup>
          <thead>
            <tr>
              <SortTh sorter={sorter} col="number">No.</SortTh>
              <SortTh sorter={sorter} col="customer">Customer</SortTh>
              {withRep && <SortTh sorter={sorter} col="rep">Rep</SortTh>}
              <SortTh sorter={sorter} col="date">Date</SortTh>
              <SortTh sorter={sorter} col="due">Due</SortTh>
              <SortTh sorter={sorter} col="total" className="text-right">Total</SortTh>
              <SortTh sorter={sorter} col="paid" className="text-right">Paid</SortTh>
              <SortTh sorter={sorter} col="balance" className="text-right">Open balance</SortTh>
              <SortTh sorter={sorter} col="status">Status</SortTh>
              <th className="qb-grid-head" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {data && data.length === 0 && (
              <tr>
                <td colSpan={COLS} className="qb-grid-cell text-qb-ink-muted">No invoices yet.</td>
              </tr>
            )}
            {unsentOnly && shown.length === 0 && (
              <tr>
                <td colSpan={COLS} className="qb-grid-cell text-qb-ink-muted">No invoices are waiting to be emailed.</td>
              </tr>
            )}
            {shown.map((inv) => {
              const open = !inv.isVoided && (inv.status === 'UNPAID' || inv.status === 'PARTIAL');
              const overdue = open && inv.dueDate < today;
              const isActive = activeId === inv.id && open;
              return (
                <Fragment key={inv.id}>
                  <tr
                    className={`qb-grid-row ${isActive ? 'is-active' : ''} ${inv.isVoided ? 'text-qb-ink-faint' : ''}`}
                    onMouseDown={() => setActiveId(open && activeId !== inv.id ? inv.id : null)}
                  >
                    <td className="qb-grid-cell">
                      {inv.invoiceNumber}
                      {inv.isFinanceCharge && <span className="ml-1 text-2xs text-qb-ink-faint" title="Finance charge">FC</span>}
                      <EmailMarks status={emailed.get(inv.id)} />
                    </td>
                    <td className="qb-grid-cell" title={inv.customerName}>{inv.customerName}</td>
                    {withRep && <td className="qb-grid-cell">{inv.salesRep ?? ''}</td>}
                    <td className="qb-grid-cell">{formatDate(inv.issueDate)}</td>
                    <td className={`qb-grid-cell ${overdue ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(inv.dueDate)}</td>
                    <td className="qb-grid-cell num" title={inv.currencyCode ? `${formatCents(inv.homeTotalCents)} booked at ${formatRate(inv.exchangeRateE6)}` : undefined}>
                      {formatMoney(inv.totalCents, inv.currencyCode)}
                    </td>
                    <td className="qb-grid-cell num">{inv.amountPaidCents ? formatMoney(inv.amountPaidCents, inv.currencyCode) : ''}</td>
                    <td className="qb-grid-cell num font-semibold" title={inv.currencyCode ? `${formatCents(inv.homeBalanceCents)} open in home currency` : undefined}>
                      {!inv.isVoided && inv.balanceCents ? formatMoney(inv.balanceCents, inv.currencyCode) : ''}
                    </td>
                    <td className="qb-grid-cell">
                      <StatusBadge status={inv.status} overdue={overdue} voided={inv.isVoided} />
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for invoice ${inv.invoiceNumber}`}
                        items={[
                          ...formMenuItems('INVOICE', inv.id, forms, emailed.get(inv.id)),
                          {
                            label: 'Edit…',
                            disabled: inv.isVoided || inv.amountPaidCents > 0,
                            hint: inv.amountPaidCents > 0 ? 'Remove its payments and credits first' : undefined,
                            onSelect: () => onEdit(inv),
                          },
                          voidItem({
                            journalEntryId: inv.journalEntryId,
                            voided: inv.isVoided,
                            description: `Invoice #${inv.invoiceNumber} for ${inv.customerName} (${formatMoney(inv.totalCents, inv.currencyCode)}).`,
                            onVoid: voidInvoice,
                          }),
                        ]}
                      />
                    </td>
                  </tr>
                  {isActive && (
                    <PaymentRow
                      invoice={{ ...inv, homeCurrency }}
                      accounts={accounts}
                      colSpan={COLS}
                      onDone={(notice) => {
                        if (notice.kind === 'success') setActiveId(null);
                        onNotice(notice);
                      }}
                    />
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export default function Invoices({ accounts, intent }) {
  const { homeCurrency } = useCompany();
  const [currency, setCurrency] = useState('');
  const [rate, setRate] = useState('');
  const invoices = useLedgerQuery(api.listInvoices);
  const [receiving, setReceiving] = useState(false);
  const customerRef = useRef(null);
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultAccountId = useMemo(
    () => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null,
    [accounts],
  );
  const [customer, setCustomer] = useState('');
  const [issueDate, setIssueDate] = useState(todayIso());
  const [terms, setTerms] = useState('net30');
  const [dueDate, setDueDate] = useState(addDays(todayIso(), 30));
  const [lines, setLines] = useState(() => Array.from({ length: INITIAL_ROWS }, () => blankLine(null)));
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  // Set while the form holds an estimate's lines: posting then marks that estimate converted.
  const [fromEstimate, setFromEstimate] = useState(null);
  // Set while the form holds a posted invoice being edited: saving replaces it.
  const [editing, setEditing] = useState(null);
  const [discountText, setDiscountText] = useState('');
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);
  const sales = useSalesDetails(customer, customers.data);

  // Accounts arrive asynchronously; give untouched lines the default income account once known.
  useEffect(() => {
    if (defaultAccountId === null) return;
    setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultAccountId } : l)));
  }, [defaultAccountId]);

  useEffect(() => {
    if (intent?.action === 'invoice') {
      setReceiving(false);
      customerRef.current?.focus();
    } else if (intent?.action === 'receive-payment') {
      setReceiving(true);
    } else if (intent?.action === 'from-estimate' && intent.data?.estimateId) {
      setReceiving(false);
      api.getEstimateDetail(intent.data.estimateId).then(
        ({ estimate, lines: estimateLines, details }) => {
          setCustomer(estimate.customerName);
          sales.set(details);
          setLines([
            ...estimateLines.map((l) => ({
              ...blankLine(l.incomeAccountId),
              productId: l.productServiceId,
              classId: l.classId ?? null,
              isTaxable: Boolean(l.isTaxable),
              description: l.description,
              qty: formatQuantity(l.quantityE4).replace(/,/g, ''),
              rate: centsToInput(l.rateCents),
            })),
            blankLine(defaultAccountId),
          ]);
          setFromEstimate({ id: estimate.id, number: estimate.estimateNumber });
          tax.set(estimate.taxRateId ?? null);
          setNotice(null);
          customerRef.current?.focus();
        },
        (err) => setNotice({ kind: 'error', text: errorText(err) }),
      );
    }
    // Only a new intent loads anything; the default account is read when it does.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [intent]);

  const applyTerms = (termId, fromDate) => {
    const t = TERMS.find((x) => x.id === termId);
    if (t && fromDate) setDueDate(addDays(fromDate, t.days));
  };

  const { filled, invalidCount, payload } = summarizeLines(lines);
  const discountCents = parseDiscount(discountText, payload.reduce((s, l) => s + (l.amountCents ?? 0), 0));
  const { tax: taxCents, discount, total } = documentTotals(payload, tax.rate, discountCents ?? 0);

  const loadForEdit = (inv) => {
    api.getInvoiceDetail(inv.id).then(
      ({ invoice, items, details }) => {
        setReceiving(false);
        sales.set(details);
        setFromEstimate(null);
        setEditing({ id: invoice.id, number: invoice.invoiceNumber });
        setCustomer(invoice.customerName);
        setIssueDate(invoice.issueDate);
        setDueDate(invoice.dueDate);
        setTerms('custom');
        setCurrency(invoice.currencyCode ?? '');
        setRate(invoice.currencyCode ? formatRate(invoice.exchangeRateE6) : '');
        setDiscountText(invoice.discountCents ? centsToInput(invoice.discountCents) : '');
        tax.set(invoice.taxRateId ?? null);
        setLines([
          ...items.map((l) => ({
            ...blankLine(l.incomeAccountId),
            productId: l.productServiceId,
            classId: l.classId ?? null,
            isTaxable: Boolean(l.isTaxable),
            description: l.description,
            qty: formatQuantity(l.quantityE4).replace(/,/g, ''),
            rate: centsToInput(l.rateCents),
          })),
          blankLine(defaultAccountId),
        ]);
        setNotice(null);
        customerRef.current?.focus();
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  };

  const reset = () => {
    setCustomer('');
    setIssueDate(todayIso());
    setTerms('net30');
    setDueDate(addDays(todayIso(), 30));
    setLines(Array.from({ length: INITIAL_ROWS }, () => blankLine(defaultAccountId)));
    setFromEstimate(null);
    setEditing(null);
    setDiscountText('');
    formClass.reset();
    tax.reset();
    sales.reset();
  };

  const [memorizing, setMemorizing] = useState(null);
  const [emailLater, setEmailLater] = useState(false);
  const save = async (mode) => {
    if (!customer.trim()) return setNotice({ kind: 'error', text: 'Enter a customer name.' });
    if (filled.length === 0) return setNotice({ kind: 'error', text: 'Add at least one line item.' });
    if (invalidCount > 0) {
      return setNotice({ kind: 'error', text: `${invalidCount} line(s) need an income account, a quantity and a rate above zero.` });
    }
    if (dueDate < issueDate) return setNotice({ kind: 'error', text: 'Due date cannot be before the invoice date.' });
    if (currency && parseRateE6(rate) === null) return setNotice({ kind: 'error', text: `Enter the ${currency} to ${homeCurrency} exchange rate.` });
    if (discountCents === null) return setNotice({ kind: 'error', text: 'Enter the discount as an amount or a percentage such as 10%.' });
    // The product travels with each line: tracked stock records its cost of goods sold.
    const postedLines = formClass.apply(
      payload.map(({ productServiceId, description, quantityE4, rateCents, incomeAccountId, classId, isTaxable }) => ({
        productServiceId,
        description,
        quantityE4,
        rateCents,
        incomeAccountId,
        classId,
        isTaxable,
      })),
    );
    const warning = formClass.missing(postedLines);
    if (warning) return setNotice({ kind: 'error', text: warning });

    setBusy(true);
    try {
      const input = {
        customerName: customer.trim(),
        issueDate,
        dueDate,
        lines: postedLines,
        taxRateId: tax.rateId,
        discountCents: discount,
        currency: currency ? { code: currency, exchangeRateE6: parseRateE6(rate) } : null,
        ...sales.payload(),
      };
      if (mode === 'memorize') return setMemorizing(input);
      if (editing) {
        await api.replaceInvoice(editing.id, input);
        setNotice({ kind: 'success', text: `Invoice #${editing.number} replaced: the original is voided and the edited invoice is posted.` });
        reset();
        return;
      }
      const inv = fromEstimate ? await api.invoiceFromEstimate(fromEstimate.id, input) : await api.createInvoice(input);
      const queued = emailLater && (await api.setEmailLater('INVOICE', inv.id, true).then(() => true, () => false));
      setNotice({
        kind: 'success',
        text: inv.currencyCode
          ? `Invoice #${inv.invoiceNumber} for ${inv.customerName} posted: ${formatMoney(inv.totalCents, inv.currencyCode)} booked as Dr Accounts Receivable ${formatCents(inv.homeTotalCents)}.`
          : `Invoice #${inv.invoiceNumber} for ${inv.customerName} posted: Dr Accounts Receivable ${formatCents(inv.totalCents)} / Cr income.` +
            (fromEstimate ? ` Estimate #${fromEstimate.number} is marked converted.` : '') +
            (queued ? ' Marked Email later.' : ''),
      });
      reset();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {receiving && (
        <ReceivePaymentPanel
          invoices={invoices.data ?? []}
          accounts={accounts}
          onCancel={() => setReceiving(false)}
          onDone={(text) => {
            setReceiving(false);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {editing && (
        <Notice
          kind="info"
          text={`Editing invoice #${editing.number}. Saving voids the original and posts this version under a new number; both stay in the audit trail.`}
          onDismiss={reset}
        />
      )}
      {fromEstimate && (
        <Notice
          kind="info"
          text={`Invoicing estimate #${fromEstimate.number}: edit anything below, then post. The estimate is marked converted only when this invoice posts.`}
          onDismiss={reset}
        />
      )}
      <section className="qb-panel flex-none">
        <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
          <label className="w-[260px]">
            <span className="qb-label">Customer</span>
            <input
              ref={customerRef}
              className="qb-input w-full"
              value={customer}
              onChange={(e) => setCustomer(e.target.value)}
              placeholder="Customer name"
              list="invoice-customers"
            />
            <datalist id="invoice-customers">
              {(customers.data ?? []).map((c) => (
                <option key={c.id} value={c.name} />
              ))}
            </datalist>
          </label>
          <label>
            <span className="qb-label">Invoice date</span>
            <input
              type="date"
              className="qb-input w-[132px]"
              value={issueDate}
              onChange={(e) => {
                setIssueDate(e.target.value);
                applyTerms(terms, e.target.value);
              }}
            />
          </label>
          <label>
            <span className="qb-label">Terms</span>
            <select
              className="qb-input w-[128px]"
              value={terms}
              onChange={(e) => {
                setTerms(e.target.value);
                applyTerms(e.target.value, issueDate);
              }}
            >
              {TERMS.map((t) => (
                <option key={t.id} value={t.id}>{t.label}</option>
              ))}
              <option value="custom">Custom</option>
            </select>
          </label>
          <label>
            <span className="qb-label">Due date</span>
            <input
              type="date"
              className="qb-input w-[132px]"
              value={dueDate}
              onChange={(e) => {
                setDueDate(e.target.value);
                setTerms('custom');
              }}
            />
          </label>
          {sales.picker}
          {formClass.header}
          {tax.picker}
          <DiscountField value={discountText} onChange={setDiscountText} invalid={discountCents === null} />
          {!receiving && (
            <button type="button" className="qb-btn qb-btn-secondary self-center" onClick={() => setReceiving(true)}>
              Receive payment
            </button>
          )}
          <CurrencyFields homeCurrency={homeCurrency} currency={currency} setCurrency={setCurrency} rate={rate} setRate={setRate} date={issueDate} />
          <div className="ml-auto text-right">
            <div className="qb-label">Balance due</div>
            <div className="num text-3xl font-semibold leading-none">{formatMoney(total, currency || null)}</div>
            {(taxCents > 0 || discount > 0) && (
              <div className="text-xs text-qb-ink-muted">
                {discount > 0 && `less ${formatMoney(discount, currency || null)} discount`}
                {discount > 0 && taxCents > 0 && ' · '}
                {taxCents > 0 && `incl. ${formatMoney(taxCents, currency || null)} sales tax`}
              </div>
            )}
            {currency && parseRateE6(rate) !== null && total > 0 && (
              <div className="text-xs text-qb-ink-muted">≈ {formatCents(toHomeCents(total, parseRateE6(rate)))} in {homeCurrency}</div>
            )}
          </div>
        </div>

        <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultAccountId} currency={currency || null} withTax={tax.enabled} customer={customer} customers={customers.data} />

        <div className="flex items-center gap-2 px-2 py-1">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultAccountId)])}>
            Add line
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines([blankLine(defaultAccountId)])}>
            Clear all lines
          </button>
          <span className="text-xs text-qb-ink-muted">Click a row to edit · Enter moves to the next line</span>
          <div className="ml-auto flex items-center gap-3">
            <span className="text-sm">
              <span className="text-qb-ink-muted">Total </span>
              <span className="num font-semibold">{formatMoney(total, currency || null)}</span>
            </span>
            {!editing && (
              <label className="flex items-center gap-1 text-xs" title="Queue the invoice in Sales › Send Forms after posting">
                <input type="checkbox" checked={emailLater} onChange={(e) => setEmailLater(e.target.checked)} />
                Email later
              </label>
            )}
            <button type="button" className="qb-btn qb-btn-secondary" onClick={reset} disabled={busy}>
              Cancel
            </button>
            <button type="button" className="qb-btn qb-btn-secondary" onClick={() => save('memorize')} disabled={busy} title="Save as a recurring invoice">
              Memorize…
            </button>
            <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
              {busy ? 'Posting…' : editing ? 'Save changes' : 'Save & Post to Ledger'}
            </button>
          </div>
        </div>
        {memorizing && (
          <MemorizeDialog
            kind="INVOICE"
            payload={memorizing}
            date={issueDate}
            defaultName={customer.trim()}
            onClose={() => setMemorizing(null)}
            onSaved={(text) => {
              setMemorizing(null);
              setNotice({ kind: 'success', text });
            }}
          />
        )}
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      </section>

      <InvoiceList accounts={accounts} onNotice={setNotice} data={invoices.data} error={invoices.error} onEdit={loadForEdit} />
    </div>
  );
}
