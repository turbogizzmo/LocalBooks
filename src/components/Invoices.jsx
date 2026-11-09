import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { layoutMessage } from '../lib/itemTypes.js';
import { newTransactionDate, useTransactionDate } from '../lib/myPreferences.js';
import AccountSelect from './AccountSelect.jsx';
import LineItemGrid, { blankLine, isFormLine, linesFromDocument, summarizeLines } from './LineItemGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { DiscountField, documentTotals, parseDiscount, useFormTax } from './SalesTax.jsx';
import Notice from './Notice.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { useTransactionReports } from './TransactionHistory.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { EmailMarks, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import Attachments, { attachPicked, PendingAttachments, pickAttachments, useAttachmentRows } from './Attachments.jsx';
import FormToolbar, { duplicateButton, fileButtons, navButtons } from './FormToolbar.jsx';
import { duplicateDates } from '../lib/formStep.js';
import { paymentTotals } from '../lib/paymentTotals.js';
import PaymentMethodTiles from './PaymentMethodTiles.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { PaymentMethodSelect, TermsSelect, usePaymentMethodChoice, useTerms } from './ProfileFields.jsx';
import { centsToInput, formatCents, formatMoney, formatRate, parseMoneyToCents, parseRateE6, toHomeCents } from '../lib/money.js';
import { foreignPaymentEffect } from '../lib/fx.js';
import CurrencyFields from './CurrencyFields.jsx';
import { useSuggestedRate } from './RateSuggestion.jsx';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { useOpenFromList, useUnsavedChanges, useWindowContent } from '../lib/windows.js';
import NameField from './NameField.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { PrintLaterBox, usePrintLater } from './PrintLater.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { defaultDepositAccount, useCustomerDelivery, useSalesPrefs } from '../lib/salesPrefs.js';

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
  const { undepositedDefault } = useSalesPrefs();
  const [accountId, setAccountId] = useState(() => defaultDepositAccount(accounts, undepositedDefault));
  const [date, setDate] = useState(todayIso());
  const [amount, setAmount] = useState(centsToInput(invoice.balanceCents));
  const foreign = invoice.currencyCode;
  const money = (c) => formatMoney(c, foreign);
  const [rate, setRate] = useState(foreign ? formatRate(invoice.exchangeRateE6) : '');
  const rateE6 = foreign ? parseRateE6(rate) : null;
  // The invoice's rate is only a starting point: with lookup on, the payment date's rate replaces it.
  const suggested = useSuggestedRate({ currency: foreign, date, rate, setRate, initialIsDefault: true });
  const method = usePaymentMethodChoice('CHECK');
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
        ...method.payload(),
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
              inputMode="decimal" data-money
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
            <PaymentMethodSelect value={method.id} onChange={method.set} className="qb-input w-[140px]" />
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
            <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
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

/**
 * Receive Payment: one customer payment applied across that customer's open invoices. How a typed
 * amount is spread (an exact match, else oldest first; oldest first; or not at all), whether ticking
 * invoices works out the amount, where it is deposited, and whether early-payment discounts are
 * taken follow Preferences › Payments.
 */
function ReceivePaymentPanel({ invoices, accounts, onDone, onCancel, onFind }) {
  const { homeCurrency } = useCompany();
  const prefs = useSalesPrefs();
  const open = invoices.filter((i) => !i.isVoided && (i.status === 'UNPAID' || i.status === 'PARTIAL'));
  const customers = [...new Set(open.map((i) => i.customerName))].sort((a, b) => a.localeCompare(b));
  const [customer, setCustomer] = useState('');
  // One payment is in one currency; the customer's open invoices are filtered to it.
  const [currency, setCurrency] = useState('');
  const [rate, setRate] = useState('');
  const [received, setReceived] = useState('');
  const [applied, setApplied] = useState({});
  // Early-payment discounts taken, by invoice, and what each invoice's terms offer on the date.
  const [discounts, setDiscounts] = useState({});
  const [offer, setOffer] = useState({ available: false, automatic: false, offered: [] });
  const [accountId, setAccountId] = useState(() => defaultDepositAccount(accounts, prefs.undepositedDefault));
  const [accountTouched, setAccountTouched] = useState(false);
  const [date, setDate] = useState(todayIso());
  const method = usePaymentMethodChoice('CHECK');
  const [reference, setReference] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const customerRef = useRef(null);
  const applySeq = useRef(0);

  // The deposit account follows the preference once it loads, until the person picks one.
  useEffect(() => {
    if (!accountTouched) setAccountId(defaultDepositAccount(accounts, prefs.undepositedDefault));
  }, [accounts, prefs.undepositedDefault, accountTouched]);

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
    setDiscounts({});
    setReceived('');
  };
  // The engine says which invoices' terms still offer a discount on the payment date (on what is
  // left of each, once, through the discount's last day), in the invoices' currency.
  const rowKey = rows.map((inv) => inv.id).join(',');
  useEffect(() => {
    if (rowKey === '' || !date) {
      setOffer((o) => (o.offered.length ? { ...o, offered: [] } : o));
      return undefined;
    }
    let live = true;
    api.getPaymentDiscounts(rowKey.split(',').map(Number), date).then(
      (next) => live && setOffer(next),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [rowKey, date]);
  const appliedCents = (inv) => parseMoneyToCents(applied[inv.id] ?? '') ?? 0;
  const offeredOn = (inv) => offer.offered.find((o) => o.invoiceId === inv.id)?.amountCents ?? 0;
  const discountCents = (inv) => parseMoneyToCents(discounts[inv.id] ?? '') ?? 0;
  const totalDiscount = rows.reduce((s, inv) => s + Math.max(0, discountCents(inv)), 0);
  const badDiscounts = rows.filter((inv) => {
    const text = (discounts[inv.id] ?? '').trim();
    const c = parseMoneyToCents(text);
    return text !== '' && (c === null || c < 0 || c > offeredOn(inv) || (c > 0 && appliedCents(inv) <= 0));
  });
  const totalApplied = rows.reduce((s, inv) => s + Math.max(0, appliedCents(inv)), 0);
  const receivedCents = parseMoneyToCents(received);
  // The totals under the grid: amount due, applied, discounts, and what stays open or unapplied.
  const totals = paymentTotals(rows, appliedCents, discountCents, received.trim() === '' ? null : receivedCents ?? null);
  const badRows = rows.filter((inv) => {
    const c = parseMoneyToCents(applied[inv.id] ?? '');
    return (applied[inv.id] ?? '').trim() !== '' && (c === null || c < 0 || c + Math.max(0, discountCents(inv)) > inv.balanceCents);
  });

  // Typing the amount received applies it as the company prefers (the backend decides, so the
  // rule is the same everywhere); the Auto Apply button applies even when that is off.
  const autoApply = (text, mode = null) => {
    setReceived(text);
    if (mode === null && prefs.autoApply === 'off') return;
    const cents = parseMoneyToCents(text) ?? 0;
    const seq = ++applySeq.current;
    if (cents <= 0) {
      setDiscounts({});
      return setApplied({});
    }
    api.getPaymentApplication(customer, currency, cents, mode, date).then(
      (plan) => {
        if (seq !== applySeq.current) return;
        const byId = new Map(plan.map((p) => [p.documentId, p]));
        setApplied(Object.fromEntries(rows.map((inv) => [inv.id, byId.has(inv.id) ? centsToInput(byId.get(inv.id).amountCents) : ''])));
        setDiscounts(Object.fromEntries(rows.map((inv) => [inv.id, byId.get(inv.id)?.discountCents > 0 ? centsToInput(byId.get(inv.id).discountCents) : ''])));
      },
      (err) => seq === applySeq.current && setError(errorText(err)),
    );
  };
  // Ticking pays the invoice in full: with discounts taken automatically, the money is what the
  // discount leaves.
  const tick = (inv, on) => {
    if (!on) return setPayment(inv, '', '');
    const discount = offer.automatic ? offeredOn(inv) : 0;
    setPayment(inv, centsToInput(inv.balanceCents - discount), discount > 0 ? centsToInput(discount) : '');
  };
  // A discount typed (or removed) keeps a ticked invoice paid in full.
  const setDiscount = (inv, text) => {
    const c = text.trim() === '' ? 0 : parseMoneyToCents(text);
    const paidInFull = appliedCents(inv) + discountCents(inv) === inv.balanceCents;
    if (paidInFull && c !== null && c >= 0 && c < inv.balanceCents) setPayment(inv, centsToInput(inv.balanceCents - c), text);
    else setDiscounts((d) => ({ ...d, [inv.id]: text }));
  };
  // A payment typed or ticked on an invoice: with auto-calculate on, the amount received follows.
  const setPayment = (inv, text, discountText = undefined) => {
    applySeq.current += 1;
    const next = { ...applied, [inv.id]: text };
    setApplied(next);
    // Clearing a payment drops its discount; a discount needs money with it.
    if (discountText !== undefined || text.trim() === '') setDiscounts((d) => ({ ...d, [inv.id]: discountText ?? '' }));
    if (prefs.autoCalculate) {
      const sum = rows.reduce((s, r) => s + Math.max(0, parseMoneyToCents(next[r.id] ?? '') ?? 0), 0);
      setReceived(sum > 0 ? centsToInput(sum) : '');
    }
  };

  const customerBalance = customerInvoices.filter((i) => (i.currencyCode ?? '') === currency).reduce((s, i) => s + i.balanceCents, 0);
  const clear = () => {
    setCustomer('');
    setCurrency('');
    setApplied({});
    setDiscounts({});
    setReceived('');
    setReference('');
    setError(null);
    customerRef.current?.focus();
  };

  const save = async () => {
    const inputs = rows
      .filter((inv) => appliedCents(inv) > 0)
      .map((inv) => ({
        invoiceId: inv.id,
        depositAccountId: accountId,
        paymentDate: date,
        amountCents: appliedCents(inv),
        ...method.payload(),
        reference: reference || null,
        exchangeRateE6: currency ? rateE6 : null,
        discountCents: Math.max(0, discountCents(inv)),
      }));
    if (inputs.length === 0) return setError('Enter a payment amount for at least one invoice.');
    if (currency && rateE6 === null) return setError('Enter the exchange rate for this payment.');
    if (badDiscounts.length > 0) return setError('A discount is more than that invoice\'s terms offer on this date, or has no payment with it.');
    if (badRows.length > 0) return setError('A payment amount (with its discount) is more than that invoice\'s open balance.');
    if (receivedCents !== null && received.trim() !== '' && receivedCents !== totalApplied) {
      return setError(`Amount received ${money(receivedCents)} does not match the ${money(totalApplied)} applied to invoices.`);
    }
    setBusy(true);
    try {
      await api.recordInvoicePayments(inputs);
      const discountNote = totalDiscount > 0 ? `, with ${money(totalDiscount)} of early-payment discounts` : '';
      onDone(`Received ${money(totalApplied)} from ${customer} across ${inputs.length} invoice${inputs.length === 1 ? '' : 's'}${discountNote}.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <FormToolbar
        label="Receive payment"
        groups={[
          navButtons({ onFind }),
          fileButtons({ onNew: clear, onSave: save, busy }),
          [
            {
              id: 'auto-apply',
              label: 'Auto Apply',
              icon: 'ReceivePayment',
              hint: 'Apply the amount received to the oldest invoices first',
              disabled: !customer || receivedCents === null || received.trim() === '',
              onClick: () => autoApply(received, prefs.autoApply === 'oldest' ? 'oldest' : 'exact'),
            },
          ],
        ]}
      />
      <div className="flex items-start justify-between px-3 pt-2">
        <h2 className="text-3xl font-light text-qb-ink">Receive Payment</h2>
        {customer && (
          <div className="flex items-baseline gap-3 pt-1 text-xs uppercase tracking-wide text-qb-ink-muted">
            Customer balance <span className="num text-lg font-semibold normal-case tracking-normal text-qb-ink">{money(customerBalance)}</span>
          </div>
        )}
      </div>
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <label>
          <span className="qb-label">Received from</span>
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
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Amount received</span>
          <input className="qb-input num w-[120px]" value={received} onChange={(e) => autoApply(e.target.value)} placeholder="0.00" inputMode="decimal" data-money disabled={!customer} />
        </label>
        <label>
          <span className="qb-label">Reference no.</span>
          <input className="qb-input w-[100px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect
            accounts={accounts}
            filter={depositFilter}
            value={accountId}
            onChange={(id) => {
              setAccountTouched(true);
              setAccountId(id);
            }}
            className="qb-input w-[190px]"
          />
        </label>
        <div>
          <span className="qb-label">Payment method</span>
          <PaymentMethodTiles value={method.id} onChange={method.set} />
        </div>
        <div className="ml-auto text-right">
          <div className="qb-label">Amount to apply</div>
          <div className="num text-3xl font-semibold leading-none">{money(totalApplied)}</div>
          {currency && rateE6 !== null && totalApplied > 0 && (
            <div className="text-xs text-qb-ink-muted">≈ {formatCents(toHomeCents(totalApplied, rateE6))} received</div>
          )}
          {totalDiscount > 0 && <div className="text-xs text-qb-ink-muted">plus {money(totalDiscount)} of discounts</div>}
        </div>
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[28px]" />
          <col className="w-[80px]" />
          <col className="w-[100px]" />
          <col className="w-[100px]" />
          <col />
          <col className="w-[120px]" />
          <col className="w-[120px]" />
          {offer.available && <col className="w-[110px]" />}
          <col className="w-[130px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head" aria-label="Pay" />
            <th className="qb-grid-head">Invoice</th>
            <th className="qb-grid-head">Date</th>
            <th className="qb-grid-head">Due</th>
            <th className="qb-grid-head" title="Early-payment discount the invoice's terms offer, and the last day to take it">Discount offered</th>
            <th className="qb-grid-head text-right">Original</th>
            <th className="qb-grid-head text-right">Open balance</th>
            {offer.available && (
              <th className="qb-grid-head text-right" title="Early-payment discount taken with this payment; it goes to the discount account in Preferences › Payments">
                Discount
              </th>
            )}
            <th className="qb-grid-head text-right">Payment</th>
          </tr>
        </thead>
        <tbody>
          {!customer && (
            <tr>
              <td colSpan={offer.available ? 9 : 8} className="qb-grid-cell text-qb-ink-muted">
                {customers.length ? 'Choose a customer to see their open invoices.' : 'There are no open invoices to receive payment against.'}
              </td>
            </tr>
          )}
          {rows.map((inv) => {
            const bad = badRows.includes(inv);
            return (
              <tr key={inv.id} className="qb-grid-row">
                <td className="qb-grid-cell text-center">
                  <input
                    type="checkbox"
                    className="accent-qb-green"
                    checked={appliedCents(inv) > 0}
                    onChange={(e) => tick(inv, e.target.checked)}
                    aria-label={inv.isStatementCharge ? `Pay statement charge of ${formatDate(inv.issueDate)}` : `Pay invoice ${inv.invoiceNumber}`}
                  />
                </td>
                <td className="qb-grid-cell">{inv.isStatementCharge ? 'Statement charge' : `#${inv.invoiceNumber}`}</td>
                <td className="qb-grid-cell">{formatDate(inv.issueDate)}</td>
                <td className={`qb-grid-cell ${inv.dueDate < todayIso() ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(inv.dueDate)}</td>
                <td className="qb-grid-cell text-sm" title={inv.termsName ?? undefined}>
                  {inv.discountUntil && inv.earlyDiscountCents > 0 && (date <= inv.discountUntil ? `${money(inv.earlyDiscountCents)} by ${formatDate(inv.discountUntil)}` : <span className="text-qb-ink-faint">ended {formatDate(inv.discountUntil)}</span>)}
                </td>
                <td className="qb-grid-cell num">{money(inv.totalCents)}</td>
                <td className="qb-grid-cell num">{money(inv.balanceCents)}</td>
                {offer.available && (
                  <td className={offeredOn(inv) > 0 || discounts[inv.id] ? 'qb-grid-cell-edit' : 'qb-grid-cell'}>
                    {(offeredOn(inv) > 0 || discounts[inv.id]) && (
                      <input
                        className="num"
                        value={discounts[inv.id] ?? ''}
                        onChange={(e) => setDiscount(inv, e.target.value)}
                        placeholder={centsToInput(offeredOn(inv))}
                        inputMode="decimal" data-money
                        aria-invalid={badDiscounts.includes(inv)}
                        aria-label={`Discount on invoice ${inv.invoiceNumber}`}
                      />
                    )}
                  </td>
                )}
                <td className="qb-grid-cell-edit">
                  <input
                    className="num"
                    value={applied[inv.id] ?? ''}
                    onChange={(e) => setPayment(inv, e.target.value)}
                    placeholder="0.00"
                    inputMode="decimal" data-money
                    aria-invalid={bad}
                    aria-label={`Payment on invoice ${inv.invoiceNumber}`}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
        {rows.length > 0 && (
          <tfoot>
            <tr className="font-semibold">
              <td className="qb-grid-cell" colSpan={5}>
                Totals
              </td>
              <td className="qb-grid-cell num">{money(totals.original)}</td>
              <td className="qb-grid-cell num">{money(totals.amountDue)}</td>
              {offer.available && <td className="qb-grid-cell num">{money(totals.discounts)}</td>}
              <td className="qb-grid-cell num">{money(totals.applied)}</td>
            </tr>
          </tfoot>
        )}
      </table>
      {rows.length > 0 && (
        <dl className="ml-auto grid grid-cols-[auto_auto] gap-x-4 px-2 py-1 text-sm" aria-label="Payment totals">
          <dt className="text-qb-ink-muted">Amount due</dt>
          <dd className="num text-right">{money(totals.amountDue)}</dd>
          <dt className="text-qb-ink-muted">Applied</dt>
          <dd className="num text-right">{money(totals.applied)}</dd>
          <dt className="text-qb-ink-muted">Discounts applied</dt>
          <dd className="num text-right">{money(totals.discounts)}</dd>
          <dt className="text-qb-ink-muted">Left due after this payment</dt>
          <dd className="num text-right">{money(totals.leftDue)}</dd>
          {totals.unapplied !== null && totals.unapplied !== 0 && (
            <>
              <dt className="text-qb-negative">{totals.unapplied > 0 ? 'Unapplied amount' : 'Applied more than received'}</dt>
              <dd className="num text-right text-qb-negative">{money(Math.abs(totals.unapplied))}</dd>
            </>
          )}
        </dl>
      )}
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

function InvoiceList({ accounts, onNotice, data, error, onEdit, onDuplicate }) {
  const { homeCurrency } = useCompany();
  const [activeId, setActiveId] = useState(null);
  const today = todayIso();
  // The Rep column shows once any invoice has a rep.
  const withRep = (data ?? []).some((inv) => inv.salesRep);
  const COLS = withRep ? 10 : 9;
  const attachments = useAttachmentRows();
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
                    // Double-click opens the invoice to edit, as Edit… in its menu does.
                    onDoubleClick={() =>
                      inv.isVoided
                        ? onNotice({ kind: 'info', text: `Invoice #${inv.invoiceNumber} is voided, so it can't be edited.` })
                        : onEdit(inv)
                    }
                    title={open ? 'Click to receive a payment, double-click to edit' : 'Double-click to edit'}
                  >
                    <td className="qb-grid-cell">
                      {inv.invoiceNumber}
                      {inv.isFinanceCharge && <span className="ml-1 text-2xs text-qb-ink-faint" title="Finance charge">FC</span>}
                      {inv.isStatementCharge && <span className="ml-1 text-2xs text-qb-ink-faint" title="Statement charge: billed on the customer's statement, not printed as an invoice">SC</span>}
                      <EmailMarks status={emailed.get(inv.id)} />
                      {inv.toBePrinted && !inv.isVoided && (
                        <span className="ml-1 rounded-xs bg-qb-green-tint px-1 text-2xs font-semibold text-qb-green-active" title="Waiting in File › Print Forms › Invoices">
                          To print
                        </span>
                      )}
                      {attachments.clip(inv.journalEntryId)}
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
                          ...(inv.isVoided
                            ? []
                            : [
                                {
                                  label: inv.toBePrinted ? 'Remove from print queue' : 'Print later',
                                  hint: inv.toBePrinted ? undefined : 'Queue it in File › Print Forms › Invoices',
                                  onSelect: () =>
                                    api.setToBePrinted('INVOICE', inv.id, !inv.toBePrinted).catch((err) => onNotice({ kind: 'error', text: errorText(err) })),
                                },
                              ]),
                          attachments.menuItem(inv.journalEntryId),
                          {
                            label: 'Edit…',
                            disabled: inv.isVoided,
                            hint: inv.amountPaidCents > 0 ? 'Its payments and credits stay applied to the edited version' : undefined,
                            onSelect: () => onEdit(inv),
                          },
                          { label: 'Duplicate', hint: 'A new invoice copied from this one, dated today', onSelect: () => onDuplicate(inv) },
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
                  {attachments.row(inv.journalEntryId, COLS)}
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
  const creditMemos = useLedgerQuery(api.listCreditMemos);
  const defaultAccountId = useMemo(
    () => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null,
    [accounts],
  );
  const [customer, setCustomer] = useState('');
  // The customer's credit memos with credit left to use (refund receipts paid the money back).
  const openCredits = useMemo(() => {
    const name = customer.trim().toLowerCase();
    if (!name) return [];
    return (creditMemos.data ?? []).filter(
      (m) => m.customerName.toLowerCase() === name && !m.isVoided && m.refundAccountId == null && m.remainingCents > 0,
    );
  }, [creditMemos.data, customer]);
  const openCreditCents = openCredits.reduce((sum, m) => sum + m.remainingCents, 0);
  const [issueDate, setIssueDate] = useTransactionDate();
  const termsList = useTerms();
  const [termsId, setTermsId] = useState(null);
  // Set once the user picks a term, or a saved invoice is loaded: the customer's term no longer fills in.
  const [termsTouched, setTermsTouched] = useState(false);
  const [dueDate, setDueDate] = useState(() => addDays(issueDate, 30));
  const [lines, setLines] = useState(() => Array.from({ length: INITIAL_ROWS }, () => blankLine(null)));
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  // Set while the form holds an estimate's lines: posting then marks that estimate converted.
  const [fromEstimate, setFromEstimate] = useState(null);
  // Set while the form holds a posted invoice being edited: saving replaces it.
  const [editing, setEditing] = useState(null);
  // Everything the form would save, as loaded; see `snapshot` below.
  const [baseline, setBaseline] = useState(null);
  const [discountText, setDiscountText] = useState('');
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);
  const sales = useSalesDetails(customer, customers.data);
  const { dropZeroLines } = useSalesPrefs();

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
    } else if (intent?.action === 'edit' && intent.data?.invoiceId) {
      // Opened in a window of its own from the list, or reopened with the desktop.
      loadForEdit({ id: intent.data.invoiceId }, false);
    } else if (intent?.action === 'duplicate' && intent.data?.invoiceId) {
      // Duplicate on a row of the invoice list.
      loadForEdit({ id: intent.data.invoiceId }, false, true, true);
    } else if (intent?.action === 'from-estimate' && intent.data?.estimateId) {
      setReceiving(false);
      api.getEstimateDetail(intent.data.estimateId).then(
        ({ estimate, lines: estimateLines, details, formLines: estimateForm }) => {
          setCustomer(estimate.customerName);
          sales.set(details);
          // Zero-amount lines stay on the estimate unless the company brings them over to price.
          const all = linesFromDocument(estimateLines, estimateForm, defaultAccountId);
          const dropZero = dropZeroLines && estimateLines.some((l) => l.rateCents > 0);
          setLines(dropZero ? all.filter((l) => isFormLine(l) || l.rate.trim() === '' || parseMoneyToCents(l.rate) !== 0) : all);
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

  const applyTerms = (t, fromDate) => {
    if (t && fromDate) setDueDate(addDays(fromDate, t.netDays));
  };
  // The customer's term (or Net 30) fills in, and sets the due date, until the user picks one.
  const customerRecord = (customers.data ?? []).find((c) => c.name.toLowerCase() === customer.trim().toLowerCase());
  useEffect(() => {
    if (termsTouched) return;
    const t =
      termsList.find((x) => x.isActive && x.id === customerRecord?.termsId) ??
      termsList.find((x) => x.isActive && x.netDays === 30 && x.discountPpm === 0) ??
      null;
    if (t && t.id !== termsId) {
      setTermsId(t.id);
      applyTerms(t, issueDate);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customerRecord?.id, customerRecord?.termsId, termsList, termsTouched, termsId]);

  const { filled, invalidCount, payload, formLines, total: linesTotal, layoutError } = summarizeLines(lines);
  const discountCents = parseDiscount(discountText, linesTotal);
  const { tax: taxCents, discount, total } = documentTotals(payload, tax.rate, discountCents ?? 0, formLines);
  useWindowContent(editing ? { name: `Invoice ${editing.number} · ${editing.customer}`, action: 'edit', data: { invoiceId: editing.id } } : null);
  // Every field the invoice saves, so changing any of them (dates, terms, currency, rate, discount,
  // tax, rep, ship and custom fields, class, lines and subtotals, discounts, charges, groups) counts as unsaved.
  const snapshot = JSON.stringify({
    customer: customer.trim(),
    issueDate,
    dueDate,
    payload,
    formLines,
    taxRateId: tax.rateId,
    discount: discountText.trim(),
    currency,
    rate: currency ? rate.trim() : '',
    sales: sales.payload(),
    formClass: formClass.formClass,
  });
  // A loaded invoice's baseline is taken once its fields have settled (the render after loading).
  useEffect(() => {
    if (editing && baseline === null) setBaseline(snapshot);
  }, [editing, baseline, snapshot]);
  // A new invoice counts as changed once it has a customer, a line or a discount.
  const dirty = editing
    ? baseline !== null && snapshot !== baseline
    : customer.trim() !== '' || payload.length > 0 || formLines.length > 0 || discountText.trim() !== '';
  useUnsavedChanges('invoice', dirty);
  // An invoice chosen in the list opens in a window of its own, as forms do in QuickBooks.
  const openFromList = useOpenFromList();

  // `focusCustomer` is off for an invoice opened in its own window, which should not open the
  // customer suggestions before anyone has touched it. `viewLocked` is for the toolbar's Previous
  // and Next, which show a voided or paid invoice locked (Save off, a notice saying why) instead of
  // stopping at it.
  // `asCopy` loads it as a new invoice copied from it (Duplicate).
  const loadForEdit = (inv, focusCustomer = true, viewLocked = false, asCopy = false) => {
    api.getInvoiceDetail(inv.id).then(
      ({ invoice, items, details, formLines: storedForm }) => {
        // However it was opened (Edit…, a double-click, a register, a customer's transactions),
        // the same rules as saving apply.
        if (asCopy) {
          setReceiving(false);
          sales.set(details);
          setFromEstimate(null);
          setTermsTouched(true);
          setTermsId(details?.termsId ?? null);
          setCurrency(invoice.currencyCode ?? '');
          setRate(invoice.currencyCode ? formatRate(invoice.exchangeRateE6) : '');
          setDiscountText(invoice.discountCents ? centsToInput(invoice.discountCents) : '');
          tax.set(invoice.taxRateId ?? null);
          setLines(linesFromDocument(items, storedForm, defaultAccountId));
          setCustomer(invoice.customerName);
          startCopy(invoice.invoiceNumber, invoice.issueDate, invoice.dueDate);
          return;
        }
        if (!viewLocked && invoice.isVoided) return setNotice({ kind: 'info', text: `Invoice #${invoice.invoiceNumber} is voided, so it can't be edited.` });
        setReceiving(false);
        sales.set(details);
        setFromEstimate(null);
        const loaded = linesFromDocument(items, storedForm, defaultAccountId);
        // Payments and credits applied stay applied to the edited version (see appliedText).
        const applied = invoice.amountPaidCents > 0 ? formatMoney(invoice.amountPaidCents, invoice.currencyCode) : null;
        setEditing({ id: invoice.id, number: invoice.invoiceNumber, customer: invoice.customerName, applied, appliedCents: invoice.amountPaidCents ?? 0 });
        // Cleared so the effect above records what was loaded, and closing the window or stepping
        // with Previous / Next asks only once something has changed.
        setBaseline(null);
        setCustomer(invoice.customerName);
        setIssueDate(invoice.issueDate);
        setDueDate(invoice.dueDate);
        setTermsTouched(true);
        setTermsId(details?.termsId ?? null);
        setCurrency(invoice.currencyCode ?? '');
        setRate(invoice.currencyCode ? formatRate(invoice.exchangeRateE6) : '');
        setDiscountText(invoice.discountCents ? centsToInput(invoice.discountCents) : '');
        tax.set(invoice.taxRateId ?? null);
        setLines(loaded);
        setNotice(null);
        if (focusCustomer) customerRef.current?.focus();
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  };

  // Duplicate: the invoice on the form becomes a new one, dated today and due as many days later as
  // it was; it gets the next number when it posts.
  const startCopy = (number, fromIssue, fromDue) => {
    const { date, due } = duplicateDates(fromIssue, fromDue, todayIso());
    setEditing(null);
    setBaseline(null);
    setIssueDate(date);
    setDueDate(due ?? date);
    setFiles([]);
    setShowAttachments(false);
    setNotice({ kind: 'info', text: `A copy of invoice #${number}, dated today: it gets the next invoice number when you save it.` });
  };
  const duplicateInvoice = () => editing && startCopy(editing.number, issueDate, dueDate);

  const reset = () => {
    setCustomer('');
    setIssueDate(newTransactionDate());
    setTermsTouched(false);
    setTermsId(null);
    setDueDate(addDays(newTransactionDate(), 30));
    setLines(Array.from({ length: INITIAL_ROWS }, () => blankLine(defaultAccountId)));
    setFromEstimate(null);
    setEditing(null);
    setBaseline(null);
    setDiscountText('');
    setFiles([]);
    setShowAttachments(false);
    formClass.reset();
    tax.reset();
    sales.reset();
  };

  const [memorizing, setMemorizing] = useState(null);
  const [emailLater, setEmailLater] = useState(false);
  const printLater = usePrintLater('INVOICE', !editing);
  useCustomerDelivery(customer, customers.data, { enabled: !editing, setEmailLater, printLater });
  // Files picked for an invoice not posted yet; attached once it posts.
  const [files, setFiles] = useState([]);
  const [showAttachments, setShowAttachments] = useState(false);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const emailed = useEmailStatuses('INVOICE');
  // The posted invoice on the form, as the list has it, for the toolbar's Print, Email, Void and Attach.
  const current = editing ? (invoices.data ?? []).find((i) => i.id === editing.id) ?? null : null;
  // History (Ctrl+H) and Journal (Ctrl+Y) of the invoice on the form.
  const reports = useTransactionReports();
  const ordered = useMemo(() => [...(invoices.data ?? [])].sort((a, b) => a.invoiceNumber - b.invoiceNumber || a.id - b.id), [invoices.data]);
  const at = current ? ordered.indexOf(current) : -1;
  const step = (inv) => {
    setShowAttachments(false);
    setFiles([]);
    loadForEdit(inv, false, true);
  };
  // Voided invoices are shown but not saved again.
  const locked = current && current.isVoided;
  const appliedText = editing?.applied ? `This invoice has ${editing.applied} in payments or credits applied. Saving will keep them applied to the new version.` : null;

  const save = async (mode) => {
    if (locked) return setNotice({ kind: 'error', text: 'This invoice is voided.' });
    if (!customer.trim()) return setNotice({ kind: 'error', text: 'Enter a customer name.' });
    if (filled.length === 0) return setNotice({ kind: 'error', text: 'Add at least one line item.' });
    if (invalidCount > 0) {
      return setNotice({ kind: 'error', text: `${invalidCount} line(s) need an income account, a quantity and a rate above zero.` });
    }
    if (dueDate < issueDate) return setNotice({ kind: 'error', text: 'Due date cannot be before the invoice date.' });
    if (currency && parseRateE6(rate) === null) return setNotice({ kind: 'error', text: `Enter the ${currency} to ${homeCurrency} exchange rate.` });
    if (layoutError) return setNotice({ kind: 'error', text: layoutMessage(layoutError) });
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
    // Said before any "save anyway?" question; saving checks it again.
    if (editing?.appliedCents > 0 && total < editing.appliedCents) {
      return setNotice({
        kind: 'error',
        text: `Invoice #${editing.number} has ${editing.applied} in payments and credits applied; the edited total, ${formatMoney(total, currency || null)}, is less. Void or remove a payment or credit first.`,
      });
    }

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
        termsId,
        formLines,
      };
      if (mode === 'memorize') return setMemorizing(input);
      // A credit limit warns; it never stops the sale.
      const homeTotal = currency ? toHomeCents(total, parseRateE6(rate)) : total;
      const over = await api.checkCreditLimit(input.customerName, homeTotal, editing?.id ?? null);
      if (
        over &&
        !(await askConfirm(
          `This invoice takes ${over.customerName} over their credit limit of ${formatCents(over.creditLimitCents)}: they owe ${formatCents(over.balanceCents)} now and would owe ${formatCents(over.newBalanceCents)}. Save it anyway?`,
          { confirmLabel: 'Save anyway' },
        ))
      ) {
        return;
      }
      if (editing) {
        await api.replaceInvoice(editing.id, input, appliedText);
        setNotice({ kind: 'success', text: `Invoice #${editing.number} replaced: the original is voided and the edited invoice is posted.` });
        reset();
        return true;
      }
      const inv = fromEstimate ? await api.invoiceFromEstimate(fromEstimate.id, input) : await api.createInvoice(input, printLater.on);
      if (fromEstimate && printLater.on) await api.setToBePrinted('INVOICE', inv.id, true).catch(() => {});
      printLater.remember(printLater.on);
      const queued = emailLater && (await api.setEmailLater('INVOICE', inv.id, true).then(() => true, () => false));
      const attachError = files.length > 0 ? await attachPicked(inv.journalEntryId, files) : null;
      // Credits apply to home-currency invoices only; the offer comes before printing or emailing,
      // so the form shows the balance after them.
      let creditNote = '';
      if (!inv.currencyCode && openCreditCents > 0) {
        const use = Math.min(openCreditCents, inv.totalCents);
        if (
          await askConfirm(
            `${inv.customerName} has ${formatCents(openCreditCents)} in available credits. Apply ${formatCents(use)} to invoice #${inv.invoiceNumber} now?`,
            { confirmLabel: 'Apply credits' },
          )
        ) {
          try {
            const used = await api.applyOpenCreditsToInvoice(inv.id, issueDate);
            if (used.count > 0) creditNote = ` ${formatCents(used.appliedCents)} in credits applied.`;
          } catch (err) {
            creditNote = ` The credits were not applied: ${errorText(err)}`;
          }
        }
      }
      if (mode === 'print') forms.print('INVOICE', inv.id);
      if (mode === 'email') forms.email('INVOICE', inv.id);
      setNotice({
        kind: 'success',
        text: inv.currencyCode
          ? `Invoice #${inv.invoiceNumber} for ${inv.customerName} posted: ${formatMoney(inv.totalCents, inv.currencyCode)} booked as Dr Accounts Receivable ${formatCents(inv.homeTotalCents)}.`
          : `Invoice #${inv.invoiceNumber} for ${inv.customerName} posted: Dr Accounts Receivable ${formatCents(inv.totalCents)} / Cr income.` +
            (fromEstimate ? ` Estimate #${fromEstimate.number} is marked converted.` : '') +
            creditNote +
            (queued ? ' Marked Email later.' : '') +
            (printLater.on ? ' Waiting in File › Print Forms › Invoices.' : '') +
            (attachError ? ` ${attachError}` : ''),
      });
      reset();
      customerRef.current?.focus();
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  // The invoice form fills its window: Save & Close closes it (formFlow.jsx).
  const flow = useFormFlow({ save: () => save('post'), clear: reset, busy, saveDisabled: Boolean(locked) });
  const onFind = () => openFromList('accounting/find', 'find', { text: '' });
  const saved = current ? { kind: 'INVOICE', id: current.id } : null;
  const notSavedHint = 'Save the invoice first, or use the Post and… entry';
  const toolbarGroups = [
    navButtons({
      onFind,
      onPrevious: () => step(at > 0 ? ordered[at - 1] : ordered[ordered.length - 1]),
      onNext: () => step(ordered[at + 1]),
      previousDisabled: busy || dirty || ordered.length === 0 || at === 0,
      nextDisabled: busy || dirty || !current || at >= ordered.length - 1,
      blockedHint: dirty ? 'Save or clear your changes first' : undefined,
    }),
    fileButtons({
      onNew: reset,
      onSave: flow.saveAndNew,
      saveHint: editing ? 'Save changes: voids the original and posts this version' : 'Save and post to the ledger',
      busy,
      saveDisabled: Boolean(locked),
      onVoid: () =>
        api.voidTransaction(current.journalEntryId).then(() => {
          setNotice({ kind: 'success', text: `Invoice #${current.invoiceNumber} voided: a reversing entry now offsets it.` });
          reset();
        }),
      voidDisabled: !current?.journalEntryId || current.isVoided || current.amountPaidCents > 0,
      voidHint: !current ? 'Open a saved invoice to void it' : current.amountPaidCents > 0 ? 'Void its payments first' : undefined,
      voidConfirm: current && {
        title: 'Void this transaction?',
        body: `Invoice #${current.invoiceNumber} for ${current.customerName} (${formatMoney(current.totalCents, current.currencyCode)}). A reversing journal entry dated the same day will zero out its effect. The original stays in the books and the audit log. This cannot be undone.`,
        confirmLabel: 'Yes, void it',
      },
    }),
    [
      {
        id: 'print',
        label: 'Print',
        icon: 'Print',
        menu: [
          { label: 'Print…', disabled: !saved, hint: saved ? undefined : notSavedHint, onSelect: () => forms.print(saved.kind, saved.id) },
          { label: 'Save as PDF…', disabled: !saved, hint: saved ? undefined : notSavedHint, onSelect: () => forms.savePdf(saved.kind, saved.id) },
          { label: 'Print Envelope…', disabled: !saved, hint: saved ? undefined : notSavedHint, onSelect: () => forms.envelope(saved.kind, saved.id) },
          !editing && { label: 'Post and print…', disabled: busy, onSelect: () => save('print') },
        ],
      },
      {
        id: 'email',
        label: 'Email',
        icon: 'Email',
        menu: [
          { label: 'Email…', disabled: !saved, hint: saved ? undefined : notSavedHint, onSelect: () => forms.email(saved.kind, saved.id) },
          !editing && { label: 'Post and email…', disabled: busy, onSelect: () => save('email') },
          saved && !current.isVoided &&
            (emailed.get(current.id)?.queued
              ? { label: 'Remove from Email later', onSelect: () => forms.setLater('INVOICE', current.id, false) }
              : { label: 'Email later', hint: 'Queue it in Sales › Send Forms', onSelect: () => forms.setLater('INVOICE', current.id, true) }),
        ],
      },
      {
        id: 'attach',
        label: 'Attach File',
        icon: 'Attach',
        hint: current ? 'Show and add files attached to this invoice' : 'Pick files to attach when this invoice posts',
        disabled: busy || current?.journalEntryId === null,
        onClick: async () => {
          if (current) setShowAttachments((v) => !v);
          else {
            const picked = await pickAttachments();
            if (picked.length) setFiles((f) => [...new Set([...f, ...picked])]);
          }
        },
      },
    ],
    [
      { id: 'memorize', label: 'Memorize', icon: 'Memorize', hint: 'Save as a recurring invoice', disabled: busy, onClick: () => save('memorize') },
      duplicateButton({ onDuplicate: duplicateInvoice, saved: Boolean(editing), dirty, busy, what: 'invoice' }),
    ],
    [
      ...reports.buttons({ journalEntryId: current?.journalEntryId, label: current ? `Invoice #${current.invoiceNumber}` : '' }),
      { id: 'receive', label: 'Receive Payments', icon: 'ReceivePayment', hint: 'Record a customer payment against open invoices', disabled: receiving, onClick: () => setReceiving(true) },
    ],
  ];

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      {receiving && (
        <ReceivePaymentPanel
          onFind={onFind}
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
          text={`Editing invoice #${editing.number}. ${appliedText ? `${appliedText} ` : ''}Saving voids the original and posts this version under a new number; both stay in the audit trail.`}
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
      <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel flex-none">
        {reports.panel}
        <FormToolbar label="Invoice" groups={toolbarGroups} />
        {locked && (
          <Notice
            kind="info"
            text={`Invoice #${current.invoiceNumber} is voided. It stays in the books for the record.`}
          />
        )}
        {showAttachments && current?.journalEntryId && (
          <div className="border-b border-qb-border bg-qb-canvas">
            <Attachments entryId={current.journalEntryId} />
          </div>
        )}
        <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
          <label className="w-[260px]">
            <span className="qb-label">Customer</span>
            <NameField kind="customer" inputRef={customerRef} className="qb-input w-full" names={customers.data ?? []} value={customer} onChange={setCustomer} placeholder="Customer name" />
          </label>
          <label>
            <span className="qb-label">Invoice date</span>
            <DateBox
              className="qb-input w-[132px]"
              value={issueDate}
              onChange={(e) => {
                setIssueDate(e.target.value);
                applyTerms(termsList.find((t) => t.id === termsId), e.target.value);
              }}
            />
          </label>
          <label>
            <span className="qb-label">Terms</span>
            <TermsSelect
              className="qb-input w-[128px]"
              value={termsId}
              onChange={(t) => {
                setTermsTouched(true);
                setTermsId(t?.id ?? null);
                applyTerms(t, issueDate);
              }}
            />
          </label>
          <label>
            <span className="qb-label">Due date</span>
            <DateBox
              className="qb-input w-[132px]"
              value={dueDate}
              onChange={(e) => setDueDate(e.target.value)}
            />
          </label>
          {sales.picker}
          {formClass.header}
          {tax.picker}
          <DiscountField value={discountText} onChange={setDiscountText} invalid={discountCents === null} />
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
            {!editing && !currency && openCreditCents > 0 && (
              <div className="text-xs text-qb-ink-muted" title={openCredits.map((m) => `Credit #${m.creditNumber}: ${formatCents(m.remainingCents)}`).join('\n')}>
                Available credits: {formatCents(openCreditCents)} ({openCredits.length === 1 ? '1 credit memo' : `${openCredits.length} credit memos`}), offered on save
              </div>
            )}
            {currency && parseRateE6(rate) !== null && total > 0 && (
              <div className="text-xs text-qb-ink-muted">≈ {formatCents(toHomeCents(total, parseRateE6(rate)))} in {homeCurrency}</div>
            )}
          </div>
        </div>

        {sales.addresses}
        <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultAccountId} currency={currency || null} withTax={tax.enabled} customer={customer} customers={customers.data} itemTypes serviceDates />

        <div className="flex items-center gap-2 px-2 py-1">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultAccountId)])}>
            Add line
          </button>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines([blankLine(defaultAccountId)])}>
            Clear all lines
          </button>
          {!current && files.length > 0 && <PendingAttachments paths={files} onChange={setFiles} label="Attach another…" />}
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
            {!editing && <PrintLaterBox printLater={printLater} />}
            <SaveButtons flow={flow} busyLabel="Posting…" />
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

      <InvoiceList accounts={accounts} onNotice={setNotice} data={invoices.data} error={invoices.error} onEdit={(inv) => openFromList('sales/invoices', 'edit', { invoiceId: inv.id })}
        onDuplicate={(inv) => openFromList('sales/invoices', 'duplicate', { invoiceId: inv.id })}
      />
    </div>
  );
}
