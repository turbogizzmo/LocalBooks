import { useEffect, useMemo, useState } from 'react';

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

function compareValues(a, b) {
  const aMissing = a === null || a === undefined || a === '';
  const bMissing = b === null || b === undefined || b === '';
  if (aMissing || bMissing) return aMissing === bMissing ? 0 : aMissing ? 1 : -1;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  if (typeof a === 'boolean' && typeof b === 'boolean') return Number(a) - Number(b);
  return collator.compare(String(a), String(b));
}

const STORE = 'localbooks.sort.';
function remembered(storageKey, columns, fallback) {
  if (!storageKey) return fallback;
  try {
    const saved = JSON.parse(localStorage.getItem(STORE + storageKey) ?? 'null');
    if (saved === 'none') return null;
    return saved && columns[saved.key] && (saved.dir === 'asc' || saved.dir === 'desc') ? saved : fallback;
  } catch {
    return fallback;
  }
}

/**
 * Click-to-sort for a list grid. `columns` maps a column key to a function returning that row's sort value
 * (numbers compare numerically, strings naturally, so "INV-9" sorts before "INV-10"; blanks always go last).
 * Clicking a header sorts ascending, again descending, and a third time goes back to `initial` (the
 * list's own order when there is none). Lists with a `date` column start newest first, as QuickBooks
 * lists do. With `storageKey`, the order chosen is remembered for that list on this computer.
 * Options: `{ initial, storageKey }`.
 */
export function useSort(rows, columns, { initial = columns.date ? { key: 'date', dir: 'desc' } : null, storageKey = null } = {}) {
  const [sort, setSort] = useState(() => remembered(storageKey, columns, initial));
  useEffect(() => {
    if (!storageKey) return;
    try {
      localStorage.setItem(STORE + storageKey, JSON.stringify(sort ?? 'none'));
    } catch {
      // Not remembered (private window or storage blocked); the list still sorts.
    }
  }, [storageKey, sort]);
  const sorted = useMemo(() => {
    const list = rows ?? [];
    const value = sort && columns[sort.key];
    if (!value) return list;
    const dir = sort.dir === 'desc' ? -1 : 1;
    // Stable: ties keep the list's own order whichever way it is sorted.
    return list
      .map((row, i) => ({ row, i, v: value(row) }))
      .sort((x, y) => {
        const c = compareValues(x.v, y.v);
        if (c === 0) return x.i - y.i;
        const blank = (v) => v === null || v === undefined || v === '';
        return blank(x.v) || blank(y.v) ? c : c * dir;
      })
      .map((x) => x.row);
    // `columns` is usually an inline object literal whose accessors never change, so it isn't a dependency.
  }, [rows, sort]);
  const toggle = (key) =>
    setSort((s) => {
      if (s?.key !== key) return { key, dir: 'asc' };
      if (s.dir === 'asc') return { key, dir: 'desc' };
      // Back to the starting order, unless that is this column: then it just flips.
      return initial?.key === key ? { key, dir: 'asc' } : initial;
    });
  return { sorted, sort, toggle };
}

/** A grid header cell that sorts its list when clicked. `sorter` is what useSort returned. */
export function SortTh({ sorter, col, className = '', children, ...rest }) {
  const active = sorter.sort?.key === col;
  const dir = active ? sorter.sort.dir : null;
  const right = className.includes('text-right');
  const arrow = (
    <span aria-hidden="true" className={`inline-block w-[10px] text-center text-[9px] ${active ? '' : 'opacity-0 group-hover:opacity-40'}`}>
      {dir === 'desc' ? '▼' : '▲'}
    </span>
  );
  return (
    <th
      className={`qb-grid-head group cursor-pointer select-none hover:text-qb-ink ${active ? 'text-qb-ink' : ''} ${className}`}
      aria-sort={dir === 'asc' ? 'ascending' : dir === 'desc' ? 'descending' : 'none'}
      onClick={() => sorter.toggle(col)}
      title="Click to sort"
      {...rest}
    >
      {right && arrow}
      {children}
      {!right && arrow}
    </th>
  );
}
