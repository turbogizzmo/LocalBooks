import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import DateBox from './DateBox.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatQuantity, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { layOutCarried, purchaseLineMessage } from '../lib/purchaseLines.js';
import { percentText } from '../lib/itemTypes.js';

/**
 * Vendors › Receive Items and Enter Bill for Received Items (T12).
 *
 * Receive goods from an open purchase order before the vendor's bill comes: the stock is on hand and
 * can be sold, and the cost waits in Items Received Not Billed (not in Accounts Payable, so Pay Bills
 * doesn't offer it). When the bill arrives, Enter Bill turns the receipt's lines into the bill, at the
 * vendor's prices; a price that differs moves the difference to the stock (or Cost of Goods Sold, for
 * units already sold) or to the line's expense account.
 */
export default function ItemReceipts({ intent }) {
  const [notice, setNotice] = useState(null);
  const receipts = useLedgerQuery(() => api.listItemReceipts(false));
  const [billing, setBilling] = useState(null); // a receipt
  const [showAll, setShowAll] = useState(intent?.action !== 'bill-received');
  const shown = (receipts.data ?? []).filter((r) => showAll || (!r.isVoided && r.billId === null));

  const voidReceipt = async (r) => {
    if (!(await askConfirm(`Void item receipt #${r.receiptNumber}? Its stock leaves inventory and its cost leaves Items Received Not Billed.`, { confirmLabel: 'Void receipt', danger: true }))) return;
    try {
      await api.voidTransaction(r.journalEntryId);
      setNotice({ kind: 'success', text: `Item receipt #${r.receiptNumber} is voided.` });
      receipts.reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2">
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      <ReceiveForm
        onSaved={(r) => {
          setNotice({ kind: 'success', text: `Item receipt #${r.receiptNumber} recorded: ${formatCents(r.totalCents)} of goods from ${r.vendorName}, waiting for the bill.` });
          receipts.reload();
        }}
        onError={(text) => setNotice({ kind: 'error', text })}
      />
      <section className="qb-panel">
        <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Item receipts</h2>
          <label className="ml-auto flex items-center gap-1 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
            Show billed and voided
          </label>
        </div>
        {receipts.error && <Notice kind="error" text={errorText(receipts.error)} />}
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">No.</th>
              <th className="qb-grid-head">Date</th>
              <th className="qb-grid-head">Vendor</th>
              <th className="qb-grid-head">PO</th>
              <th className="qb-grid-head">Reference</th>
              <th className="qb-grid-head text-right">Amount</th>
              <th className="qb-grid-head">Status</th>
              <th className="qb-grid-head" />
            </tr>
          </thead>
          <tbody>
            {receipts.data && shown.length === 0 && (
              <tr>
                <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">
                  {showAll ? 'No items received yet.' : 'No receipts are waiting for their bill.'}
                </td>
              </tr>
            )}
            {shown.map((r) => (
              <tr key={r.id} className={`qb-grid-row ${r.isVoided ? 'text-qb-ink-faint' : ''}`}>
                <td className="qb-grid-cell">{r.receiptNumber}</td>
                <td className="qb-grid-cell">{formatDate(r.receiptDate)}</td>
                <td className="qb-grid-cell">{r.vendorName}</td>
                <td className="qb-grid-cell">{r.poNumber ? `#${r.poNumber}` : ''}</td>
                <td className="qb-grid-cell">{r.reference ?? ''}</td>
                <td className="qb-grid-cell num">{formatCents(r.totalCents)}</td>
                <td className="qb-grid-cell">{r.isVoided ? 'Voided' : r.billId !== null ? `Billed${r.billNumber ? ` (#${r.billNumber})` : ''}` : 'Waiting for bill'}</td>
                <td className="qb-grid-cell text-right">
                  {!r.isVoided && r.billId === null && (
                    <span className="flex justify-end gap-1">
                      <button type="button" className="qb-btn qb-btn-primary" onClick={() => setBilling(r)}>
                        Enter Bill
                      </button>
                      <button type="button" className="qb-btn qb-btn-secondary" onClick={() => voidReceipt(r)}>
                        Void
                      </button>
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      {billing && (
        <BillForm
          key={billing.id}
          receipt={billing}
          onCancel={() => setBilling(null)}
          onSaved={(text) => {
            setBilling(null);
            setNotice({ kind: 'success', text });
            receipts.reload();
          }}
        />
      )}
    </div>
  );
}

/** What is left to receive of an order line: quantity (× 10,000) or amount. */
function remaining(line) {
  return line.quantityE4 !== null ? Math.max(0, line.quantityE4 - line.receivedQuantityE4) : Math.max(0, line.amountCents - line.receivedAmountCents);
}

function ReceiveForm({ onSaved, onError }) {
  const orders = useLedgerQuery(() => api.listPurchaseOrders());
  const open = (orders.data ?? []).filter((o) => o.status === 'OPEN');
  const [orderId, setOrderId] = useState(null);
  const order = open.find((o) => o.id === orderId) ?? null;
  const [lines, setLines] = useState([]);
  const [now, setNow] = useState({}); // line id → text received now
  const [date, setDate] = useState(todayIso());
  const [reference, setReference] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    if (orderId === null) {
      setLines([]);
      return undefined;
    }
    api.listOrderReceiving(orderId).then(
      (rows) => {
        if (!live) return;
        setLines(rows);
        setNow(Object.fromEntries(rows.map((l) => [l.id, l.quantityE4 !== null ? formatQuantity(remaining(l)) : centsToInput(remaining(l))])));
      },
      (err) => live && onError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [orderId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Each line as it would be received, or why it can't be.
  const planned = useMemo(
    () =>
      lines.map((l) => {
        const text = (now[l.id] ?? '').trim();
        if (text === '' || text === '0') return { line: l, skip: true };
        if (l.quantityE4 !== null) {
          const qty = parseQuantityE4(text);
          const amount = qty === null ? null : quantityAmount(qty, l.rateCents ?? 0);
          return qty === null || amount === null ? { line: l, error: true } : { line: l, qty, amount, over: qty > remaining(l) };
        }
        const amount = parseMoneyToCents(text);
        return amount === null || amount <= 0 ? { line: l, error: true } : { line: l, amount, over: amount > remaining(l) };
      }),
    [lines, now],
  );
  const chosen = planned.filter((p) => !p.skip && !p.error);
  const total = chosen.reduce((s, p) => s + p.amount, 0);
  const invalid = planned.some((p) => p.error);

  const save = async () => {
    if (!order || chosen.length === 0 || invalid) return;
    const over = chosen.filter((p) => p.over);
    if (over.length > 0 && !(await askConfirm(`${over.length} line${over.length === 1 ? ' receives' : 's receive'} more than is left on the order. Receive them anyway?`, { confirmLabel: 'Receive anyway' }))) return;
    setBusy(true);
    try {
      const receipt = await api.receiveItems({
        vendorId: order.vendorId,
        purchaseOrderId: order.id,
        receiptDate: date,
        reference: reference.trim() || null,
        memo: null,
        lines: chosen.map((p) => ({
          purchaseOrderLineId: p.line.id,
          accountId: p.line.accountId,
          productServiceId: p.line.productServiceId,
          description: p.line.description,
          quantityE4: p.qty ?? null,
          rateCents: p.qty ? p.line.rateCents : null,
          amountCents: p.amount,
          classId: p.line.classId,
        })),
      });
      setOrderId(null);
      setReference('');
      onSaved(receipt);
      orders.reload();
    } catch (err) {
      onError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1.5">
        <h2 className="self-center text-md font-semibold">Receive Items</h2>
        <label>
          <span className="qb-label">Purchase order</span>
          <select className="qb-input w-[260px]" value={orderId ?? ''} onChange={(e) => setOrderId(e.target.value === '' ? null : Number(e.target.value))}>
            <option value="">{open.length === 0 ? 'No open purchase orders' : 'Choose an open order…'}</option>
            {open.map((o) => (
              <option key={o.id} value={o.id}>
                #{o.poNumber} · {o.vendorName} · {formatCents(o.totalCents)}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Date received</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Packing slip / ref.</span>
          <input className="qb-input w-[120px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <span className="ml-auto self-center text-sm">
          <span className="text-qb-ink-muted">Received </span>
          <span className="num font-semibold">{formatCents(total)}</span>
        </span>
        <button type="button" className="qb-btn qb-btn-primary" disabled={busy || !order || chosen.length === 0 || invalid} onClick={save}>
          {busy ? 'Saving…' : 'Receive'}
        </button>
      </div>
      {order && (
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Item</th>
              <th className="qb-grid-head">Description</th>
              <th className="qb-grid-head text-right">Ordered</th>
              <th className="qb-grid-head text-right">Received</th>
              <th className="qb-grid-head text-right">Receive now</th>
              <th className="qb-grid-head text-right">Amount</th>
            </tr>
          </thead>
          <tbody>
            {planned.map((p) => {
              const l = p.line;
              const byQty = l.quantityE4 !== null;
              return (
                <tr key={l.id} className="qb-grid-row">
                  <td className="qb-grid-cell">{l.productName ?? ''}</td>
                  <td className="qb-grid-cell">{l.description ?? ''}</td>
                  <td className="qb-grid-cell num">{byQty ? formatQuantity(l.quantityE4) : formatCents(l.amountCents)}</td>
                  <td className="qb-grid-cell num">{byQty ? formatQuantity(l.receivedQuantityE4) : formatCents(l.receivedAmountCents)}</td>
                  <td className="qb-grid-cell num">
                    <input
                      className="qb-input num w-[90px]"
                      value={now[l.id] ?? ''}
                      onChange={(e) => setNow({ ...now, [l.id]: e.target.value })}
                      aria-label={`Receive now: ${l.productName ?? l.description ?? 'line'}`}
                      aria-invalid={Boolean(p.error)}
                      inputMode="decimal"
                    />
                    {p.over && <span className="ml-1 text-2xs text-qb-negative" title="More than is left on the order">over</span>}
                  </td>
                  <td className="qb-grid-cell num">{p.amount ? formatCents(p.amount) : ''}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** Enter Bill for Received Items: the receipt's lines at the vendor's prices. */
function BillForm({ receipt, onCancel, onSaved }) {
  const [lines, setLines] = useState(null);
  const [amounts, setAmounts] = useState({});
  const [billNumber, setBillNumber] = useState('');
  const [billDate, setBillDate] = useState(todayIso());
  const [dueDate, setDueDate] = useState(addDays(todayIso(), 30));
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  // The purchase order's discounts and charges this bill takes (T21).
  const [carried, setCarried] = useState([]);

  useEffect(() => {
    api.getReceiptBillFormLines(receipt.id).then(setCarried, () => setCarried([]));
    api.listItemReceiptLines(receipt.id).then(
      (rows) => {
        setLines(rows);
        setAmounts(Object.fromEntries(rows.map((l) => [l.id, centsToInput(l.amountCents)])));
      },
      (err) => setError(errorText(err)),
    );
  }, [receipt.id]);

  const parsed = (lines ?? []).map((l) => ({ line: l, cents: parseMoneyToCents(amounts[l.id] ?? '') }));
  const invalid = parsed.some((p) => p.cents === null || p.cents <= 0);
  const laid = invalid ? null : layOutCarried(parsed.map((p) => p.cents), carried);
  const total = !laid ? null : laid.error ? null : laid.total;
  // The price difference on the lines themselves, before the order's discounts and charges.
  const difference = invalid ? null : parsed.reduce((s, p) => s + p.cents, 0) - receipt.totalCents;

  const save = async () => {
    if (invalid || !lines) return;
    if (laid?.error) return setError(purchaseLineMessage(laid.error));
    if (dueDate < billDate) return setError('The due date is before the bill date.');
    setBusy(true);
    try {
      await api.billReceivedItems({
        itemReceiptId: receipt.id,
        billNumber: billNumber.trim() || null,
        billDate,
        dueDate,
        termsId: null,
        memo: null,
        lines: parsed.map((p) => ({ receiptLineId: p.line.id, amountCents: p.cents })),
      });
      onSaved(
        `Bill for item receipt #${receipt.receiptNumber} entered: ${formatCents(total)} owed to ${receipt.vendorName}` +
          (difference ? `, ${formatCents(Math.abs(difference))} ${difference > 0 ? 'more' : 'less'} than received.` : '.'),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel" aria-label="Enter Bill for Received Items">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1.5">
        <h2 className="self-center text-md font-semibold">
          Enter Bill for Received Items · receipt #{receipt.receiptNumber}, {receipt.vendorName}
        </h2>
        <label>
          <span className="qb-label">Ref. no.</span>
          <input className="qb-input w-[110px]" value={billNumber} onChange={(e) => setBillNumber(e.target.value)} autoFocus />
        </label>
        <label>
          <span className="qb-label">Bill date</span>
          <DateBox className="qb-input w-[132px]" value={billDate} onChange={(e) => setBillDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Due date</span>
          <DateBox className="qb-input w-[132px]" value={dueDate} onChange={(e) => setDueDate(e.target.value)} />
        </label>
        <span className="ml-auto self-center text-sm">
          <span className="text-qb-ink-muted">Bill total </span>
          <span className="num font-semibold">{total === null ? '—' : formatCents(total)}</span>
          {difference ? <span className="ml-2 text-xs text-qb-ink-muted">({difference > 0 ? '+' : '−'}{formatCents(Math.abs(difference))} on the receipt)</span> : null}
        </span>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="qb-btn qb-btn-primary" disabled={busy || invalid || !lines} onClick={save}>
          {busy ? 'Posting…' : 'Save & Post Bill'}
        </button>
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {lines && (
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Item</th>
              <th className="qb-grid-head">Description</th>
              <th className="qb-grid-head text-right">Qty</th>
              <th className="qb-grid-head text-right">Received at</th>
              <th className="qb-grid-head text-right">Billed amount</th>
            </tr>
          </thead>
          <tbody>
            {parsed.map(({ line: l, cents }) => (
              <tr key={l.id} className="qb-grid-row">
                <td className="qb-grid-cell">{l.productName ?? ''}</td>
                <td className="qb-grid-cell">{l.description}</td>
                <td className="qb-grid-cell num">{l.quantityE4 !== null ? formatQuantity(l.quantityE4) : ''}</td>
                <td className="qb-grid-cell num">{formatCents(l.amountCents)}</td>
                <td className="qb-grid-cell num">
                  <input
                    className="qb-input num w-[110px]"
                    value={amounts[l.id] ?? ''}
                    onChange={(e) => setAmounts({ ...amounts, [l.id]: e.target.value })}
                    aria-label={`Billed amount: ${l.productName ?? l.description}`}
                    aria-invalid={cents === null || cents <= 0}
                    inputMode="decimal"
                    data-money
                  />
                </td>
              </tr>
            ))}
            {laid && !laid.error && laid.lines.map((f, i) => (
              <tr key={`f${i}`} className="qb-grid-row text-qb-ink-muted">
                <td className="qb-grid-cell">{f.kind === 'DISCOUNT' ? 'Discount' : 'Charge'}</td>
                <td className="qb-grid-cell">{f.description}{f.percentPpm ? ` (${percentText(f.percentPpm)} of line ${f.afterLine})` : ''}</td>
                <td className="qb-grid-cell" />
                <td className="qb-grid-cell" />
                <td className="qb-grid-cell num">{formatCents(f.kind === 'DISCOUNT' ? -laid.amounts[i] : laid.amounts[i])}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {laid && !laid.error && laid.lines.length > 0 && (
        <p className="px-2 py-1 text-xs text-qb-ink-muted">The purchase order&apos;s discounts and charges come over to this bill.</p>
      )}
      {laid && !laid.error && laid.held.map(({ line: f, afterLines }) => (
        <p key={`h${f.source}`} className="px-2 py-1 text-xs text-qb-ink-muted">
          {`${f.description || 'Discount'} (${formatCents(f.amountCents)}) is more than ${afterLines.length > 1 ? 'lines' : 'line'} ${afterLines.join(' and ')} on this bill, so it waits for a later bill for that order line. If no more is coming, enter it as a vendor credit.`}
        </p>
      ))}
    </section>
  );
}
