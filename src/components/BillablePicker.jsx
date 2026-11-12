import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents, formatMoney, formatQuantity, mulDivRound } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { timeBillable } from '../lib/billableOffer.js';

// The billable costs and time of one customer or job, picked to bill: shared by Sales › Bill Costs
// and the invoice form's Add billable costs…, so both show the same rows, markup and rules. The
// backend bills them the same way too (`billable_costs::write_with_billing`).

/** A cost's key in the picked set. */
export const costKey = (c) => `${c.source}:${c.lineId}`;

/** Thousandths of a percent → "15", "12.5". */
export const percentText = (milli) => (milli / 1000).toLocaleString('en-US', { maximumFractionDigits: 3, useGrouping: false });

/**
 * A markup percent as the backend reads it: digits with up to 3 decimals (and an optional %), 0 to
 * 1,000%, in thousandths of a percent; blank means the default from Preferences. `null` when invalid.
 */
export function parseMarkup(text, fallback) {
  const t = text.trim().replace(/%$/, '').trim();
  if (t === '') return fallback;
  const m = /^(\d{0,4})(?:\.(\d{0,3}))?$/.exec(t);
  if (!m || (m[1] === '' && !m[2])) return null;
  const milli = Number(m[1] || '0') * 1000 + Number((m[2] ?? '').padEnd(3, '0'));
  return milli <= 1_000_000 ? milli : null;
}

/** round_half_away(amount × milli / 100,000), as the backend works it out per cost. */
export const markupOf = (cents, milli) => mulDivRound(cents, milli, 100_000) ?? 0;

export { timeBillable };

/**
 * The picked costs and time among `shown` and `shownTime`. They start as `initial` (sets of cost
 * keys and time ids: an earlier choice) or as everything billable, and go back to everything
 * billable whenever `resetKey` changes (another customer, the lists reloaded).
 */
export function useBillablePicks(shown, shownTime, resetKey, initial = null) {
  const everything = () => [new Set(shown.map(costKey)), new Set(shownTime.filter(timeBillable).map((t) => t.id))];
  const [picked, setPicked] = useState(() => initial?.costs ?? everything()[0]);
  const [pickedTime, setPickedTime] = useState(() => initial?.time ?? everything()[1]);
  // Compared while rendering (not in an effect), so a development double render can't reset it.
  const [lastKey, setLastKey] = useState(resetKey);
  if (resetKey !== lastKey) {
    const [costs, time] = everything();
    setLastKey(resetKey);
    setPicked(costs);
    setPickedTime(time);
  }
  const flip = (set, k) => {
    const next = new Set(set);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    return next;
  };
  return {
    picked,
    pickedTime,
    toggle: (c) => setPicked((p) => flip(p, costKey(c))),
    toggleTime: (t) => setPickedTime((p) => flip(p, t.id)),
    chosen: shown.filter((c) => picked.has(costKey(c))),
    chosenTime: shownTime.filter((t) => pickedTime.has(t.id)),
  };
}

/** Costs, markup and time totals of a choice, as the invoice will have them. */
export function billableTotals(chosen, chosenTime, milli) {
  const costs = chosen.reduce((s, c) => s + c.amountCents, 0);
  const markup = milli === null ? 0 : chosen.reduce((s, c) => s + markupOf(c.amountCents, milli), 0);
  const time = chosenTime.reduce((s, t) => s + t.amountCents, 0);
  return { costs, markup, time, total: costs + markup + time };
}

/** The time and cost tables with a Bill tick on each row. */
export function BillableTables({ shown, shownTime, picks, milli }) {
  const { picked, pickedTime, toggle, toggleTime } = picks;
  return (
    <>
      {shownTime.length > 0 && (
        <table className="qb-grid border-b border-qb-border">
          <thead>
            <tr>
              <th className="qb-grid-head w-[28px]" aria-label="Bill" />
              <th className="qb-grid-head">Date</th>
              <th className="qb-grid-head">Time by</th>
              <th className="qb-grid-head">Service</th>
              <th className="qb-grid-head">Description</th>
              <th className="qb-grid-head text-right">Hours</th>
              <th className="qb-grid-head text-right">Rate</th>
              <th className="qb-grid-head text-right">Amount</th>
            </tr>
          </thead>
          <tbody>
            {shownTime.map((t) => (
              <tr key={`t${t.id}`} className="qb-grid-row">
                <td className="qb-grid-cell text-center">
                  <input
                    type="checkbox"
                    className="accent-qb-green"
                    checked={pickedTime.has(t.id)}
                    disabled={!timeBillable(t)}
                    title={!timeBillable(t) ? 'Give this time a service item on the timesheet to bill it' : undefined}
                    onChange={() => toggleTime(t)}
                    aria-label={`Bill time of ${formatDate(t.date)} by ${t.employeeName}`}
                  />
                </td>
                <td className="qb-grid-cell">{formatDate(t.date)}</td>
                <td className="qb-grid-cell">{t.employeeName}</td>
                <td className="qb-grid-cell">{t.serviceName ?? <span className="text-qb-negative">No service item</span>}</td>
                <td className="qb-grid-cell">{t.description ?? ''}</td>
                <td className="qb-grid-cell num">{formatQuantity(t.hoursE4)}</td>
                <td className="qb-grid-cell num">{formatCents(t.hourlyRateCents)}</td>
                <td className="qb-grid-cell num">{formatCents(t.amountCents)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {shown.length === 0 ? (
        <p className="px-3 py-2 text-sm text-qb-ink-muted">
          No unbilled costs. Give an expense, check, card charge or bill line a Customer:Job and tick Bill? to bill it here; billable time from the timesheet shows above.
        </p>
      ) : (
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head w-[28px]" aria-label="Bill" />
              <th className="qb-grid-head">Date</th>
              <th className="qb-grid-head">Source</th>
              <th className="qb-grid-head">Vendor</th>
              <th className="qb-grid-head">Account</th>
              <th className="qb-grid-head">Description</th>
              <th className="qb-grid-head text-right">Amount</th>
              <th className="qb-grid-head text-right">Markup</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((c) => (
              <tr key={costKey(c)} className="qb-grid-row">
                <td className="qb-grid-cell text-center">
                  <input type="checkbox" className="accent-qb-green" checked={picked.has(costKey(c))} onChange={() => toggle(c)} aria-label={`Bill ${c.memo ?? c.accountName} of ${formatDate(c.date)}`} />
                </td>
                <td className="qb-grid-cell">{formatDate(c.date)}</td>
                <td className="qb-grid-cell">{c.source === 'BILL' ? 'Bill' : c.source === 'CHARGE' ? 'Charge' : 'Check / expense'}</td>
                <td className="qb-grid-cell">{c.vendorName ?? '—'}</td>
                <td className="qb-grid-cell">{c.accountName}</td>
                <td className="qb-grid-cell">{c.memo ?? ''}</td>
                <td className="qb-grid-cell num" title={c.currencyCode ? `${formatMoney(c.foreignAmountCents, c.currencyCode)} at the expense's rate` : undefined}>
                  {formatCents(c.amountCents)}
                  {c.currencyCode && <div className="text-2xs text-qb-ink-muted">{formatMoney(c.foreignAmountCents, c.currencyCode)}</div>}
                </td>
                <td className="qb-grid-cell num text-qb-ink-muted">{milli === null ? '—' : formatCents(markupOf(c.amountCents, milli))}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

/** The one-line summary under the tables. */
export function BillableSummary({ chosen, chosenTime, totals }) {
  return (
    <span className="text-sm">
      {chosen.length + chosenTime.length} picked · costs <span className="num font-semibold">{formatCents(totals.costs)}</span> · markup <span className="num font-semibold">{formatCents(totals.markup)}</span>
      {chosenTime.length > 0 && (
        <>
          {' '}· time <span className="num font-semibold">{formatCents(totals.time)}</span>
        </>
      )}{' '}
      · total <span className="num font-semibold">{formatCents(totals.total)}</span>
    </span>
  );
}

/** The unbilled costs and time of one customer or job, loaded when `customerId` is set. */
export function useUnbilled(customerId) {
  const [state, setState] = useState({ customerId: null, costs: [], time: [], loaded: false, error: null });
  useEffect(() => {
    if (customerId == null) return setState({ customerId: null, costs: [], time: [], loaded: false, error: null });
    let live = true;
    Promise.all([api.listUnbilledCosts(customerId), api.listUnbilledTime(customerId)]).then(
      ([costs, time]) => live && setState({ customerId, costs, time, loaded: true, error: null }),
      (err) => live && setState({ customerId, costs: [], time: [], loaded: true, error: errorText(err) }),
    );
    return () => {
      live = false;
    };
  }, [customerId]);
  return state;
}

/**
 * The invoice form's Add billable costs… window: the customer's or job's unbilled costs and time,
 * the markup, and Add to invoice. Nothing is billed until the invoice is saved. `initial` is the
 * choice already on the invoice (`{ costs, time, markup }`), to change it.
 */
export function BillablePickerDialog({ customerId, customerName, defaults, initial, onAdd, onClose }) {
  const unbilled = useUnbilled(customerId);
  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-[55] grid place-items-center bg-qb-nav/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" aria-labelledby="billable-picker-title" className="flex max-h-[85vh] w-[860px] max-w-[95vw] flex-col rounded-xs bg-white shadow-popover">
        {/* Waits for the defaults too, so a blank markup means the default here as on save. */}
        {unbilled.loaded && defaults ? (
          <PickerBody customerId={customerId} customerName={customerName} unbilled={unbilled} defaults={defaults} initial={initial} onAdd={onAdd} onClose={onClose} />
        ) : (
          <p id="billable-picker-title" className="px-3 py-2 text-sm text-qb-ink-muted">
            Loading the billable costs and time for {customerName}…
          </p>
        )}
      </div>
    </div>
  );
}

/** Mounted once the lists are in, so the picks start from them (or from `initial`). */
function PickerBody({ customerId, customerName, unbilled, defaults, initial, onAdd, onClose }) {
  const [markup, setMarkup] = useState(() => initial?.markup ?? percentText(defaults.markupMilliPercent));
  const picks = useBillablePicks(unbilled.costs, unbilled.time, customerId, initial ? { costs: new Set(initial.costs.map(costKey)), time: new Set(initial.time.map((t) => t.id)) } : null);
  const milli = parseMarkup(markup, defaults.markupMilliPercent);
  const totals = billableTotals(picks.chosen, picks.chosenTime, milli);
  const reimbursedTo = defaults.asIncome ? 'an income account (counted as income)' : 'each cost’s expense account (reducing the expense)';
  return (
    <>
      <div className="flex flex-wrap items-end gap-3 border-b border-qb-border px-3 py-2">
        <div className="flex flex-col">
          <h2 id="billable-picker-title" className="text-md font-semibold">
            Billable costs and time
          </h2>
          <span className="text-xs text-qb-ink-muted">For {customerName}</span>
        </div>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Markup %</span>
          {/* Focus starts in the window, not on the form behind it. */}
          {/* eslint-disable-next-line jsx-a11y/no-autofocus */}
          <input autoFocus className="qb-input num w-[80px]" inputMode="decimal" value={markup} onChange={(e) => setMarkup(e.target.value)} aria-invalid={milli === null} />
        </label>
        <span className="pb-1 text-2xs text-qb-ink-faint">Credited to {reimbursedTo}; the markup to the markup account in Preferences › Time &amp; Expenses.</span>
      </div>
      {unbilled.error && <Notice kind="error" text={unbilled.error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <BillableTables shown={unbilled.costs} shownTime={unbilled.time} picks={picks} milli={milli} />
      </div>
      <div className="flex items-center gap-3 border-t border-qb-border px-3 py-1.5">
        <BillableSummary chosen={picks.chosen} chosenTime={picks.chosenTime} totals={totals} />
        <button type="button" className="qb-btn qb-btn-secondary ml-auto" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="qb-btn qb-btn-primary"
          disabled={milli === null || Boolean(unbilled.error)}
          onClick={() => onAdd({ customerId, costs: picks.chosen, time: picks.chosenTime, markup: percentText(milli), milli })}
        >
          Add to invoice
        </button>
      </div>
    </>
  );
}
