import { useState } from 'react';
import { createPortal } from 'react-dom';
import { Dialog } from './InventoryDialog.jsx';
import { formatCents, formatQuantity } from '../lib/money.js';

/**
 * Find & Select Items, from the item box on a form: look for items by name or description, tick
 * several, and add a line for each. A portal, since the item box sits in a table.
 */
export default function FindItemsDialog({ products, onClose, onAdd }) {
  const [text, setText] = useState('');
  const [ticked, setTicked] = useState([]);
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = products.filter((p) => words.every((w) => [p.name, p.description].some((v) => (v ?? '').toLowerCase().includes(w))));
  const toggle = (id) => setTicked((t) => (t.includes(id) ? t.filter((x) => x !== id) : [...t, id]));
  return createPortal(
    <Dialog title="Find & Select Items" onClose={onClose}>
      <form
        className="flex w-[560px] max-w-full flex-col gap-2 px-3 py-2 text-xs"
        onSubmit={(e) => {
          e.preventDefault();
          if (ticked.length > 0) onAdd(ticked);
        }}
      >
        <input className="qb-input" value={text} onChange={(e) => setText(e.target.value)} placeholder="Look for an item by name or description" aria-label="Look for" autoFocus />
        <div className="max-h-[300px] overflow-auto border border-qb-border scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[28px]" aria-label="Tick" />
                <th className="qb-grid-head">Item</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head w-[90px] text-right">Price</th>
              </tr>
            </thead>
            <tbody>
              {shown.length === 0 && (
                <tr>
                  <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">No item matches.</td>
                </tr>
              )}
              {shown.map((p) => (
                <tr key={p.id} className="qb-grid-row cursor-pointer" onClick={() => toggle(p.id)}>
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" className="accent-qb-green" checked={ticked.includes(p.id)} onChange={() => toggle(p.id)} onClick={(e) => e.stopPropagation()} aria-label={`Select ${p.name}`} />
                  </td>
                  <td className="qb-grid-cell font-semibold">
                    {p.name}
                    {p.qtyOnHandE4 != null && <span className="ml-1 font-normal text-qb-ink-muted">{formatQuantity(p.qtyOnHandE4)} on hand</span>}
                  </td>
                  <td className="qb-grid-cell text-qb-ink-muted">{p.description ?? ''}</td>
                  <td className="qb-grid-cell num">{p.rateCents ? formatCents(p.rateCents) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-qb-border pt-2">
          <span className="mr-auto text-qb-ink-muted">{ticked.length} selected</span>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={ticked.length === 0}>
            Add {ticked.length > 0 ? ticked.length : ''} line{ticked.length === 1 ? '' : 's'}
          </button>
        </div>
      </form>
    </Dialog>,
    document.body,
  );
}
