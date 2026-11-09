// Customize Columns: a "Columns" button on a list, register or form grid that shows or hides each
// column and moves it earlier or later. The choice is kept per table in My Preferences.
import { useEffect, useMemo, useRef, useState } from 'react';
import { setColumnLayout, useMyPreferences } from '../lib/myPreferences.js';
import { arrangeColumns, layoutOf, moveColumn, toggleColumn, visibleColumns } from '../lib/columns.js';

/**
 * A table's columns as the person chose them. `table` names the table (e.g. 'customers');
 * `defs` its columns in their usual order ({ id, label, required?, hidden? }, plus anything the
 * table draws them with). Returns `{ columns, all, toggle, move, reset, customized }`.
 */
export function useColumns(table, defs) {
  const saved = useMyPreferences().columnLayouts?.[table] ?? null;
  const all = useMemo(() => arrangeColumns(defs, saved), [defs, saved]);
  const keep = (next) => setColumnLayout(table, layoutOf(next));
  return {
    columns: visibleColumns(all),
    all,
    toggle: (id) => keep(toggleColumn(all, id)),
    move: (id, step) => keep(moveColumn(all, id, step)),
    reset: () => setColumnLayout(table, null),
    customized: saved !== null,
  };
}

/** The Columns button and its menu, for a table's toolbar. */
export default function ColumnsMenu({ cols, label = 'Columns' }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => !ref.current?.contains(e.target) && setOpen(false);
    const esc = (e) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', esc);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', esc);
    };
  }, [open]);
  return (
    <div ref={ref} className="no-print relative flex-none self-center">
      <button type="button" className="qb-btn qb-btn-secondary h-row px-2" aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen((o) => !o)} title="Choose which columns show and their order">
        {label} ▾
      </button>
      {open && (
        <div role="dialog" aria-label="Customize columns" className="absolute right-0 top-full z-30 mt-0.5 w-[230px] rounded-xs border border-qb-border-strong bg-white py-1 shadow-popover">
          <div className="px-2 pb-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Columns</div>
          <ul className="max-h-[320px] overflow-auto scrollbar-thin">
            {cols.all.map((c, i) => (
              <li key={c.id} className="flex items-center gap-1 px-2 py-0.5 text-sm hover:bg-qb-row-hover">
                <label className="flex min-w-0 flex-1 items-center gap-1.5" title={c.required ? 'Always shown' : undefined}>
                  <input type="checkbox" checked={c.visible} disabled={c.required} onChange={() => cols.toggle(c.id)} />
                  <span className="truncate">{c.label}</span>
                </label>
                <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-ink disabled:opacity-30" disabled={i === 0} onClick={() => cols.move(c.id, -1)} aria-label={`Move ${c.label} earlier`}>
                  ▲
                </button>
                <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-ink disabled:opacity-30" disabled={i === cols.all.length - 1} onClick={() => cols.move(c.id, 1)} aria-label={`Move ${c.label} later`}>
                  ▼
                </button>
              </li>
            ))}
          </ul>
          <div className="border-t border-qb-border px-2 pt-1">
            <button type="button" className="text-xs text-qb-link hover:underline disabled:text-qb-ink-faint disabled:no-underline" disabled={!cols.customized} onClick={() => cols.reset()}>
              Default columns
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
