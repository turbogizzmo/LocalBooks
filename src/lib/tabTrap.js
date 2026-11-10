// Tab inside a window (A17): as in the desktop program, Tab and Shift+Tab move among the active
// window's own fields and buttons and wrap round at its ends, instead of leaving it for a window
// behind (which would then come to the front) or the program's toolbar.

/** What Tab can reach: enabled controls and anything with a tabindex of 0 or more. */
const TABBABLE =
  'a[href], area[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), iframe, [contenteditable="true"], [tabindex]';

/**
 * The stop Tab moves to among `count` stops: `after` is the first stop after the focused element
 * (null when there is none), `before` the last stop before it; going past either end wraps round.
 */
export function nextTabStop(count, { after, before }, back) {
  if (count <= 0) return null;
  if (back) return before ?? count - 1;
  return after ?? 0;
}

/**
 * The elements Tab stops on inside `root`, in document order: shown, visible, enabled, not inert;
 * a group of radio buttons is one stop (its checked one, else its first), as the browser has it.
 */
export function tabStops(root) {
  const seenGroups = new Set();
  return [...root.querySelectorAll(TABBABLE)].filter((el) => {
    if (el.tabIndex < 0 || el.closest('[inert]') || el.getClientRects().length === 0) return false;
    if (getComputedStyle(el).visibility === 'hidden') return false;
    if (el instanceof HTMLInputElement && el.type === 'radio' && el.name) {
      const group = `${el.form?.id ?? ''}|${el.name}`;
      const checked = root.querySelector(`input[type="radio"][name="${CSS.escape(el.name)}"]:checked`);
      if (checked ? checked !== el : seenGroups.has(group)) return false;
      seenGroups.add(group);
    }
    return true;
  });
}

/**
 * A window's onKeyDown: Tab and Shift+Tab move to the next or previous stop inside `root`, wrapping
 * round. Keys from a modal dialog inside the window, or from outside its own elements (a dialog
 * rendered on the page body), are left to that dialog.
 */
export function trapTab(e, root) {
  if (e.key !== 'Tab' || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || !root) return;
  const from = e.target;
  if (!(from instanceof Element) || !root.contains(from)) return;
  const modal = from.closest('[aria-modal="true"]');
  if (modal && modal !== root && root.contains(modal)) return;
  const stops = tabStops(root);
  let after = null;
  let before = null;
  stops.forEach((el, i) => {
    if (el === from) return;
    const pos = from.compareDocumentPosition(el);
    // A stop inside the focused element (a cell's input) comes after it.
    if (pos & (Node.DOCUMENT_POSITION_FOLLOWING | Node.DOCUMENT_POSITION_CONTAINED_BY)) after ??= i;
    else if (pos & Node.DOCUMENT_POSITION_PRECEDING) before = i;
  });
  const next = nextTabStop(stops.length, { after, before }, e.shiftKey);
  if (next === null) return;
  e.preventDefault();
  stops[next].focus();
}
