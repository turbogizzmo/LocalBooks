import { useEffect, useRef, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents, formatQuantity, mulDivRound, parseMoneyToCents, parseQuantityE4 } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

const KIND = { PURCHASE: 'Purchase', SALE: 'Sale', ADJUSTMENT: 'Adjustment', VOID: 'Void' };

function Dialog({ title, children, onClose }) {
  const ref = useRef(null);
  useEffect(() => {
    ref.current?.querySelector('input, select')?.focus();
    const onKey = (e) => e.key === 'Escape' && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center bg-black/30 p-6" role="dialog" aria-modal="true" aria-label={title}>
      <div ref={ref} className="qb-panel flex max-h-full w-full max-w-[620px] flex-col shadow-lg">
        <h2 className="border-b border-qb-border px-3 py-2 text-md font-semibold">{title}</h2>
        {children}
      </div>
    </div>
  );
}

/** Starts tracking stock for a product: optional SKU; the default inventory and COGS accounts are used. */
export function TrackDialog({ product, onClose, onDone }) {
  const [sku, setSku] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.trackInventory({ productId: product.id, sku: sku.trim() || null });
      onDone(`Now tracking stock for ${product.name}. Record opening stock with Adjust, or buy it on a bill.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog title={`Track stock for ${product.name}`} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <p className="text-qb-ink-muted">
          LocalBooks will keep {product.name}’s quantity on hand and its cost at moving average. Buying it on a bill adds stock; selling it on an
          invoice or sales receipt records its cost in <strong>Cost of Goods Sold</strong>. It starts at zero on hand.
        </p>
        <label className="flex w-[220px] flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">SKU (optional)</span>
          <input className="qb-input" value={sku} onChange={(e) => setSku(e.target.value)} placeholder="e.g. W-100" />
        </label>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Track stock</button>
        </div>
      </form>
    </Dialog>
  );
}

/** Opening stock, counts and write-offs, plus the item's stock history. */
export function AdjustDialog({ item, accounts, onClose, onDone }) {
  const obe = accounts.find((a) => a.isSystem && a.fullName === 'Opening Balance Equity')?.id ?? null;
  const cogs = item.cogsAccountId;
  const [direction, setDirection] = useState('add');
  const [qty, setQty] = useState('');
  const [cost, setCost] = useState('');
  const [date, setDate] = useState(todayIso());
  const [offset, setOffset] = useState(obe);
  const [memo, setMemo] = useState('');
  const [history, setHistory] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    api.listStockMovements(item.id).then(setHistory, (err) => setError(errorText(err)));
  }, [item.id]);
  useEffect(() => setOffset(direction === 'add' ? obe : cogs), [direction, obe, cogs]);

  const qtyE4 = parseQuantityE4(qty);
  const costCents = cost.trim() === '' ? null : parseMoneyToCents(cost);
  // Same rule as the ledger: the last units take exactly the remaining value.
  const removeCost = !qtyE4 || item.qtyOnHandE4 <= 0 ? null : qtyE4 === item.qtyOnHandE4 ? item.valueCents : mulDivRound(item.valueCents, qtyE4, item.qtyOnHandE4);
  const valid = qtyE4 && offset && (direction === 'remove' ? qtyE4 <= item.qtyOnHandE4 : costCents !== null && costCents > 0);

  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.adjustInventory({
        itemId: item.id,
        date,
        qtyChangeE4: direction === 'add' ? qtyE4 : -qtyE4,
        totalCostCents: direction === 'add' ? costCents : null,
        offsetAccountId: offset,
        memo: memo.trim() || null,
      });
      onDone(`${item.name}: ${direction === 'add' ? 'added' : 'removed'} ${formatQuantity(qtyE4)}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={`Adjust stock · ${item.name}`} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="flex flex-wrap gap-x-4 text-sm">
          <span>On hand <strong className="num">{formatQuantity(item.qtyOnHandE4)}</strong></span>
          <span>Value <strong className="num">{formatCents(item.valueCents)}</strong></span>
          <span>Average cost <strong className="num">{item.averageCostCents !== null ? formatCents(item.averageCostCents) : '—'}</strong></span>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <fieldset className="flex flex-col gap-0.5">
            <legend className="mb-0.5 font-semibold text-qb-ink-muted">Change</legend>
            <label className="flex items-center gap-1.5"><input type="radio" checked={direction === 'add'} onChange={() => setDirection('add')} className="accent-qb-green" /> Add stock (opening stock, found units)</label>
            <label className="flex items-center gap-1.5"><input type="radio" checked={direction === 'remove'} onChange={() => setDirection('remove')} className="accent-qb-green" /> Remove stock (count short, damaged, used)</label>
          </fieldset>
          <label className="flex w-[90px] flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Quantity</span>
            <input className="qb-input num" value={qty} onChange={(e) => setQty(e.target.value)} inputMode="decimal" placeholder="0" />
          </label>
          {direction === 'add' ? (
            <label className="flex w-[120px] flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Total cost</span>
              <input className="qb-input num" value={cost} onChange={(e) => setCost(e.target.value)} inputMode="decimal" placeholder="0.00" />
            </label>
          ) : (
            <div className="flex w-[120px] flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">At average cost</span>
              <span className="num py-1">{removeCost !== null ? formatCents(removeCost) : '—'}</span>
            </div>
          )}
          <label className="flex w-[132px] flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Date</span>
            <input type="date" className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
          </label>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex min-w-[260px] flex-1 flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{direction === 'add' ? 'Balancing account (usually Opening Balance Equity)' : 'Expense account for the loss'}</span>
            <AccountSelect accounts={accounts} value={offset} onChange={setOffset} filter={(a) => a.id !== item.assetAccountId && a.accountType !== 'BANK' && a.accountType !== 'ACCOUNTS_RECEIVABLE' && a.accountType !== 'ACCOUNTS_PAYABLE'} />
          </label>
          <label className="flex min-w-[180px] flex-1 flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Memo</span>
            <input className="qb-input" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder={direction === 'add' ? 'Opening stock' : 'Stock count'} />
          </label>
        </div>
        {direction === 'remove' && qtyE4 > item.qtyOnHandE4 && <Notice kind="error" text={`Only ${formatQuantity(item.qtyOnHandE4)} on hand.`} />}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Close</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || !valid}>Save & Post Adjustment</button>
        </div>
      </form>
      <div className="min-h-0 flex-1 overflow-auto border-t border-qb-border scrollbar-thin">
        <div className="px-3 pt-2 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Stock history</div>
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Date</th>
              <th className="qb-grid-head">Type</th>
              <th className="qb-grid-head">Entry</th>
              <th className="qb-grid-head text-right">Qty</th>
              <th className="qb-grid-head text-right">Value</th>
              <th className="qb-grid-head text-right">On hand</th>
            </tr>
          </thead>
          <tbody>
            {history?.length === 0 && (
              <tr><td colSpan={6} className="qb-grid-cell text-qb-ink-muted">No stock movements yet.</td></tr>
            )}
            {(history ?? []).map((m) => (
              <tr key={m.id}>
                <td className="qb-grid-cell">{formatDate(m.movementDate)}</td>
                <td className="qb-grid-cell">{KIND[m.kind] ?? m.kind}</td>
                <td className="qb-grid-cell" title={m.memo ?? ''}>#{m.entryNumber}</td>
                <td className={`qb-grid-cell num ${m.qtyChangeE4 < 0 ? 'text-qb-negative' : ''}`}>{m.qtyChangeE4 > 0 ? '+' : ''}{formatQuantity(m.qtyChangeE4)}</td>
                <td className="qb-grid-cell num">{formatCents(m.valueChangeCents)}</td>
                <td className="qb-grid-cell num">{formatQuantity(m.qtyAfterE4)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Dialog>
  );
}
