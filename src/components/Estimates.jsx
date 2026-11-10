import { useEffect, useMemo, useRef, useState } from 'react';
import { layoutMessage } from '../lib/itemTypes.js';
import { useTransactionDate } from '../lib/myPreferences.js';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { DiscountField, documentTotals, parseDiscount, useFormTax } from './SalesTax.jsx';
import Notice from './Notice.jsx';
import FormToolbar, { fileButtons, navButtons } from './FormToolbar.jsx';
import { useDirtySince, useOpenFromList } from '../lib/windows.js';
import SavedDocView, { useSavedDoc } from './SavedDocView.jsx';
import { byDateThenNumber } from '../lib/formStep.js';
import RowMenu from './RowMenu.jsx';
import { EmailMarks, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import NameField from './NameField.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { useCustomerDelivery, useSalesPrefs } from '../lib/salesPrefs.js';
import { FormInfoPanel } from './InfoPanel.jsx';

/** Estimate lines priced at 0.00: on with the estimate_lines rebuild in migration 0072. */
const ZERO_LINES = false;

const STATUS = {
  PENDING: { label: '○ Pending', className: 'text-qb-ink' },
  ACCEPTED: { label: '✓ Accepted', className: 'font-semibold text-qb-green-active' },
  REJECTED: { label: '✕ Rejected', className: 'text-qb-ink-faint' },
  CONVERTED: { label: '● Converted · invoice', className: 'text-qb-info' },
};

function EstimateForm({ accounts, onSaved, onCancel, forms, savedStep }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  const [customer, setCustomer] = useState('');
  const [date, setDate] = useTransactionDate();
  const [expires, setExpires] = useState(() => addDays(date, 30));
  const [memo, setMemo] = useState('');
  const [lines, setLines] = useState(() => Array.from({ length: 3 }, () => blankLine(null)));
  const [busy, setBusy] = useState(false);
  const [emailLater, setEmailLater] = useState(false);
  const [error, setError] = useState(null);
  const customerRef = useRef(null);
  const openFromList = useOpenFromList();
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);
  const sales = useSalesDetails(customer, customers.data);
  useCustomerDelivery(customer, customers.data, { setEmailLater });

  useEffect(() => customerRef.current?.focus(), []);
  useEffect(() => {
    if (defaultIncome === null) return;
    setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);

  // A 0.00 line (an included extra) is allowed once estimate lines accept it.
  const { filled, invalidCount, payload, formLines, total: linesTotal, layoutError } = summarizeLines(lines, { allowZeroRate: ZERO_LINES });
  const [discountText, setDiscountText] = useState('');
  // Opening a saved estimate (Previous) asks for the entries to be saved or cleared first.
  const dirty = useDirtySince('estimate', { customer, memo, discountText, lines: lines.map((l) => [l.productId, l.description, l.qty, l.rate]) });
  const discountCents = parseDiscount(discountText, linesTotal);
  const { tax: taxCents, discount, total } = documentTotals(payload, tax.rate, discountCents ?? 0, formLines);

  const clear = () => {
    setCustomer('');
    setMemo('');
    setLines(Array.from({ length: 3 }, () => blankLine(defaultIncome)));
    formClass.reset();
    tax.reset();
    sales.reset();
    setDiscountText('');
    setError(null);
    customerRef.current?.focus();
  };

  const save = async (mode) => {
    if (!customer.trim()) return setError('Enter a customer.');
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a whole-number quantity and a rate above zero.`);
    if (expires && expires < date) return setError('The expiration date is before the estimate date.');
    if (layoutError) return setError(layoutMessage(layoutError));
    if (discountCents === null) return setError('Enter the discount as an amount or a percentage such as 10%.');
    setBusy(true);
    try {
      const estimate = await api.createEstimate({
        customerName: customer.trim(),
        estimateDate: date,
        expirationDate: expires || null,
        memo: memo.trim() || null,
        // Estimates do not post, so there is no missing-class warning; the class carries to the invoice.
        lines: formClass.apply(payload),
        taxRateId: tax.rateId,
        discountCents: discount,
        ...sales.payload(),
        formLines,
      });
      const queued = emailLater && (await api.setEmailLater('ESTIMATE', estimate.id, true).then(() => true, () => false));
      onSaved(`Estimate #${estimate.estimateNumber} for ${estimate.customerName} saved (${formatCents(estimate.totalCents)}). Estimates do not post to the ledger.${queued ? ' Marked Email later.' : ''}`);
      if (mode === 'print') forms.print('ESTIMATE', estimate.id);
      if (mode === 'email') forms.email('ESTIMATE', estimate.id);
      clear();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save: () => save('save'), clear, onClose: onCancel, busy });

  return (
    <div className="flex flex-none gap-2">
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel min-w-0 flex-1">
      <FormToolbar
        label="Estimate"
        groups={[
          navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...savedStep(busy || dirty) }),
          fileButtons({ onNew: clear, onSave: flow.saveAndNew, busy }),
          [
            { id: 'print', label: 'Print', icon: 'Print', menu: [{ label: 'Save and print…', disabled: busy, onSelect: () => save('print') }] },
            { id: 'email', label: 'Email', icon: 'Email', menu: [{ label: 'Save and email…', disabled: busy, onSelect: () => save('email') }] },
          ],
        ]}
      />
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New estimate</h2>
        <label className="w-[240px]">
          <span className="qb-label">Customer</span>
          <NameField kind="customer" inputRef={customerRef} className="qb-input w-full" names={customers.data ?? []} value={customer} onChange={setCustomer} placeholder="Customer name" />
        </label>
        <label>
          <span className="qb-label">Estimate date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Expiration date</span>
          <DateBox className="qb-input w-[132px]" value={expires} onChange={(e) => setExpires(e.target.value)} />
        </label>
        {sales.picker}
        {formClass.header}
        {tax.picker}
        <DiscountField value={discountText} onChange={setDiscountText} invalid={discountCents === null} />
        <div className="ml-auto text-right">
          <div className="qb-label">Total</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
          {(taxCents > 0 || discount > 0) && (
            <div className="text-xs text-qb-ink-muted">
              {discount > 0 && `less ${formatCents(discount)} discount`}
              {discount > 0 && taxCents > 0 && ' · '}
              {taxCents > 0 && `incl. ${formatCents(taxCents)} sales tax`}
            </div>
          )}
        </div>
      </div>
      {sales.addresses}
      <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultIncome} withTax={tax.enabled} customer={customer} customers={customers.data} itemTypes allowZeroRate={ZERO_LINES} />
      <div className="flex items-center gap-2 px-2 py-1">
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
          Add line
        </button>
        <input className="qb-input w-[300px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Message displayed on estimate" />
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex items-center gap-2">
          <label className="flex items-center gap-1 text-xs" title="Queue the estimate in Sales › Send Forms after saving">
            <input type="checkbox" checked={emailLater} onChange={(e) => setEmailLater(e.target.checked)} />
            Email later
          </label>
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <SaveButtons flow={flow} />
        </div>
      </div>
    </section>
    {/* The chosen customer's information panel (T56); it can be hidden. */}
    <FormInfoPanel kind="customer" name={customer} names={customers.data} />
    </div>
  );
}

export default function Estimates(props) {
  const { estimates, loaded } = useSalesPrefs();
  if (loaded && !estimates) {
    return (
      <div className="p-2">
        <Notice kind="info" text="Estimates are turned off for this company. An Admin can turn them on in Preferences › Jobs & Estimates; estimates already saved are kept." />
      </div>
    );
  }
  return <EstimateList {...props} />;
}

function EstimateList({ accounts, intent, navigate }) {
  const { data, error } = useLedgerQuery(api.listEstimates);
  const { salesOrders } = useSalesPrefs();
  const [composing, setComposing] = useState(intent?.action === 'estimate');
  const [notice, setNotice] = useState(null);
  const today = todayIso();

  useEffect(() => {
    if (intent?.action === 'estimate') setComposing(true);
  }, [intent]);

  const setStatus = async (estimate, status) => {
    try {
      await api.setEstimateStatus(estimate.id, status);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const toOrder = async (estimate) => {
    try {
      const made = await api.createSalesOrderFromEstimate(estimate.id, today);
      const left = made.leftOut.length ? ` Left on the estimate (add them on the invoice): ${made.leftOut.join('; ')}.` : '';
      setNotice({ kind: 'success', text: `Estimate #${estimate.estimateNumber} became sales order #${made.order.orderNumber}. Invoice it from Sales › Sales Orders.${left}` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const convert = async (estimate) => {
    const closed = await api.convertEstimateToInvoice(estimate.id, today, addDays(today, 30));
    setNotice({ kind: 'success', text: `Estimate #${closed.estimateNumber} converted to invoice #${closed.invoiceNumber} and posted to Accounts Receivable.` });
  };
  const rows = data ?? [];
  const [printPortal, forms] = useFormPrinter(setNotice);
  const emailed = useEmailStatuses('ESTIMATE');
  const [unsentOnly, setUnsentOnly] = useState(false);
  const sorter = useSort(
    rows.filter((e) => !unsentOnly || emailed.get(e.id)?.queued),
    {
      number: (e) => e.estimateNumber,
      customer: (e) => e.customerName,
      date: (e) => e.estimateDate,
      expires: (e) => e.expirationDate,
      amount: (e) => e.totalCents,
      status: (e) => STATUS[e.status].label,
    },
    { storageKey: 'estimates' },
  );
  const shown = sorter.sorted;
  // Saved estimates open read-only in the form's place; Previous / Next step through them.
  const ordered = useMemo(() => byDateThenNumber(rows, (e) => e.estimateDate, (e) => e.estimateNumber), [rows]);
  const saved = useSavedDoc(ordered);
  const viewing = saved.current;
  const openFromList = useOpenFromList();
  const openEstimate = (e) => {
    saved.open(e);
    setComposing(true);
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      {composing && viewing && (
        <SavedDocView
          label="Estimate"
          title={`Estimate #${viewing.estimateNumber} · ${viewing.customerName} · ${STATUS[viewing.status].label}${viewing.status === 'CONVERTED' && viewing.invoiceNumber ? ` #${viewing.invoiceNumber}` : ''}`}
          step={saved.step}
          onFind={() => openFromList('accounting/find', 'find', { text: '' })}
          onNew={saved.close}
          form={{ kind: 'ESTIMATE', id: viewing.id }}
          record={{ kind: 'ESTIMATE', id: viewing.id }}
          onNotice={setNotice}
          extra={[
            [
              {
                id: 'to-invoice',
                label: 'Create Invoice',
                icon: 'HomeInvoice',
                hint: viewing.status === 'CONVERTED' ? 'Already turned into an invoice' : viewing.status === 'REJECTED' ? 'Mark it pending or accepted first' : 'Load its lines into the invoice form to review before posting',
                disabled: viewing.status === 'CONVERTED' || viewing.status === 'REJECTED',
                onClick: () => navigate('sales/invoices', 'from-estimate', { estimateId: viewing.id }),
              },
            ],
          ]}
        />
      )}
      {composing && !viewing && (
        <EstimateForm accounts={accounts} forms={forms} savedStep={(blocked) => saved.fromNew(blocked)} onCancel={() => setComposing(false)} onSaved={(text) => setNotice({ kind: 'success', text })} />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load estimates: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Estimates</h2>
          <span className="text-xs text-qb-ink-muted">Non-posting quotes · ▾ to accept, reject, or turn into an invoice</span>
          <label className="ml-auto flex items-center gap-1 text-xs" title="Estimates marked Email later that have not been emailed yet">
            <input type="checkbox" checked={unsentOnly} onChange={(e) => setUnsentOnly(e.target.checked)} />
            Unsent only
          </label>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setComposing(true)}>
              New estimate
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[136px]" />
              <col />
              <col className="w-[92px]" />
              <col className="w-[92px]" />
              <col className="w-[112px]" />
              <col className="w-[150px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="number">No.</SortTh>
                <SortTh sorter={sorter} col="customer">Customer</SortTh>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="expires">Expires</SortTh>
                <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No estimates yet.</td>
                </tr>
              )}
              {unsentOnly && shown.length === 0 && (
                <tr>
                  <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No estimates are waiting to be emailed.</td>
                </tr>
              )}
              {shown.map((e) => {
                const expired = e.status !== 'CONVERTED' && e.expirationDate && e.expirationDate < today;
                // An estimate that became a sales order is invoiced through the order.
                const open = e.status !== 'CONVERTED' && !e.salesOrderNumber;
                return (
                  <tr key={e.id} className="qb-grid-row cursor-pointer" onDoubleClick={() => openEstimate(e)}>
                    <td className="qb-grid-cell">
                      {e.estimateNumber}
                      <EmailMarks status={emailed.get(e.id)} />
                    </td>
                    <td className="qb-grid-cell font-semibold">{e.customerName}</td>
                    <td className="qb-grid-cell">{formatDate(e.estimateDate)}</td>
                    <td className={`qb-grid-cell ${expired ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(e.expirationDate)}</td>
                    <td className="qb-grid-cell num">{formatCents(e.totalCents)}</td>
                    <td className={`qb-grid-cell ${STATUS[e.status].className}`}>
                      {STATUS[e.status].label}
                      {e.status === 'CONVERTED' && e.invoiceNumber ? ` #${e.invoiceNumber}` : ''}
                      {e.salesOrderNumber ? ` · sales order #${e.salesOrderNumber}` : ''}
                      {expired ? ' · expired' : ''}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        items={[
                          { label: 'Open', onSelect: () => openEstimate(e) },
                          ...formMenuItems('ESTIMATE', e.id, forms, emailed.get(e.id)),
                          {
                            label: 'Edit as invoice…',
                            disabled: !open || e.status === 'REJECTED',
                            hint: e.status === 'REJECTED' ? 'Mark it pending or accepted first' : 'Load its lines into the invoice form to review before posting',
                            onSelect: () => navigate('sales/invoices', 'from-estimate', { estimateId: e.id }),
                          },
                          {
                            label: 'Convert to invoice now',
                            disabled: !open || e.status === 'REJECTED',
                            hint: e.status === 'REJECTED' ? 'Mark it pending or accepted first' : undefined,
                            confirm: {
                              title: 'Convert to invoice?',
                              body: `This creates and posts an invoice to ${e.customerName} for ${formatCents(e.totalCents)}, dated today and due in 30 days, and closes estimate #${e.estimateNumber}.`,
                              confirmLabel: 'Create invoice',
                            },
                            onSelect: () => convert(e),
                          },
                          salesOrders && {
                            label: 'Create sales order',
                            disabled: !open || e.status === 'REJECTED',
                            hint: e.salesOrderNumber ? `Already sales order #${e.salesOrderNumber}` : e.status === 'REJECTED' ? 'Mark it pending or accepted first' : 'The customer ordered: ship and invoice it in parts from Sales › Sales Orders',
                            onSelect: () => toOrder(e),
                          },
                          { label: 'Mark accepted', disabled: !open || e.status === 'ACCEPTED', onSelect: () => setStatus(e, 'ACCEPTED') },
                          { label: 'Mark rejected', disabled: !open || e.status === 'REJECTED', onSelect: () => setStatus(e, 'REJECTED') },
                          { label: 'Mark pending', disabled: !open || e.status === 'PENDING', onSelect: () => setStatus(e, 'PENDING') },
                        ].filter(Boolean)}
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
