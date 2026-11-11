// Lists in a custom order, shown flat or as a tree: the chart of accounts, items and customers.
// `useListOrder` arranges a list's rows (src/lib/listOrder.js) in the order kept in the company file
// (list_order.rs), lets rows be dragged among their siblings, and keeps the view (flat or tree) and
// which parents are collapsed.
import { useCallback, useMemo, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { arrange, moveRow, parentIds, visibleRows } from '../lib/listOrder.js';

const VIEW_KEY = 'localbooks.listView.';

function readView(list, fallback) {
  try {
    const saved = window.localStorage.getItem(VIEW_KEY + list);
    return saved === 'flat' || saved === 'tree' ? saved : fallback;
  } catch {
    return fallback;
  }
}

/**
 * `list` is 'ACCOUNTS', 'ITEMS' or 'CUSTOMERS'; `rows` in the list's usual order; `parentOf(row)`
 * its parent's id or null. `dragEnabled` is false while the list is searched or sorted by a
 * column, when the custom order isn't what's shown. Reports problems through `onNotice`.
 */
export function useListOrder(list, rows, { parentOf, dragEnabled = true, onNotice, defaultView = 'tree' }) {
  const order = useLedgerQuery(useCallback(() => api.getListOrder(list), [list]));
  const [view, setViewState] = useState(() => readView(list, defaultView));
  const [collapsed, setCollapsed] = useState(() => new Set());
  const [dragging, setDragging] = useState(null);
  const [over, setOver] = useState(null); // { id, after }

  const arranged = useMemo(() => arrange(rows ?? [], { parentOf, order: order.data ?? [] }), [rows, parentOf, order.data]);
  const tree = view === 'tree';
  const visible = useMemo(() => (tree ? visibleRows(arranged, collapsed) : arranged), [tree, arranged, collapsed]);

  const setView = (next) => {
    setViewState(next);
    try {
      window.localStorage.setItem(VIEW_KEY + list, next);
    } catch {
      // Not remembered; the view still changes.
    }
  };
  const toggle = (id) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const drop = async (targetId, after) => {
    const next = dragging === null ? null : moveRow(arranged, dragging, targetId, { after });
    setDragging(null);
    setOver(null);
    if (!next) {
      if (dragging !== null && dragging !== targetId) onNotice?.({ kind: 'info', text: 'Drag a row among the rows under the same parent.' });
      return;
    }
    try {
      await api.setListOrder(list, next);
      await order.reload();
    } catch (err) {
      onNotice?.({ kind: 'error', text: errorText(err) });
    }
  };

  /** Props for a row's <tr>: drag it, or drop another row before or after it. */
  const rowProps = (id) => {
    if (!dragEnabled) return {};
    const marked = over?.id === id && dragging !== null && dragging !== id;
    return {
      draggable: true,
      onDragStart: (e) => {
        setDragging(id);
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', String(id));
      },
      onDragOver: (e) => {
        if (dragging === null) return;
        e.preventDefault();
        const box = e.currentTarget.getBoundingClientRect();
        const after = e.clientY > box.top + box.height / 2;
        if (over?.id !== id || over.after !== after) setOver({ id, after });
      },
      onDragLeave: () => over?.id === id && setOver(null),
      onDrop: (e) => {
        e.preventDefault();
        const box = e.currentTarget.getBoundingClientRect();
        drop(id, e.clientY > box.top + box.height / 2);
      },
      onDragEnd: () => {
        setDragging(null);
        setOver(null);
      },
      style: marked ? { boxShadow: `inset 0 ${over.after ? -2 : 2}px 0 0 #0b5d57` } : undefined,
      title: 'Drag to move it among the rows under the same parent',
    };
  };

  const resetOrder = async () => {
    try {
      await api.resetListOrder(list);
      await order.reload();
      onNotice?.({ kind: 'success', text: 'Back to the usual order.' });
    } catch (err) {
      onNotice?.({ kind: 'error', text: errorText(err) });
    }
  };

  return {
    view,
    setView,
    tree,
    arranged,
    visible,
    collapsed,
    toggle,
    expandAll: () => setCollapsed(new Set()),
    collapseAll: () => setCollapsed(parentIds(arranged)),
    rowProps,
    customOrder: (order.data ?? []).length > 0,
    resetOrder,
  };
}

/** Flat / Tree, Expand all, Collapse all and Usual order, for a list's toolbar. */
export function ListViewControls({ ctl, label }) {
  return (
    <span className="flex items-center gap-1 text-sm">
      <select className="qb-input h-row w-[84px]" value={ctl.view} onChange={(e) => ctl.setView(e.target.value)} aria-label={`Show the ${label} as`}>
        <option value="tree">Tree</option>
        <option value="flat">Flat</option>
      </select>
      {ctl.tree && (
        <>
          <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={ctl.expandAll} title="Show every subaccount, job or sub-item">
            Expand all
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={ctl.collapseAll} title="Show only the top-level rows">
            Collapse all
          </button>
        </>
      )}
      {ctl.customOrder && (
        <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={ctl.resetOrder} title="Forget the order rows were dragged into">
          Usual order
        </button>
      )}
    </span>
  );
}

/** The ▸ / ▾ before a parent's name in the tree view (a spacer for other rows). */
export function TreeToggle({ ctl, item }) {
  if (!ctl.tree) return null;
  if (!item.hasChildren) return <span className="inline-block w-[14px]" aria-hidden="true" />;
  const open = !ctl.collapsed.has(item.id);
  return (
    <button
      type="button"
      className="inline-block w-[14px] text-center text-2xs text-qb-ink-muted hover:text-qb-ink"
      onClick={(e) => {
        e.stopPropagation();
        ctl.toggle(item.id);
      }}
      onDoubleClick={(e) => e.stopPropagation()}
      aria-expanded={open}
      aria-label={open ? 'Collapse' : 'Expand'}
    >
      {open ? '▾' : '▸'}
    </button>
  );
}
