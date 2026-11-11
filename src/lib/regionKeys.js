// F6 between the parts of the LocalBooks window (gap R19). Tab stays inside the active window (see
// tabTrap.js), so the sidebar and the toolbar need their own way in: F6 moves focus to the next
// part (the sidebar, the Open Window List, the toolbar, the active window) and Shift+F6 to the one
// before, wrapping round, as F6 moves between panes in most desktop programs. Each part is marked
// with `data-f6-region`; a part comes back to the field it was left on.

import { tabStops } from './tabTrap.js';

/**
 * The part F6 moves to among `count` parts, from the part at `current` (-1 when focus is in none
 * of them): the next one, or with `back` the one before, wrapping round. Null when there are none.
 */
export function nextRegion(count, current, back) {
  if (count <= 0) return null;
  if (current < 0) return back ? count - 1 : 0;
  return (current + (back ? count - 1 : 1)) % count;
}

/** The field each part was last left on, so F6 brings you back to it. */
const lastFocus = new WeakMap();

/** Remembers where focus was in a part as it leaves (a `focusout` listener on the document). */
export function rememberRegionFocus(e) {
  const region = e.target instanceof Element ? e.target.closest('[data-f6-region]') : null;
  if (region) lastFocus.set(region, e.target);
}

/**
 * The document's keydown for F6 and Shift+F6. Left alone while a modal dialog is open (Tab and
 * Esc are the dialog's) or when another key is held with it.
 */
export function moveBetweenRegions(e) {
  if (e.key !== 'F6' || e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented) return;
  if (document.querySelector('[aria-modal="true"]')) return;
  const regions = [...document.querySelectorAll('[data-f6-region]')].filter((r) => !r.closest('[inert]') && r.getClientRects().length > 0);
  const current = regions.findIndex((r) => r.contains(document.activeElement));
  const next = nextRegion(regions.length, current, e.shiftKey);
  if (next === null) return;
  e.preventDefault();
  const region = regions[next];
  const remembered = lastFocus.get(region);
  const target =
    remembered && region.contains(remembered) && remembered.isConnected && tabStops(region).includes(remembered)
      ? remembered
      : (region.querySelector('[aria-current="page"]') ?? tabStops(region)[0]);
  (target ?? region).focus();
}
