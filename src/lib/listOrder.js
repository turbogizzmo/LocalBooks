// Lists in a custom order, shown flat or as a tree (the chart of accounts, items and customers).
// Pure helpers, so `node --test` covers them; the order itself is kept in the company file
// (list_order.rs) as the ids of the list's rows in the order they were dragged into.

/**
 * The rows as a tree: each parent followed by its children, siblings in the custom `order` (the
 * ids, first to last; rows not in it follow in the list's own order). `idOf` and `parentOf` read a
 * row's id and its parent's id (null for a top-level row; a parent missing from the list makes the
 * row top-level). Returns `[{ row, id, depth, hasChildren }]`.
 */
export function arrange(rows, { idOf = (r) => r.id, parentOf = () => null, order = [] } = {}) {
  const ids = new Set(rows.map(idOf));
  const position = new Map(order.map((id, i) => [id, i]));
  const natural = new Map(rows.map((r, i) => [idOf(r), i]));
  const children = new Map();
  for (const row of rows) {
    const parent = parentOf(row);
    const key = parent !== null && parent !== undefined && ids.has(parent) && parent !== idOf(row) ? parent : null;
    if (!children.has(key)) children.set(key, []);
    children.get(key).push(row);
  }
  const rank = (row) => {
    const id = idOf(row);
    return position.has(id) ? [0, position.get(id)] : [1, natural.get(id)];
  };
  for (const list of children.values()) {
    list.sort((a, b) => {
      const [ga, ia] = rank(a);
      const [gb, ib] = rank(b);
      return ga - gb || ia - ib;
    });
  }
  const out = [];
  const seen = new Set();
  const visit = (row, depth) => {
    const id = idOf(row);
    if (seen.has(id)) return;
    seen.add(id);
    const kids = children.get(id) ?? [];
    out.push({ row, id, depth, hasChildren: kids.length > 0 });
    for (const kid of kids) visit(kid, depth + 1);
  };
  for (const row of children.get(null) ?? []) visit(row, 0);
  // A loop of parents (which the books don't allow) still shows every row.
  for (const row of rows) if (!seen.has(idOf(row))) visit(row, 0);
  return out;
}

/** The arranged rows still showing when the rows in `collapsed` (a Set of ids) hide their children. */
export function visibleRows(arranged, collapsed) {
  const out = [];
  let hideBelow = null;
  for (const item of arranged) {
    if (hideBelow !== null && item.depth > hideBelow) continue;
    hideBelow = null;
    out.push(item);
    if (item.hasChildren && collapsed.has(item.id)) hideBelow = item.depth;
  }
  return out;
}

/** Every row with children, for Collapse all. */
export function parentIds(arranged) {
  return new Set(arranged.filter((a) => a.hasChildren).map((a) => a.id));
}

/** The arranged row's parent id (null at the top). */
function parentAt(arranged, index) {
  const { depth } = arranged[index];
  for (let i = index - 1; i >= 0; i -= 1) if (arranged[i].depth < depth) return arranged[i].id;
  return null;
}

/**
 * The order after dragging `dragId` onto `targetId`: the dragged row, with its children, moves to
 * just before the target (or after it, with `after`). Rows move only among their siblings, so
 * this returns null when the two have different parents or are the same row.
 */
export function moveRow(arranged, dragId, targetId, { after = false } = {}) {
  const from = arranged.findIndex((a) => a.id === dragId);
  const to = arranged.findIndex((a) => a.id === targetId);
  if (from < 0 || to < 0 || from === to) return null;
  if (parentAt(arranged, from) !== parentAt(arranged, to)) return null;
  const span = (index) => {
    let end = index + 1;
    while (end < arranged.length && arranged[end].depth > arranged[index].depth) end += 1;
    return end;
  };
  const moving = arranged.slice(from, span(from));
  const rest = [...arranged.slice(0, from), ...arranged.slice(span(from))];
  const at = rest.findIndex((a) => a.id === targetId);
  // After the target means after its children too.
  let index = at;
  if (after) {
    index = at + 1;
    while (index < rest.length && rest[index].depth > rest[at].depth) index += 1;
  }
  return [...rest.slice(0, index), ...moving, ...rest.slice(index)].map((a) => a.id);
}

/** A parent id from a "Parent:Child" name (customers' jobs, sub-items), given the rows' ids by name. */
export function parentByName(name, idsByName) {
  const text = String(name ?? '');
  const at = text.lastIndexOf(':');
  return at > 0 ? (idsByName.get(text.slice(0, at).toLowerCase()) ?? null) : null;
}
