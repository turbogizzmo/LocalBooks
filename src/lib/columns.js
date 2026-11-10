// Customize Columns: which columns a list, register or form grid shows, and in what order. Pure,
// so `node --test` covers it; `useColumns` (ColumnsMenu.jsx) keeps each table's choice in My
// Preferences (`columnLayouts`), on this computer, for the person using it.

/**
 * A table's columns as chosen: `defs` are the table's own columns in their usual order, each
 * `{ id, label, required?, hidden? }` (`hidden`: off until turned on); `saved` is the kept choice
 * `{ order: [ids], shown: [ids] }` or nothing. Returns every column, in order, with `visible`.
 * Columns the saved order doesn't know (added in a later version) keep their usual place;
 * required columns always show.
 */
export function arrangeColumns(defs, saved) {
  const order = Array.isArray(saved?.order) ? saved.order.filter((id) => defs.some((d) => d.id === id)) : [];
  const out = order.map((id) => defs.find((d) => d.id === id));
  defs.forEach((d, usual) => {
    if (out.includes(d)) return;
    // After the column that comes before it usually, so a new column lands where it belongs.
    const before = defs.slice(0, usual).reverse().find((p) => out.includes(p));
    out.splice(before ? out.indexOf(before) + 1 : 0, 0, d);
  });
  const shown = Array.isArray(saved?.shown) ? new Set(saved.shown) : null;
  return out.map((d) => ({ ...d, visible: Boolean(d.required) || (shown ? shown.has(d.id) || (!saved.order?.includes(d.id) && !d.hidden) : !d.hidden) }));
}

/** The choice to keep for an arranged list. */
export function layoutOf(arranged) {
  return { order: arranged.map((c) => c.id), shown: arranged.filter((c) => c.visible).map((c) => c.id) };
}

/** Shows or hides a column (required ones stay). */
export function toggleColumn(arranged, id) {
  return arranged.map((c) => (c.id === id && !c.required ? { ...c, visible: !c.visible } : c));
}

/**
 * Moves a column one place earlier (`step` -1) or later (1), past any columns `offered` says this
 * table doesn't have (a form without Class, say), so the move is one the table shows.
 */
export function moveColumn(arranged, id, step, offered = () => true) {
  const at = arranged.findIndex((c) => c.id === id);
  let to = at + step;
  while (to >= 0 && to < arranged.length && !offered(arranged[to].id)) to += step;
  if (at < 0 || to < 0 || to >= arranged.length) return arranged;
  const out = [...arranged];
  [out[at], out[to]] = [out[to], out[at]];
  return out;
}

/** The visible columns, in order. */
export const visibleColumns = (arranged) => arranged.filter((c) => c.visible);
