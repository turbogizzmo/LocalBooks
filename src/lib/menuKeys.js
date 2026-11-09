// Keyboard use of a pop-up menu (R19), shared by the form toolbar's menus and a list row's ▾ menu:
// ↑ and ↓ move between its items (wrapping round), Home and End go to the first and last, Escape
// closes it and puts focus back on the button that opened it, and Tab closes it.

/**
 * Which item gets focus after `key` with focus on item `at` (-1: none) of `count` enabled items,
 * or null when the key doesn't move.
 */
export function nextMenuIndex(key, at, count) {
  if (count <= 0) return null;
  switch (key) {
    case 'ArrowDown':
      return at < 0 ? 0 : (at + 1) % count;
    case 'ArrowUp':
      return at < 0 ? count - 1 : (at + count - 1) % count;
    case 'Home':
      return 0;
    case 'End':
      return count - 1;
    default:
      return null;
  }
}

/**
 * A menu's onKeyDown: moves focus among the enabled buttons in `menu`, and on Escape or Tab calls
 * `close` (Escape also refocuses `opener`).
 */
export function menuKeyDown(e, { menu, opener, close }) {
  if (e.key === 'Escape') {
    e.stopPropagation();
    close();
    opener?.focus();
    return;
  }
  if (e.key === 'Tab') {
    close();
    return;
  }
  const items = menu ? [...menu.querySelectorAll('button:not(:disabled)')] : [];
  const next = nextMenuIndex(e.key, items.indexOf(document.activeElement), items.length);
  if (next === null) return;
  e.preventDefault();
  items[next]?.focus();
}
