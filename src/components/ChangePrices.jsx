import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';

const TYPES = [
  ['ALL', 'All products and services'],
  ['INVENTORY', 'Inventory (stock tracked)'],
  ['OTHER', 'Services and non-inventory'],
];

const ROUNDING = [
  ['NONE', 'No rounding (to the cent)'],
  ['NEAREST', 'To the nearest'],
  ['UP', 'Up to the next'],
  ['DOWN', 'Down to the'],
  ['END_IN99', 'Up to a price ending in .99'],
];
const STEPS = [5, 10, 25, 50, 100, 500, 1000];

/** "12.5" → 12500 thousandths of a percent; null when not a valid percentage (up to 3 decimals). */
function parsePercentMilli(text) {
  const m = /^(\d{0,4})(?:\.(\d{0,3}))?$/.exec(String(text).trim().replace(/%$/, ''));
  if (!m || (m[1] === '' && (m[2] ?? '') === '')) return null;
  return Number(m[1] || 0) * 1000 + Number((m[2] ?? '').padEnd(3, '0'));
}

function describe(details) {
  const a = details?.adjustment;
  if (!a) return '';
  const size = a.kind === 'PERCENT' ? `${Math.abs(a.percentMilli) / 1000}%` : formatCents(Math.abs(a.amountCents));
  const way = (a.kind === 'PERCENT' ? a.percentMilli : a.amountCents) < 0 ? 'down' : 'up';
  return `${size} ${way} from ${details.basis === 'COST' ? 'cost' : 'the current price'}`;
}

/** Products & Services › Change prices: new sales prices for many items at once. */
export default function ChangePrices({ onClose }) {
  const items = useLedgerQuery(api.listPriceItems);
  const history = useLedgerQuery(api.listPriceChanges);
  const [type, setType] = useState('ALL');
  const [showInactive, setShowInactive] = useState(false);
  const [picked, setPicked] = useState(() => new Set());
  const [basis, setBasis] = useState('CURRENT_PRICE');
  const [kind, setKind] = useState('PERCENT');
  const [direction, setDirection] = useState(1);
  const [size, setSize] = useState('');
  const [mode, setMode] = useState('NONE');
  const [step, setStep] = useState(100);
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const shown = useMemo(
    () =>
      (items.data ?? []).filter(
        (i) => (showInactive || i.isActive) && (type === 'ALL' || (type === 'INVENTORY') === i.isInventory),
      ),
    [items.data, type, showInactive],
  );
  const amount = kind === 'PERCENT' ? parsePercentMilli(size) : parseMoneyToCents(size);
  const valid = amount !== null && amount >= 0 && size.trim() !== '';
  const rounding = mode === 'NONE' || mode === 'END_IN99' ? { mode } : { mode, stepCents: step };
  const ids = shown.filter((i) => picked.has(i.id)).map((i) => i.id);
  const input = valid
    ? {
        productIds: ids,
        basis,
        adjustment: kind === 'PERCENT' ? { kind, percentMilli: direction * amount } : { kind, amountCents: direction * amount },
        rounding,
      }
    : null;
  const key = JSON.stringify(input);

  useEffect(() => {
    if (!input || input.productIds.length === 0) return setPreview(null);
    let stale = false;
    const t = setTimeout(() => {
      api.previewItemPrices(JSON.parse(key)).then(
        (rows) => !stale && setPreview(new Map(rows.map((r) => [r.productId, r]))),
        (err) => !stale && setNotice({ kind: 'error', text: errorText(err) }),
      );
    }, 150);
    return () => {
      stale = true;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, items.data]);

  const changing = ids.filter((id) => preview?.get(id)?.newCents != null);
  const toggle = (id) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const allPicked = shown.length > 0 && shown.every((i) => picked.has(i.id));

  const apply = async () => {
    if (!window.confirm(`Change the sales price of ${changing.length} item${changing.length === 1 ? '' : 's'}? Invoices and other documents already written keep their prices.`)) return;
    setBusy(true);
    try {
      const r = await api.applyItemPrices(input);
      setNotice({ kind: 'success', text: `Changed ${r.changed.length} price${r.changed.length === 1 ? '' : 's'}. Recorded in the audit trail (record ${r.auditRecordId}).` });
      setPicked(new Set());
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Change item prices</h2>
        <span className="min-w-0 truncate text-xs text-qb-ink-muted">New sales prices for many items at once. Documents already written keep their prices.</span>
        <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={onClose}>
          Back to Products & Services
        </button>
      </div>
      <div className="flex flex-none flex-wrap items-end gap-3 border-b border-qb-border px-2 py-1.5 text-sm">
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Show</span>
          <select className="qb-input w-[210px]" value={type} onChange={(e) => setType(e.target.value)}>
            {TYPES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
        </label>
        <label className="flex items-center gap-1 pb-1 text-xs">
          <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
          Inactive items
        </label>
        <span className="mx-1 h-[34px] w-px bg-qb-border" />
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Change</span>
          <select className="qb-input" value={direction} onChange={(e) => setDirection(Number(e.target.value))} aria-label="Raise or lower">
            <option value={1}>Raise by</option>
            <option value={-1}>Lower by</option>
          </select>
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Amount</span>
          <span className="flex gap-1">
            <input
              className="qb-input num w-[90px]"
              value={size}
              onChange={(e) => setSize(e.target.value)}
              inputMode="decimal"
              placeholder={kind === 'PERCENT' ? '10' : '1.00'}
              aria-label="Amount of change"
              aria-invalid={size.trim() !== '' && !valid}
            />
            <select className="qb-input" value={kind} onChange={(e) => setKind(e.target.value)} aria-label="Percent or amount">
              <option value="PERCENT">%</option>
              <option value="AMOUNT">fixed amount</option>
            </select>
          </span>
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Based on</span>
          <select className="qb-input" value={basis} onChange={(e) => setBasis(e.target.value)}>
            <option value="CURRENT_PRICE">Current price</option>
            <option value="COST">Unit cost</option>
          </select>
        </label>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Round</span>
          <span className="flex gap-1">
            <select className="qb-input" value={mode} onChange={(e) => setMode(e.target.value)} aria-label="Rounding">
              {ROUNDING.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
            {mode !== 'NONE' && mode !== 'END_IN99' && (
              <select className="qb-input" value={step} onChange={(e) => setStep(Number(e.target.value))} aria-label="Rounding step">
                {STEPS.map((s) => <option key={s} value={s}>{formatCents(s)}</option>)}
              </select>
            )}
          </span>
        </label>
        <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || changing.length === 0} onClick={apply}>
          Apply {changing.length || ''} price change{changing.length === 1 ? '' : 's'}
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {items.error && <Notice kind="error" text={items.error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head w-[32px]">
                <input
                  type="checkbox"
                  className="accent-qb-green"
                  aria-label="Select all shown"
                  checked={allPicked}
                  onChange={(e) => setPicked(e.target.checked ? new Set(shown.map((i) => i.id)) : new Set())}
                />
              </th>
              <th className="qb-grid-head">Item</th>
              <th className="qb-grid-head">Type</th>
              <th className="qb-grid-head text-right">Unit cost</th>
              <th className="qb-grid-head text-right">Current price</th>
              <th className="qb-grid-head text-right">New price</th>
              <th className="qb-grid-head">Note</th>
            </tr>
          </thead>
          <tbody>
            {items.data && shown.length === 0 && (
              <tr>
                <td colSpan={7} className="qb-grid-cell text-qb-ink-muted">No items of this type.</td>
              </tr>
            )}
            {shown.map((i) => {
              const p = picked.has(i.id) ? preview?.get(i.id) : null;
              return (
                <tr key={i.id} className={`qb-grid-row ${i.isActive ? '' : 'text-qb-ink-faint'}`}>
                  <td className="qb-grid-cell">
                    <input type="checkbox" className="accent-qb-green" aria-label={`Change ${i.name}`} checked={picked.has(i.id)} onChange={() => toggle(i.id)} />
                  </td>
                  <td className="qb-grid-cell font-semibold">{i.name}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">{i.isInventory ? 'Inventory' : 'Service / non-inventory'}</td>
                  <td className="qb-grid-cell num">{i.unitCostCents !== null ? formatCents(i.unitCostCents) : '—'}</td>
                  <td className="qb-grid-cell num">{formatCents(i.rateCents)}</td>
                  <td className={`qb-grid-cell num font-semibold ${p?.newCents != null && p.newCents < i.rateCents ? 'text-qb-negative' : ''}`}>
                    {p?.newCents != null ? formatCents(p.newCents) : ''}
                  </td>
                  <td className="qb-grid-cell text-xs text-qb-ink-muted">{p?.reason ?? ''}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {(history.data ?? []).length > 0 && (
          <div className="border-t border-qb-border px-2 py-1.5">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Earlier price changes (from the audit trail)</h3>
            <ul className="mt-0.5 text-xs">
              {history.data.slice(0, 10).map((h) => (
                <li key={h.auditRecordId} title={(h.details?.items ?? []).map((it) => `${it.name}: ${formatCents(it.beforeCents)} → ${formatCents(it.afterCents)}`).join('\n')}>
                  {new Date(h.timestamp).toLocaleString()} · {h.details?.items?.length ?? 0} item{h.details?.items?.length === 1 ? '' : 's'}, {describe(h.details)}
                  {h.actor ? ` · ${h.actor}` : ''}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </section>
  );
}
