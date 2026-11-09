// File › Print Forms › Credit Memos…, Sales Receipts…, Purchase Orders…, Bill Payment Stubs… and
// Mailing Labels…: pick the documents (or names) and print them in one go.
import { Fragment, useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import DateBox from './DateBox.jsx';
import { FormSheet, useFormPrinter } from './FormPages.jsx';
import { PrintPortal, useCompany } from './PrintDocuments.jsx';
import { PurchaseOrderDocument } from './PurchaseOrders.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, periodRange } from '../lib/dates.js';

/** The batches, by the File › Print Forms action that opens each. */
export const BATCHES = {
  'credit-memos': { kind: 'CREDIT_MEMO', title: 'Credit Memos', one: 'credit memo', party: 'Customer', number: 'No.' },
  'sales-receipts': { kind: 'SALES_RECEIPT', title: 'Sales Receipts', one: 'sales receipt', party: 'Customer', number: 'No.' },
  'purchase-orders': { kind: 'PURCHASE_ORDER', title: 'Purchase Orders', one: 'purchase order', party: 'Vendor', number: 'PO no.' },
  'bill-payments': { kind: 'BILL_PAYMENT', title: 'Bill Payment Stubs', one: 'payment stub', party: 'Vendor', number: 'Check no.' },
};

/** Ticks, starting with every row ticked (voided ones left out); resets when the rows change. */
function usePicker(rows) {
  const [picked, setPicked] = useState(() => new Set());
  const key = rows.map((r) => r.id).join(',');
  useEffect(() => setPicked(new Set(rows.filter((r) => !r.isVoided).map((r) => r.id))), [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (id) =>
    setPicked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  // "All" means every row that isn't voided: a voided one is ticked only by hand.
  const live = rows.filter((r) => !r.isVoided);
  const all = live.length > 0 && live.every((r) => picked.has(r.id));
  const toggleAll = () => setPicked(all ? new Set() : new Set(live.map((r) => r.id)));
  return { picked, toggle, all, toggleAll, chosen: rows.filter((r) => picked.has(r.id)) };
}

/** Purchase orders print from their screen's own document, one per sheet. */
function usePurchaseOrderPrinter(onNotice, accounts) {
  const company = useCompany();
  const [orders, setOrders] = useState(null);
  useEffect(() => {
    if (!orders) return undefined;
    const clear = () => setOrders(null);
    window.addEventListener('afterprint', clear);
    const timer = setTimeout(() => printDocument(orders.length === 1 ? `PO ${orders[0].order.poNumber}` : 'Purchase orders'), 120);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('afterprint', clear);
    };
  }, [orders]);
  const print = async (ids) => {
    try {
      const all = await api.listPurchaseOrders();
      const chosen = ids.map((id) => all.find((o) => o.id === id)).filter(Boolean);
      setOrders(await Promise.all(chosen.map(async (order) => ({ order, lines: await api.getPurchaseOrderLines(order.id) }))));
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const portal = orders && (
    <PrintPortal>
      {orders.map(({ order, lines }, i) => (
        <div key={order.id} style={i ? { breakBefore: 'page' } : undefined}>
          <PurchaseOrderDocument company={company} order={order} lines={lines} accounts={accounts} />
        </div>
      ))}
    </PrintPortal>
  );
  return [portal, print];
}

const RANGES = [
  ['thisMonth', 'This month'],
  ['lastMonth', 'Last month'],
  ['thisQuarter', 'This quarter'],
  ['thisYear', 'This year'],
];

/** A batch of one kind of document from a date range. */
export function PrintBatch({ action, accounts }) {
  const [which, setWhich] = useState(action in BATCHES ? action : 'credit-memos');
  const batch = BATCHES[which];
  const [[from, to], setRange] = useState(() => periodRange('thisMonth'));
  const { data, error, reload } = useLedgerQuery(() => api.listPrintBatch(batch.kind, from, to));
  // The query reloads when the books change; the kind and dates are this screen's own.
  useEffect(() => {
    reload();
  }, [batch.kind, from, to]); // eslint-disable-line react-hooks/exhaustive-deps
  const rows = useMemo(() => data ?? [], [data]);
  const picker = usePicker(rows);
  const [notice, setNotice] = useState(null);
  const [formPortal, forms] = useFormPrinter(setNotice);
  const [poPortal, printOrders] = usePurchaseOrderPrinter(setNotice, accounts);

  const print = () => {
    if (picker.chosen.length === 0) return setNotice({ kind: 'error', text: `Tick the ${batch.one}s to print.` });
    setNotice(null);
    if (batch.kind === 'PURCHASE_ORDER') return printOrders(picker.chosen.map((r) => r.id));
    return forms.printMany(picker.chosen.map((r) => ({ kind: batch.kind, id: r.id })));
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {formPortal}
      {poPortal}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the ${batch.one}s: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none flex-wrap items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Print</h2>
          <select className="qb-input h-row" aria-label="Forms" value={which} onChange={(e) => setWhich(e.target.value)}>
            {Object.entries(BATCHES).map(([id, b]) => (
              <option key={id} value={id}>{b.title}</option>
            ))}
          </select>
          <select className="qb-input h-row" aria-label="Dates" defaultValue="thisMonth" onChange={(e) => e.target.value && setRange(periodRange(e.target.value))}>
            {RANGES.map(([id, label]) => (
              <option key={id} value={id}>{label}</option>
            ))}
            <option value="">Custom</option>
          </select>
          <DateBox className="qb-input h-row w-[120px]" aria-label="From" value={from} onChange={(e) => setRange([e.target.value, to])} />
          <span className="text-xs text-qb-ink-muted">to</span>
          <DateBox className="qb-input h-row w-[120px]" aria-label="To" value={to} onChange={(e) => setRange([from, e.target.value])} />
          <span className="text-xs text-qb-ink-muted">
            {picker.chosen.length} of {rows.length} selected
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={print} disabled={picker.chosen.length === 0}>
            Print…
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[34px]" />
              <col className="w-[92px]" />
              <col className="w-[92px]" />
              <col />
              <col className="w-[120px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input type="checkbox" className="accent-qb-green" checked={picker.all} onChange={picker.toggleAll} aria-label="Select all" />
                </th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">{batch.number}</th>
                <th className="qb-grid-head">{batch.party}</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No {batch.one}s are dated in that range.</td>
                </tr>
              )}
              {rows.map((r) => (
                <tr key={r.id} className={`qb-grid-row cursor-pointer ${r.isVoided ? 'text-qb-ink-faint' : ''}`} onClick={() => picker.toggle(r.id)}>
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" className="accent-qb-green" checked={picker.picked.has(r.id)} onChange={() => picker.toggle(r.id)} onClick={(e) => e.stopPropagation()} aria-label={`Print ${batch.one} ${r.number ?? ''}`.trim()} />
                  </td>
                  <td className="qb-grid-cell">{formatDate(r.date)}</td>
                  <td className="qb-grid-cell">{r.number ?? ''}</td>
                  <td className="qb-grid-cell font-semibold">
                    {r.name}
                    {r.isVoided && <span className="ml-1 text-2xs font-normal">(voided)</span>}
                  </td>
                  <td className="qb-grid-cell num">{formatCents(r.totalCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

/** Mailing labels: 30 to a Letter sheet, 2⅝ × 1 inch; printed edge to edge so they line up. */
export function MailingLabels() {
  const [names, setNames] = useState('CUSTOMERS');
  const [withAddress, setWithAddress] = useState(true);
  const [skip, setSkip] = useState(0);
  const { data, error, reload } = useLedgerQuery(() => api.listLabelNames(names));
  useEffect(() => {
    reload();
  }, [names]); // eslint-disable-line react-hooks/exhaustive-deps
  const rows = useMemo(() => (data ?? []).filter((r) => !withAddress || r.hasAddress), [data, withAddress]);
  const picker = usePicker(rows);
  const [notice, setNotice] = useState(null);
  const [sheets, setSheets] = useState(null);

  useEffect(() => {
    if (!sheets) return undefined;
    const style = document.createElement('style');
    style.textContent = '@page { size: letter; margin: 0; }';
    document.head.appendChild(style);
    const clear = () => setSheets(null);
    window.addEventListener('afterprint', clear);
    const timer = setTimeout(() => printDocument(sheets.title), 80);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('afterprint', clear);
      style.remove();
    };
  }, [sheets]);

  const print = async () => {
    if (picker.chosen.length === 0) return setNotice({ kind: 'error', text: 'Tick the names to print labels for.' });
    setNotice(null);
    try {
      return setSheets(await api.renderMailingLabels(names, picker.chosen.map((r) => r.id), skip));
    } catch (err) {
      return setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {sheets && (
        <PrintPortal>
          {sheets.pages.map((page, i) => (
            <FormSheet key={i} page={page} logo={null} className="envelope-sheet" />
          ))}
        </PrintPortal>
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load the names: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none flex-wrap items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Print mailing labels</h2>
          <select className="qb-input h-row" aria-label="Names" value={names} onChange={(e) => setNames(e.target.value)}>
            <option value="CUSTOMERS">Customers</option>
            <option value="VENDORS">Vendors</option>
          </select>
          <label className="flex items-center gap-1 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={withAddress} onChange={(e) => setWithAddress(e.target.checked)} />
            Only names with an address
          </label>
          <label className="flex items-center gap-1 text-sm" title="Labels already peeled off the first sheet: printing starts after them, across then down">
            Skip
            <input type="number" min={0} max={29} className="qb-input h-row w-[56px]" value={skip} onChange={(e) => setSkip(Math.max(0, Math.min(29, Number.parseInt(e.target.value || '0', 10) || 0)))} />
            used labels
          </label>
          <span className="text-xs text-qb-ink-muted">
            {picker.chosen.length} of {rows.length} · 30 per Letter sheet, 2⅝ × 1 in
          </span>
          <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={print} disabled={picker.chosen.length === 0}>
            Print…
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[34px]" />
              <col className="w-[240px]" />
              <col />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head text-center">
                  <input type="checkbox" className="accent-qb-green" checked={picker.all} onChange={picker.toggleAll} aria-label="Select all" />
                </th>
                <th className="qb-grid-head">Name</th>
                <th className="qb-grid-head">Label</th>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={3} className="qb-grid-cell text-qb-ink-muted">No active {names === 'CUSTOMERS' ? 'customers' : 'vendors'}{withAddress ? ' with an address' : ''}.</td>
                </tr>
              )}
              {rows.map((r) => (
                <tr key={r.id} className="qb-grid-row cursor-pointer" onClick={() => picker.toggle(r.id)}>
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" className="accent-qb-green" checked={picker.picked.has(r.id)} onChange={() => picker.toggle(r.id)} onClick={(e) => e.stopPropagation()} aria-label={`Label for ${r.name}`} />
                  </td>
                  <td className="qb-grid-cell font-semibold">{r.name}</td>
                  <td className="qb-grid-cell text-qb-ink-muted" title={r.lines.join('\n')}>
                    {r.lines.slice(1).map((line, i) => (
                      <Fragment key={i}>
                        {i ? ' · ' : ''}
                        {line}
                      </Fragment>
                    ))}
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

