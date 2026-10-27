import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { ROUNDING, STEPS, parsePercentMilli } from './ChangePrices.jsx';
import { TableReport } from './MoreReports.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';

const BASIS = [
  ['FIXED', 'Fixed price'],
  ['STANDARD', 'Standard price ±%'],
  ['COST', 'Unit cost ±%'],
];

/** −10,000 → "−10%", 12,500 → "+12.5%". */
export function percentText(milli) {
  const n = Math.abs(milli) / 1000;
  return `${milli < 0 ? '−' : '+'}${n}%`;
}

/** "10" → 10000, "-12.5" → −12500 (thousandths of a percent); null when not a percentage. */
function parseSignedPercent(text) {
  const t = String(text).trim();
  const negative = /^[-−]/.test(t);
  const milli = parsePercentMilli(t.replace(/^[-−+]/, ''));
  return milli === null ? null : negative ? -milli : milli;
}

const milliInput = (milli) => (milli == null ? '' : String(milli / 1000));

/** One-line description of a level ("−10% of standard, to the nearest $1.00", "3 items"). */
export function levelSummary(level) {
  if (level.kind === 'FIXED_PERCENT') return `${percentText(level.percentMilli)} on every item`;
  const n = level.items?.length ?? 0;
  return `Per item · ${n} item${n === 1 ? '' : 's'}`;
}

/** The customer form's "Price level" field. */
export function CustomerPriceLevelField({ form, setForm }) {
  const levels = useLedgerQuery(api.listPriceLevels).data ?? [];
  if (levels.length === 0 && !form.priceLevelId) return null;
  return (
    <label className="flex flex-col gap-0.5">
      <span className="font-semibold text-qb-ink-muted">Price level</span>
      <select className="qb-input" value={form.priceLevelId ?? ''} onChange={(e) => setForm((f) => ({ ...f, priceLevelId: e.target.value === '' ? null : Number(e.target.value) }))}>
        <option value="">Standard prices</option>
        {levels
          .filter((l) => l.isActive || l.id === form.priceLevelId)
          .map((l) => (
            <option key={l.id} value={l.id}>
              {l.name} ({levelSummary(l)}){l.isActive ? '' : ' (inactive)'}
            </option>
          ))}
      </select>
    </label>
  );
}

/**
 * Price levels for the sales forms: `levels` (active ones), `priceOf(levelId, productId)` (cents, or
 * null for the standard price) and `levelOf(customer)` (the customer's default level id or null).
 */
export function usePriceLevels() {
  const { data } = useLedgerQuery(api.getItemPriceList);
  return useMemo(() => {
    const levels = (data?.levels ?? []).filter((l) => l.isActive);
    const active = new Set(levels.map((l) => l.id));
    const prices = new Map();
    for (const row of data?.items ?? []) {
      for (const p of row.prices) prices.set(`${p.priceLevelId}:${row.id}`, p.priceCents);
    }
    const defaults = new Map((data?.customerLevels ?? []).map((c) => [c.customerId, c.priceLevelId]));
    return {
      levels,
      priceOf: (levelId, productId) => (levelId != null && active.has(levelId) ? (prices.get(`${levelId}:${productId}`) ?? null) : null),
      levelOf: (customer) => {
        const id = customer ? defaults.get(customer.id) : undefined;
        return id != null && active.has(id) ? id : null;
      },
    };
  }, [data]);
}

const blankDraft = () => ({ id: null, name: '', kind: 'FIXED_PERCENT', percent: '', mode: 'NONE', step: 100, items: {}, isActive: true });

function draftOf(level) {
  const r = level.rounding ?? { mode: 'NONE' };
  return {
    id: level.id,
    name: level.name,
    kind: level.kind,
    percent: milliInput(level.percentMilli),
    mode: r.mode,
    step: r.stepCents ?? 100,
    items: Object.fromEntries(
      level.items.map((i) => [i.productId, { basis: i.basis, value: i.basis === 'FIXED' ? (i.priceCents / 100).toFixed(2) : milliInput(i.percentMilli) }]),
    ),
    isActive: level.isActive,
  };
}

/** The level the backend takes, or { error } saying what's wrong with the draft. */
function inputOf(draft) {
  const rounding = draft.mode === 'NONE' || draft.mode === 'END_IN99' ? { mode: draft.mode } : { mode: draft.mode, stepCents: draft.step };
  const percentMilli = draft.kind === 'FIXED_PERCENT' ? parseSignedPercent(draft.percent || '0') : 0;
  if (percentMilli === null) return { error: 'Enter the percentage, such as -10 for 10% off or 15 for 15% more.' };
  const items = [];
  if (draft.kind === 'PER_ITEM') {
    for (const [productId, row] of Object.entries(draft.items)) {
      if (row.basis === 'FIXED') {
        const cents = parseMoneyToCents(row.value);
        if (cents === null || cents < 0) return { error: 'Enter a price for every item on the level.' };
        items.push({ productId: Number(productId), basis: 'FIXED', priceCents: cents });
      } else {
        const milli = parseSignedPercent(row.value || '0');
        if (milli === null) return { error: 'Enter a percentage for every item on the level.' };
        items.push({ productId: Number(productId), basis: row.basis, percentMilli: milli });
      }
    }
  }
  return { input: { name: draft.name.trim(), kind: draft.kind, percentMilli, rounding, items, isActive: draft.isActive } };
}

const SOURCE_NOTE = {
  STANDARD: 'Not on this level: standard price',
  NO_COST: 'No unit cost yet: standard price',
};

/** Products & Services › Price levels: the list of levels and an editor with a live price preview. */
export default function PriceLevels({ onClose }) {
  const levels = useLedgerQuery(api.listPriceLevels);
  const items = useLedgerQuery(api.listPriceItems);
  const history = useLedgerQuery(api.listPriceLevelChanges);
  const [draft, setDraft] = useState(null);
  const [preview, setPreview] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const [showInactive, setShowInactive] = useState(false);

  const built = draft ? inputOf(draft) : null;
  const key = JSON.stringify(built?.input ?? null);
  useEffect(() => {
    if (!built?.input) return setPreview(null);
    let stale = false;
    const t = setTimeout(() => {
      api.previewPriceLevel({ ...JSON.parse(key), name: JSON.parse(key).name || 'Preview' }).then(
        (rows) => !stale && setPreview(new Map(rows.map((r) => [r.productId, r]))),
        () => !stale && setPreview(null),
      );
    }, 150);
    return () => {
      stale = true;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, items.data]);

  const set = (patch) => setDraft((d) => ({ ...d, ...patch }));
  const setItem = (productId, row) =>
    setDraft((d) => {
      const next = { ...d.items };
      if (row === null) delete next[productId];
      else next[productId] = { ...(next[productId] ?? { basis: 'FIXED', value: '' }), ...row };
      return { ...d, items: next };
    });

  const save = async () => {
    if (built.error) return setNotice({ kind: 'error', text: built.error });
    setBusy(true);
    try {
      const saved = await api.savePriceLevel(draft.id, built.input);
      setNotice({ kind: 'success', text: `Saved price level ${saved.name}. Recorded in the audit trail.` });
      setDraft(draftOf(saved));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const remove = async (level) => {
    const who = level.customerCount ? ` ${level.customerCount} customer${level.customerCount === 1 ? '' : 's'} who have it go back to standard prices.` : '';
    if (!(await askConfirm(`Delete price level ${level.name}?${who} Documents already written keep their prices.`, { confirmLabel: 'Delete', danger: true }))) return;
    try {
      await api.deletePriceLevel(level.id);
      setNotice({ kind: 'success', text: `Deleted price level ${level.name}.` });
      if (draft?.id === level.id) setDraft(null);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const toggleActive = async (level) => {
    try {
      await api.setPriceLevelActive(level.id, !level.isActive);
      if (draft?.id === level.id) set({ isActive: !level.isActive });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const shownItems = (items.data ?? []).filter((i) => i.isActive || draft?.items[i.id]);
  const shownLevels = (levels.data ?? []).filter((l) => showInactive || l.isActive);

  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Price levels</h2>
        <span className="min-w-0 truncate text-xs text-qb-ink-muted">Sets of prices for some customers. A customer's level fills in the rate on new invoices, sales receipts and estimates.</span>
        <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={onClose}>
          Back to Products & Services
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {levels.error && <Notice kind="error" text={levels.error} />}
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[300px] flex-none flex-col border-r border-qb-border">
          <div className="flex items-center gap-2 border-b border-qb-border px-2 py-1">
            <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => setDraft(blankDraft())}>
              New price level
            </button>
            <label className="ml-auto flex items-center gap-1 text-xs">
              <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
              Show inactive
            </label>
          </div>
          <ul className="min-h-0 flex-1 overflow-auto scrollbar-thin" aria-label="Price levels">
            {levels.data && shownLevels.length === 0 && <li className="px-2 py-1.5 text-sm text-qb-ink-muted">No price levels yet.</li>}
            {shownLevels.map((l) => (
              <li key={l.id} className={`border-b border-qb-border px-2 py-1 text-sm ${draft?.id === l.id ? 'bg-qb-green-tint' : ''} ${l.isActive ? '' : 'text-qb-ink-faint'}`}>
                <button type="button" className="block w-full text-left" onClick={() => setDraft(draftOf(l))}>
                  <span className="font-semibold">{l.name}</span>
                  {l.isActive ? '' : ' (inactive)'}
                  <span className="block text-xs text-qb-ink-muted">
                    {levelSummary(l)} · {l.customerCount} customer{l.customerCount === 1 ? '' : 's'}
                  </span>
                </button>
                <span className="flex gap-2 text-xs">
                  <button type="button" className="text-qb-link hover:underline" onClick={() => toggleActive(l)}>
                    {l.isActive ? 'Make inactive' : 'Make active'}
                  </button>
                  <button type="button" className="text-qb-negative hover:underline" onClick={() => remove(l)}>
                    Delete
                  </button>
                </span>
              </li>
            ))}
          </ul>
          {(history.data ?? []).length > 0 && (
            <div className="max-h-[140px] flex-none overflow-auto border-t border-qb-border px-2 py-1 scrollbar-thin">
              <h3 className="text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Changes (from the audit trail)</h3>
              <ul className="text-xs">
                {history.data.slice(0, 10).map((h) => (
                  <li key={h.auditRecordId}>
                    {new Date(h.timestamp).toLocaleString()} · {String(h.details?.event ?? '').replace('PRICE_LEVEL_', '').toLowerCase()}{' '}
                    {(h.details?.after ?? h.details?.before)?.name}
                    {h.actor ? ` · ${h.actor}` : ''}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          {!draft ? (
            <p className="px-3 py-2 text-sm text-qb-ink-muted">Pick a price level to change it, or make a new one.</p>
          ) : (
            <>
              <div className="flex flex-none flex-wrap items-end gap-3 border-b border-qb-border px-2 py-1.5 text-sm">
                <label className="flex flex-col gap-0.5 text-xs">
                  <span className="font-semibold text-qb-ink-muted">Name</span>
                  <input className="qb-input w-[180px]" value={draft.name} onChange={(e) => set({ name: e.target.value })} aria-label="Price level name" />
                </label>
                <label className="flex flex-col gap-0.5 text-xs">
                  <span className="font-semibold text-qb-ink-muted">Type</span>
                  <select className="qb-input" value={draft.kind} onChange={(e) => set({ kind: e.target.value })} aria-label="Price level type">
                    <option value="FIXED_PERCENT">Fixed % (every item)</option>
                    <option value="PER_ITEM">Per item</option>
                  </select>
                </label>
                {draft.kind === 'FIXED_PERCENT' && (
                  <label className="flex flex-col gap-0.5 text-xs">
                    <span className="font-semibold text-qb-ink-muted">Change standard prices by %</span>
                    <input
                      className="qb-input num w-[90px]"
                      value={draft.percent}
                      onChange={(e) => set({ percent: e.target.value })}
                      inputMode="decimal"
                      placeholder="-10"
                      aria-label="Percentage change"
                      aria-invalid={parseSignedPercent(draft.percent || '0') === null}
                    />
                  </label>
                )}
                <label className="flex flex-col gap-0.5 text-xs">
                  <span className="font-semibold text-qb-ink-muted">Round {draft.kind === 'PER_ITEM' ? 'percentage prices' : ''}</span>
                  <span className="flex gap-1">
                    <select className="qb-input" value={draft.mode} onChange={(e) => set({ mode: e.target.value })} aria-label="Rounding">
                      {ROUNDING.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                    </select>
                    {draft.mode !== 'NONE' && draft.mode !== 'END_IN99' && (
                      <select className="qb-input" value={draft.step} onChange={(e) => set({ step: Number(e.target.value) })} aria-label="Rounding step">
                        {STEPS.map((s) => <option key={s} value={s}>{formatCents(s)}</option>)}
                      </select>
                    )}
                  </span>
                </label>
                <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || !draft.name.trim()} onClick={save}>
                  {draft.id ? 'Save changes' : 'Create price level'}
                </button>
              </div>
              {built?.error && <Notice kind="info" text={built.error} />}
              <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
                <table className="qb-grid">
                  <thead>
                    <tr>
                      {draft.kind === 'PER_ITEM' && <th className="qb-grid-head w-[32px]" aria-label="On this level" />}
                      <th className="qb-grid-head">Item</th>
                      <th className="qb-grid-head text-right">Unit cost</th>
                      <th className="qb-grid-head text-right">Standard price</th>
                      {draft.kind === 'PER_ITEM' && <th className="qb-grid-head">Price is</th>}
                      {draft.kind === 'PER_ITEM' && <th className="qb-grid-head text-right">Amount or %</th>}
                      <th className="qb-grid-head text-right">Level price</th>
                      <th className="qb-grid-head">Note</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shownItems.map((i) => {
                      const row = draft.items[i.id];
                      const p = preview?.get(i.id);
                      return (
                        <tr key={i.id} className="qb-grid-row">
                          {draft.kind === 'PER_ITEM' && (
                            <td className="qb-grid-cell">
                              <input
                                type="checkbox"
                                className="accent-qb-green"
                                aria-label={`Put ${i.name} on this level`}
                                checked={Boolean(row)}
                                onChange={(e) => setItem(i.id, e.target.checked ? { value: (i.rateCents / 100).toFixed(2) } : null)}
                              />
                            </td>
                          )}
                          <td className="qb-grid-cell font-semibold">{i.name}</td>
                          <td className="qb-grid-cell num">{i.unitCostCents !== null ? formatCents(i.unitCostCents) : '—'}</td>
                          <td className="qb-grid-cell num">{formatCents(i.rateCents)}</td>
                          {draft.kind === 'PER_ITEM' && (
                            <td className="qb-grid-cell">
                              {row && (
                                <select className="qb-input h-[22px]" value={row.basis} onChange={(e) => setItem(i.id, { basis: e.target.value, value: '' })} aria-label={`How ${i.name} is priced`}>
                                  {BASIS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                                </select>
                              )}
                            </td>
                          )}
                          {draft.kind === 'PER_ITEM' && (
                            <td className="qb-grid-cell">
                              {row && (
                                <input
                                  className="qb-input num h-[22px] w-[90px]"
                                  value={row.value}
                                  inputMode="decimal"
                                  placeholder={row.basis === 'FIXED' ? '0.00' : '-10'}
                                  onChange={(e) => setItem(i.id, { value: e.target.value })}
                                  aria-label={`${i.name} ${row.basis === 'FIXED' ? 'price' : 'percentage'}`}
                                />
                              )}
                            </td>
                          )}
                          <td className={`qb-grid-cell num font-semibold ${p && p.priceCents < i.rateCents ? 'text-qb-negative' : ''}`}>{p ? formatCents(p.priceCents) : ''}</td>
                          <td className="qb-grid-cell text-xs text-qb-ink-muted">{p ? (SOURCE_NOTE[p.source] ?? '') : ''}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

/** Reports › Item Price List: each item's standard price and its price at every price level. */
export function ItemPriceListReport() {
  const { data, error } = useLedgerQuery(api.getItemPriceList);
  const [showInactive, setShowInactive] = useState(false);
  const levels = (data?.levels ?? []).filter((l) => showInactive || l.isActive);
  const items = (data?.items ?? []).filter((i) => showInactive || i.isActive);
  const priceAt = (item, level) => item.prices.find((p) => p.priceLevelId === level.id)?.priceCents ?? item.rateCents;
  return (
    <TableReport
      title="Item Price List"
      subtitle={levels.length ? `${levels.length} price level${levels.length === 1 ? '' : 's'}` : 'No price levels yet: standard prices only'}
      columns={[
        { label: 'Item' },
        { label: 'Description' },
        { label: 'Cost', align: 'right' },
        { label: 'Standard price', align: 'right' },
        ...levels.map((l) => ({ label: `${l.name}${l.isActive ? '' : ' (inactive)'}`, align: 'right' })),
      ]}
      rows={items.map((i) => ({
        cells: [
          `${i.name}${i.isActive ? '' : ' (inactive)'}`,
          i.description ?? '',
          i.unitCostCents != null ? formatCents(i.unitCostCents) : '',
          formatCents(i.rateCents),
          ...levels.map((l) => formatCents(priceAt(i, l))),
        ],
      }))}
      error={error}
      toolbar={
        <label className="flex items-center gap-1 pb-1 text-sm">
          <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
          Show inactive items and levels
        </label>
      }
      footer="Cost: average cost while stock is on hand, else the last purchase cost · A level's price falls back to the standard price for items it doesn't list or that have no cost"
      onCsvRows={() =>
        items.map((i) => [i.name, i.description ?? '', i.unitCostCents != null ? (i.unitCostCents / 100).toFixed(2) : '', (i.rateCents / 100).toFixed(2), ...levels.map((l) => (priceAt(i, l) / 100).toFixed(2))])
      }
    />
  );
}
