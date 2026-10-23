import { useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { isCategory } from './Bills.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { accountLabel } from './AccountSelect.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const STATUS = { OPEN: 'Open', CLOSED: 'Closed', BILLED: 'Billed' };

function PurchaseOrderDocument({ company, order, lines, accounts }) {
  return (
    <article className="doc">
      <DocHeader company={company} title="Purchase Order" subtitle={`PO #${order.poNumber} · ${formatDate(order.poDate)}`} />
      <div className="doc-bill-to">
        <div className="doc-label">Vendor</div>
        <div className="doc-company-name">{order.vendorName}</div>
        {order.expectedDate && <div>Expected by {formatDate(order.expectedDate)}</div>}
      </div>
      <table className="doc-table">
        <thead>
          <tr>
            <th>Item / account</th>
            <th>Description</th>
            <th className="doc-num">Qty</th>
            <th className="doc-num">Rate</th>
            <th className="doc-num">Amount</th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => (
            <tr key={i}>
              <td>{accountLabel(accounts, l.accountId)}</td>
              <td>{l.description ?? ''}</td>
              <td className="doc-num">{l.quantityE4 ? formatQuantity(l.quantityE4) : ''}</td>
              <td className="doc-num">{l.rateCents ? formatCents(l.rateCents) : ''}</td>
              <td className="doc-num">{formatCents(l.amountCents)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <table className="doc-totals">
        <tbody>
          <tr className="doc-grand">
            <td>Total</td>
            <td className="doc-num">{formatCents(order.totalCents)}</td>
          </tr>
        </tbody>
      </table>
      {order.memo && <p className="doc-note">{order.memo}</p>}
    </article>
  );
}

function NewPurchaseOrder({ accounts, vendors, defaultCategory, onSaved, onCancel }) {
  const stockItems = useLedgerQuery(api.listInventoryItems);
  const [vendor, setVendor] = useState('');
  const [poDate, setPoDate] = useState(todayIso);
  const [expected, setExpected] = useState('');
  const [memo, setMemo] = useState('');
  const [rows, setRows] = useState(() => [blankSplit(defaultCategory), blankSplit(defaultCategory)]);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const formClass = useFormClass(accounts);
  const { lines, invalid, total } = summarizeSplits(rows);

  const save = async () => {
    if (!vendor.trim()) return setError('Enter a vendor.');
    if (invalid > 0) return setError(`${invalid} row(s) need an account and an amount above zero.`);
    if (lines.length === 0) return setError('Add at least one line.');
    const posted = formClass.apply(lines).map(({ memo: description, ...l }) => ({ ...l, description }));
    setBusy(true);
    try {
      await api.createPurchaseOrder({ vendorName: vendor.trim(), poDate, expectedDate: expected || null, memo: memo.trim() || null, lines: posted });
      onSaved(`Purchase order for ${vendor.trim()} saved: ${formatCents(total)}. It does not post to the books until you turn it into a bill.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New purchase order</h2>
        <label>
          <span className="qb-label">Vendor</span>
          <input className="qb-input w-[200px]" value={vendor} onChange={(e) => setVendor(e.target.value)} list="po-vendors" autoFocus />
          <datalist id="po-vendors">
            {vendors.map((v) => (
              <option key={v.id} value={v.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">PO date</span>
          <input type="date" className="qb-input w-[132px]" value={poDate} onChange={(e) => setPoDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Expected</span>
          <input type="date" className="qb-input w-[132px]" value={expected} onChange={(e) => setExpected(e.target.value)} />
        </label>
        {formClass.header}
        <div className="ml-auto self-center text-sm">
          <span className="text-qb-ink-muted">Total </span>
          <span className="num font-semibold">{formatCents(total)}</span>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} withPricing withClasses stockItems={stockItems.data ?? []} />
      <div className="flex items-center gap-2 px-2 py-1">
        <input className="qb-input w-[320px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Message to vendor" />
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>Close</button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>Save purchase order</button>
        </div>
      </div>
    </section>
  );
}

function BillDialog({ order, onClose, onDone }) {
  const [billDate, setBillDate] = useState(todayIso);
  const [dueDate, setDueDate] = useState(() => addDays(todayIso(), 30));
  const [number, setNumber] = useState('');
  const [error, setError] = useState(null);
  const create = async () => {
    try {
      await api.billPurchaseOrder(order.id, billDate, dueDate, number.trim() || null);
      onDone(`PO #${order.poNumber} turned into a bill from ${order.vendorName} for ${formatCents(order.totalCents)}; the bill is posted to Accounts Payable.`);
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Create bill">
      <div className="qb-panel w-[380px] p-3 shadow-panel">
        <h2 className="mb-2 text-md font-semibold">Create bill from PO #{order.poNumber}</h2>
        <div className="grid grid-cols-2 gap-2">
          <label>
            <span className="qb-label">Bill date</span>
            <input type="date" className="qb-input w-full" value={billDate} onChange={(e) => { setBillDate(e.target.value); setDueDate(addDays(e.target.value, 30)); }} />
          </label>
          <label>
            <span className="qb-label">Due date</span>
            <input type="date" className="qb-input w-full" value={dueDate} onChange={(e) => setDueDate(e.target.value)} />
          </label>
          <label className="col-span-2">
            <span className="qb-label">Vendor’s bill no.</span>
            <input className="qb-input w-full" value={number} onChange={(e) => setNumber(e.target.value)} />
          </label>
        </div>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose}>Cancel</button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={create} disabled={dueDate < billDate}>Create bill</button>
        </div>
      </div>
    </div>
  );
}

/** Expenses › Purchase Orders: non-posting orders to vendors, later turned into bills. */
export default function PurchaseOrders({ accounts, intent }) {
  const company = useCompany();
  const { data, error, reload } = useLedgerQuery(api.listPurchaseOrders);
  const vendors = useLedgerQuery(api.listVendors);
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && isCategory(a) && a.classification === 'EXPENSE')?.id ?? null, [accounts]);
  const [composing, setComposing] = useState(intent?.action === 'purchase-order');
  const [notice, setNotice] = useState(null);
  const [billing, setBilling] = useState(null);
  const [printing, setPrinting] = useState(null);
  const [filter, setFilter] = useState('OPEN');
  const list = (data ?? []).filter((o) => filter === 'ALL' || o.status === filter);

  const print = async (order) => {
    try {
      const lines = await api.getPurchaseOrderLines(order.id);
      setPrinting({ order, lines });
      setTimeout(() => printDocument(), 120);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const setClosed = async (order, closed) => {
    try {
      await api.setPurchaseOrderClosed(order.id, closed);
      setNotice({ kind: 'success', text: `PO #${order.poNumber} ${closed ? 'closed' : 'reopened'}.` });
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printing && (
        <PrintPortal>
          <PurchaseOrderDocument company={company} order={printing.order} lines={printing.lines} accounts={accounts} />
        </PrintPortal>
      )}
      {composing ? (
        <NewPurchaseOrder
          accounts={accounts}
          vendors={vendors.data ?? []}
          defaultCategory={defaultCategory}
          onCancel={() => setComposing(false)}
          onSaved={(text) => {
            setComposing(false);
            setNotice({ kind: 'success', text });
            reload();
          }}
        />
      ) : null}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {billing && (
        <BillDialog
          order={billing}
          onClose={() => setBilling(null)}
          onDone={(text) => {
            setBilling(null);
            setNotice({ kind: 'success', text });
            reload();
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Purchase orders</h2>
          <select className="qb-input h-row w-[120px]" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Show">
            <option value="OPEN">Open</option>
            <option value="BILLED">Billed</option>
            <option value="CLOSED">Closed</option>
            <option value="ALL">All</option>
          </select>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New purchase order
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">PO no.</th>
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Vendor</th>
                <th className="qb-grid-head">Expected</th>
                <th className="qb-grid-head">Memo</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head num">Total</th>
                <th className="qb-grid-head w-[36px]" />
              </tr>
            </thead>
            <tbody>
              {data && list.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No purchase orders to show.</td>
                </tr>
              )}
              {list.map((o) => (
                <tr key={o.id} className="qb-grid-row">
                  <td className="qb-grid-cell font-semibold">{o.poNumber}</td>
                  <td className="qb-grid-cell">{formatDate(o.poDate)}</td>
                  <td className="qb-grid-cell">{o.vendorName}</td>
                  <td className="qb-grid-cell">{o.expectedDate ? formatDate(o.expectedDate) : ''}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">{o.memo ?? ''}</td>
                  <td className="qb-grid-cell">{STATUS[o.status] ?? o.status}</td>
                  <td className="qb-grid-cell num">{formatCents(o.totalCents)}</td>
                  <td className="qb-grid-cell">
                    <RowMenu
                      label={`Actions for PO ${o.poNumber}`}
                      items={[
                        { label: 'Create bill…', disabled: o.status !== 'OPEN', onSelect: () => setBilling(o) },
                        { label: 'Print or save as PDF', onSelect: () => print(o) },
                        o.status === 'CLOSED'
                          ? { label: 'Reopen', onSelect: () => setClosed(o, false) }
                          : { label: 'Close without billing', disabled: o.status !== 'OPEN', onSelect: () => setClosed(o, true) },
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
