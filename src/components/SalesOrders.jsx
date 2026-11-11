import { useEffect, useMemo, useRef, useState } from 'react';
import { useTransactionDate } from '../lib/myPreferences.js';
import LineItemGrid, { blankLine, linesFromDocument, summarizeLines } from './LineItemGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { documentTotals, useFormTax } from './SalesTax.jsx';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { SortTh, useSort } from './SortHead.jsx';
import NameField from './NameField.jsx';
import DocNumberBox, { useDocNumber } from './DocNumberBox.jsx';
import DateBox from './DateBox.jsx';
import { FormInfoPanel } from './InfoPanel.jsx';
import { EmailMarks, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import ListReport, { useListReports } from './ListReports.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity, parseQuantityE4 } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { useSalesPrefs } from '../lib/salesPrefs.js';

const STATUS = {
  OPEN: { label: '○ Open', className: 'text-qb-ink' },
  PARTLY_INVOICED: { label: '◐ Partly invoiced', className: 'font-semibold text-qb-info' },
  CLOSED: { label: '● Closed', className: 'text-qb-ink-faint' },
};

/** The orders, and the Report Center's sales order reports (`list_reports/sales_orders.rs`). */
const TABS = [
  ['orders', 'Sales orders'],
  ['list:open-so-customer', 'Open by customer'],
  ['list:open-so-item', 'Open by item'],
  ['list:pending-sales', 'Pending sales'],
];

/** A new sales order, or an existing one (`order`, its detail) being changed. */
function SalesOrderForm({ accounts, order, onSaved, onCancel }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  const [customer, setCustomer] = useState(order?.customerName ?? '');
  const [date, setDate] = useTransactionDate();
  const [expected, setExpected] = useState(order?.expectedDate ?? '');
  const [memo, setMemo] = useState(order?.memo ?? '');
  const [lines, setLines] = useState(() =>
    order
      ? linesFromDocument(order.lines, [], null).map((l, i) => (i < order.lines.length ? { ...l, lineId: order.lines[i].id, invoicedQtyE4: order.lines[i].invoicedQtyE4 } : l))
      : Array.from({ length: 3 }, () => blankLine(null)),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const customerRef = useRef(null);
  // Order #: the next number, shown before saving; the person may type another. A saved order keeps its number.
  const docNumber = useDocNumber('SALES_ORDER', 'Order');
  const formClass = useFormClass(accounts);
  const tax = useFormTax(customer, customers.data);
  const sales = useSalesDetails(customer, customers.data);

  useEffect(() => {
    customerRef.current?.focus();
    if (order) {
      setDate(order.orderDate);
      tax.set(order.taxRateId ?? null);
      sales.set({ salesRepId: order.salesRepId, shipMethodId: order.shipMethodId, customerMessageId: order.customerMessageId, billTo: order.billTo, shipTo: order.shipTo, poNumber: order.poNumber, fob: order.fob });
    }
    // Loads the order once, when the form opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    if (defaultIncome === null) return;
    setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);

  const { evaluated, filled, invalidCount, payload, formLines } = summarizeLines(lines);
  const { tax: taxCents, total } = documentTotals(payload, tax.rate, 0, []);
  // The grid lines behind `payload`, in the same order, to send each kept line's id back.
  const itemLines = lines.filter((_, i) => !evaluated[i].blank && !evaluated[i].form);

  const save = async () => {
    if (!customer.trim()) return setError('Enter a customer.');
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a quantity and a rate above zero.`);
    if (formLines.length > 0) return setError('Subtotals, discounts and group headings go on the invoice; a sales order holds item lines only.');
    if (expected && expected < date) return setError('The expected date is before the order date.');
    setBusy(true);
    setError(null);
    try {
      const chosen = order ? { value: null } : await docNumber.resolve();
      if (chosen.cancelled) return;
      if (chosen.error) return setError(chosen.error);
      const input = {
        orderNumber: chosen.value,
        customerName: customer.trim(),
        orderDate: date,
        expectedDate: expected || null,
        memo: memo.trim() || null,
        lines: formClass.apply(payload).map((l, k) => ({ ...l, id: itemLines[k]?.lineId ?? null })),
        taxRateId: tax.rateId,
        ...sales.payload(),
      };
      const saved = order ? await api.updateSalesOrder(order.id, input) : await api.createSalesOrder(input);
      onSaved(`Sales order #${saved.orderNumber} for ${saved.customerName} saved (${formatCents(saved.totalCents)} before tax). Sales orders don't post; each invoice made from one does.`);
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-none gap-2">
      <section className="qb-panel min-w-0 flex-1" aria-label={order ? `Sales order ${order.orderNumber}` : 'New sales order'}>
        <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
          <h2 className="mr-2 self-center text-md font-semibold">{order ? `Sales order #${order.orderNumber}` : 'New sales order'}</h2>
          <label className="w-[240px]">
            <span className="qb-label">Customer</span>
            <NameField kind="customer" inputRef={customerRef} className="qb-input w-full" names={customers.data ?? []} value={customer} onChange={setCustomer} placeholder="Customer name" />
          </label>
          <label>
            <span className="qb-label">Order date</span>
            <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
          <DocNumberBox number={docNumber} fixed={order ? String(order.orderNumber) : null} />
          <label>
            <span className="qb-label">Expected</span>
            <DateBox className="qb-input w-[132px]" value={expected} onChange={(e) => setExpected(e.target.value)} />
          </label>
          {sales.picker}
          {formClass.header}
          {tax.picker}
          <div className="ml-auto text-right">
            <div className="qb-label">Total</div>
            <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
            {taxCents > 0 && <div className="text-xs text-qb-ink-muted">incl. {formatCents(taxCents)} sales tax (worked out again on each invoice)</div>}
          </div>
        </div>
        {sales.addresses}
        <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultIncome} withTax={tax.enabled} customer={customer} customers={customers.data} />
        <div className="flex items-center gap-2 px-2 py-1">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
            Add line
          </button>
          <input className="qb-input w-[300px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo" />
          {error && <span className="text-sm text-qb-negative">{error}</span>}
          <div className="ml-auto flex items-center gap-2">
            <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
              Close
            </button>
            <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
              Save sales order
            </button>
          </div>
        </div>
      </section>
      <FormInfoPanel kind="customer" name={customer} names={customers.data} />
    </div>
  );
}

/** Choose how much of each open line to invoice now; the invoice form then opens with those lines. */
function InvoicePartDialog({ order, onClose, onInvoice }) {
  const open = order.lines.filter((l) => l.leftQtyE4 > 0);
  const [qty, setQty] = useState(() => Object.fromEntries(open.map((l) => [l.id, formatQuantity(l.leftQtyE4).replace(/,/g, '')])));
  // Blank or zero leaves a line for a later invoice.
  const parsed = Object.fromEntries(open.map((l) => [l.id, /^\s*0*(\.0*)?\s*$/.test(qty[l.id]) ? 0 : parseQuantityE4(qty[l.id])]));
  const bad = open.find((l) => parsed[l.id] === null || parsed[l.id] < 0 || parsed[l.id] > l.leftQtyE4);
  const any = open.some((l) => parsed[l.id] > 0);
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Create invoice from sales order">
      <div className="qb-panel flex w-[640px] flex-col gap-2 p-3 shadow-panel">
        <h2 className="text-md font-semibold">Create invoice from sales order #{order.orderNumber}</h2>
        <p className="text-xs text-qb-ink-muted">How much of each line ships on this invoice. You can still change rates and add lines in the invoice form before it posts.</p>
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Item</th>
              <th className="qb-grid-head text-right">Ordered</th>
              <th className="qb-grid-head text-right">Invoiced</th>
              <th className="qb-grid-head text-right">Left</th>
              <th className="qb-grid-head text-right">To invoice</th>
            </tr>
          </thead>
          <tbody>
            {open.map((l) => (
              <tr key={l.id} className="qb-grid-row">
                <td className="qb-grid-cell">{l.itemName ?? l.description}</td>
                <td className="qb-grid-cell num">{formatQuantity(l.quantityE4)}</td>
                <td className="qb-grid-cell num">{formatQuantity(l.invoicedQtyE4)}</td>
                <td className="qb-grid-cell num">{formatQuantity(l.leftQtyE4)}</td>
                <td className="qb-grid-cell num">
                  <input
                    className={`qb-input w-[90px] text-right ${parsed[l.id] === null || parsed[l.id] > l.leftQtyE4 ? 'border-qb-negative' : ''}`}
                    aria-label={`To invoice, ${l.itemName ?? l.description}`}
                    value={qty[l.id]}
                    onChange={(e) => setQty((q) => ({ ...q, [l.id]: e.target.value }))}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {bad && <p className="text-sm text-qb-negative">Enter a quantity from 0 up to what is left on each line.</p>}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" disabled={Boolean(bad) || !any} onClick={() => onInvoice(Object.fromEntries(open.map((l) => [l.id, parsed[l.id]])))}>
            Open in invoice form
          </button>
        </div>
      </div>
    </div>
  );
}

export default function SalesOrders(props) {
  const { salesOrders, loaded } = useSalesPrefs();
  if (loaded && !salesOrders) {
    return (
      <div className="p-2">
        <Notice kind="info" text="Sales orders are turned off for this company. An Admin can turn them on in Preferences › Sales & Customers; orders already saved are kept." />
      </div>
    );
  }
  return <SalesOrderList {...props} />;
}

function SalesOrderList({ accounts, intent, navigate }) {
  const [openOnly, setOpenOnly] = useState(true);
  const { data, error, reload } = useLedgerQuery(() => api.listSalesOrders(openOnly));
  useEffect(() => {
    reload();
  }, [openOnly, reload]);
  const [tab, setTab] = useState('orders');
  const [editing, setEditing] = useState(intent?.action === 'sales-order' ? 'new' : null);
  const [partOf, setPartOf] = useState(null);
  const [notice, setNotice] = useState(null);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const catalog = useListReports();
  const emailed = useEmailStatuses('SALES_ORDER');

  useEffect(() => {
    if (intent?.action === 'sales-order') {
      setTab('orders');
      setEditing('new');
    }
  }, [intent]);

  const rows = data ?? [];
  const sorter = useSort(
    rows,
    {
      number: (o) => o.orderNumber,
      customer: (o) => o.customerName,
      date: (o) => o.orderDate,
      expected: (o) => o.expectedDate,
      amount: (o) => o.totalCents,
      open: (o) => o.openCents,
      status: (o) => STATUS[o.status].label,
    },
    { storageKey: 'sales-orders' },
  );

  const act = async (fn, text) => {
    try {
      await fn();
      if (text) setNotice({ kind: 'success', text });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const openOrder = (o) => act(async () => setEditing(await api.getSalesOrder(o.id)));
  const invoiceAll = (o) => navigate('sales/invoices', 'from-sales-order', { orderId: o.id });
  const invoicePart = (o) => act(async () => setPartOf(await api.getSalesOrder(o.id)));

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      <div className="flex flex-none gap-1" role="tablist" aria-label="Sales orders">
        {TABS.map(([id, label]) => (
          <button key={id} type="button" role="tab" aria-selected={tab === id} className={`qb-btn h-row ${tab === id ? 'qb-btn-primary' : 'qb-btn-secondary'}`} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </div>
      {tab !== 'orders' && <ListReport report={tab} catalog={catalog} navigate={navigate} />}
      {tab === 'orders' && (
        <>
          {editing && (
            <SalesOrderForm
              key={editing === 'new' ? 'new' : editing.id}
              accounts={accounts}
              order={editing === 'new' ? null : editing}
              onCancel={() => setEditing(null)}
              onSaved={(text) => {
                setNotice({ kind: 'success', text });
                setEditing(null);
              }}
            />
          )}
          {partOf && (
            <InvoicePartDialog
              order={partOf}
              onClose={() => setPartOf(null)}
              onInvoice={(quantities) => {
                setPartOf(null);
                navigate('sales/invoices', 'from-sales-order', { orderId: partOf.id, quantities });
              }}
            />
          )}
          {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
          {error && <Notice kind="error" text={`Could not load sales orders: ${error}`} />}
          <section className="qb-panel flex min-h-0 flex-1 flex-col">
            <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
              <h2 className="text-md font-semibold">Sales orders</h2>
              <span className="text-xs text-qb-ink-muted">Non-posting orders · ▾ to invoice all or part, close or reopen</span>
              <label className="ml-auto flex items-center gap-1 text-xs">
                <input type="checkbox" checked={openOnly} onChange={(e) => setOpenOnly(e.target.checked)} />
                Open only
              </label>
              {!editing && (
                <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setEditing('new')}>
                  New sales order
                </button>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
              <table className="qb-grid">
                <thead>
                  <tr>
                    <SortTh sorter={sorter} col="number">No.</SortTh>
                    <SortTh sorter={sorter} col="customer">Customer</SortTh>
                    <SortTh sorter={sorter} col="date">Date</SortTh>
                    <SortTh sorter={sorter} col="expected">Expected</SortTh>
                    <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
                    <SortTh sorter={sorter} col="open" className="text-right">Not invoiced</SortTh>
                    <SortTh sorter={sorter} col="status">Status</SortTh>
                    <th className="qb-grid-head" aria-label="Actions" />
                  </tr>
                </thead>
                <tbody>
                  {data && rows.length === 0 && (
                    <tr>
                      <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">{openOnly ? 'No open sales orders.' : 'No sales orders yet.'}</td>
                    </tr>
                  )}
                  {sorter.sorted.map((o) => {
                    const closed = o.status === 'CLOSED';
                    return (
                      <tr key={o.id} className="qb-grid-row cursor-pointer" onDoubleClick={() => openOrder(o)}>
                        <td className="qb-grid-cell">
                          {o.orderNumber}
                          <EmailMarks status={emailed.get(o.id)} />
                        </td>
                        <td className="qb-grid-cell font-semibold">{o.customerName}</td>
                        <td className="qb-grid-cell">{formatDate(o.orderDate)}</td>
                        <td className="qb-grid-cell">{o.expectedDate ? formatDate(o.expectedDate) : ''}</td>
                        <td className="qb-grid-cell num">{formatCents(o.totalCents)}</td>
                        <td className="qb-grid-cell num">{formatCents(o.openCents)}</td>
                        <td className={`qb-grid-cell ${STATUS[o.status].className}`}>{STATUS[o.status].label}</td>
                        <td className="qb-grid-cell cell-tight text-center">
                          <RowMenu
                            items={[
                              { label: 'Open', disabled: o.isClosed, hint: o.isClosed ? 'Reopen it to change it' : undefined, onSelect: () => openOrder(o) },
                              ...formMenuItems('SALES_ORDER', o.id, forms, emailed.get(o.id)),
                              { label: 'Create invoice for all that is left', disabled: closed, onSelect: () => invoiceAll(o) },
                              { label: 'Create invoice for part…', disabled: closed, onSelect: () => invoicePart(o) },
                              o.isClosed
                                ? { label: 'Reopen', onSelect: () => act(() => api.reopenSalesOrder(o.id), `Sales order #${o.orderNumber} reopened.`) }
                                : {
                                    label: 'Close',
                                    disabled: closed,
                                    confirm: { title: 'Close this sales order?', body: `Whatever is left on sales order #${o.orderNumber} won't be invoiced. You can reopen it later.`, confirmLabel: 'Close order' },
                                    onSelect: () => act(() => api.closeSalesOrder(o.id), `Sales order #${o.orderNumber} closed.`),
                                  },
                              {
                                label: 'Delete',
                                disabled: o.hasInvoices,
                                hint: o.hasInvoices ? 'Invoices were made from it; close it instead' : undefined,
                                confirm: { title: 'Delete this sales order?', body: `Sales order #${o.orderNumber} never posted, so nothing in the books changes.`, confirmLabel: 'Delete' },
                                onSelect: () => act(() => api.deleteSalesOrder(o.id), `Sales order #${o.orderNumber} deleted.`),
                              },
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
        </>
      )}
    </div>
  );
}
