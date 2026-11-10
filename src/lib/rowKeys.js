// Keyboard use of a list's rows (R19): the first row is a Tab stop; ↑ and ↓ (Home and End) move
// between rows; Enter opens the row with focus, as a double-click does. Keys pressed on a control
// inside a row (its ▾ menu, a checkbox) are that control's own.

/** The row to move to from `at` of `count` rows for `key`, or null when the key doesn't move. */
export function nextRowIndex(key, at, count) {
  if (count <= 0) return null;
  switch (key) {
    case 'ArrowDown':
      return Math.min(count - 1, at + 1);
    case 'ArrowUp':
      return Math.max(0, at - 1);
    case 'Home':
      return 0;
    case 'End':
      return count - 1;
    default:
      return null;
  }
}

/**
 * Props for a list row: `open` runs on a double-click and on Enter; `select` (if given) when the row
 * gets focus, as a click would; `first` makes it the list's Tab stop.
 */
export function rowKeys({ first, open, select = null }) {
  return {
    tabIndex: first ? 0 : -1,
    'data-row-nav': '',
    onDoubleClick: open,
    onFocus: select ? (e) => e.target === e.currentTarget && select() : undefined,
    onKeyDown: (e) => {
      if (e.target !== e.currentTarget || e.altKey || e.ctrlKey || e.metaKey) return;
      if (e.key === 'Enter') {
        e.preventDefault();
        open();
        return;
      }
      const rows = [...(e.currentTarget.parentElement?.querySelectorAll(':scope > [data-row-nav]') ?? [])];
      const next = nextRowIndex(e.key, rows.indexOf(e.currentTarget), rows.length);
      if (next === null) return;
      e.preventDefault();
      rows[next]?.focus();
    },
  };
}
