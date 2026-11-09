// The information panel (InfoPanel.jsx): which summary lines show for a customer, vendor or item,
// and whether the panel is hidden. The line choice is pure, so it is tested with node --test
// (infoPanel.test.js).

/**
 * The summary lines for a customer or vendor, from `get_party_summary`: `[label, cents, note]`.
 * A customer's open balance is what they owe; a vendor's what is owed to them. Estimates,
 * unbilled costs and open purchase orders show only when there are some.
 */
export function partySummaryLines(kind, s) {
  if (!s) return [];
  const plural = (n, one) => `${n} ${one}${n === 1 ? '' : 's'}`;
  const lines = [[kind === 'customer' ? 'Open balance' : 'Balance you owe', s.openBalanceCents, null]];
  if (kind === 'customer') {
    if (s.activeEstimateCount > 0) lines.push(['Active estimates', s.activeEstimateCents, plural(s.activeEstimateCount, 'estimate')]);
    if (s.unbilledCostCount > 0) lines.push(['Unbilled costs', s.unbilledCostCents, plural(s.unbilledCostCount, 'cost')]);
    if (s.unbilledTripCount > 0) lines.push(['Unbilled mileage', null, plural(s.unbilledTripCount, 'trip')]);
  } else if (s.openPoCount > 0) {
    lines.push(['Open purchase orders', s.openPoCents, plural(s.openPoCount, 'order')]);
  }
  return lines;
}

const KEY = 'localbooks.infoPanel.hidden';

/** Whether the panel is hidden in `place` ('customer', 'vendor', 'item', 'form'), on this computer. */
export function infoPanelHidden(place, storage = globalThis.localStorage) {
  try {
    return JSON.parse(storage?.getItem(KEY) ?? '[]').includes(place);
  } catch {
    return false;
  }
}

/** Hides or shows the panel in `place`, remembered on this computer. */
export function setInfoPanelHidden(place, hidden, storage = globalThis.localStorage) {
  try {
    const now = new Set(JSON.parse(storage?.getItem(KEY) ?? '[]'));
    if (hidden) now.add(place);
    else now.delete(place);
    storage?.setItem(KEY, JSON.stringify([...now]));
  } catch {
    // Storage unavailable: the choice holds while the window is open.
  }
}

/**
 * Date Stamp in the notes: a new first line starting with `stamp` (today, as dates show), ready to
 * type after, with the notes so far below it.
 */
export function stampNotes(notes, stamp) {
  const rest = (notes ?? '').replace(/^\s+/, '');
  return rest ? `${stamp}: \n${rest}` : `${stamp}: `;
}
