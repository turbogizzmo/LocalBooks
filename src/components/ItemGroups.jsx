import { useState } from 'react';
import Notice from './Notice.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatQuantity, parseQuantityE4 } from '../lib/money.js';

const blankMember = () => ({ productId: null, qty: '1' });

/** One group's name, description, print setting and items. */
function GroupForm({ group, products, onDone, onCancel }) {
  const [name, setName] = useState(group?.name ?? '');
  const [description, setDescription] = useState(group?.description ?? '');
  const [printItems, setPrintItems] = useState(group?.printItems ?? true);
  const [members, setMembers] = useState(() =>
    group ? group.members.map((m) => ({ productId: m.productId, qty: formatQuantity(m.quantityE4).replace(/,/g, '') })) : [blankMember(), blankMember()],
  );
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  // Groups hold services, parts and fixed charges, not discounts or percent charges.
  const choices = products.filter((p) => (p.isActive || members.some((m) => m.productId === p.id)) && p.itemType !== 'DISCOUNT' && !p.percentPpm);
  const setMember = (i, patch) => setMembers((ms) => ms.map((m, j) => (j === i ? { ...m, ...patch } : m)));

  const save = async (e) => {
    e.preventDefault();
    e.stopPropagation();
    const filled = members.filter((m) => m.productId !== null);
    if (!name.trim()) return setError('Enter a name for the group.');
    if (filled.length === 0) return setError('Add at least one item to the group.');
    const parsed = filled.map((m) => ({ productId: m.productId, quantityE4: parseQuantityE4(m.qty) }));
    if (parsed.some((m) => m.quantityE4 === null || m.quantityE4 <= 0)) return setError('Each item needs a quantity above zero.');
    setBusy(true);
    try {
      const saved = await api.saveItemGroup(group?.id ?? null, { name: name.trim(), description: description.trim() || null, printItems, members: parsed });
      onDone(`${group ? 'Saved' : 'Added'} the ${saved.name} group.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} className="flex flex-col gap-2 px-3 py-2 text-xs">
      <div className="grid grid-cols-2 gap-2">
        <label className="flex flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">Group name</span>
          <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">Description on sales forms</span>
          <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <label className="col-span-2 flex items-center gap-1.5 text-sm">
          <input type="checkbox" className="accent-qb-green" checked={!printItems} onChange={(e) => setPrintItems(!e.target.checked)} />
          Print as one line: printed forms show the group’s description and total, not its items
        </label>
      </div>
      <table className="qb-grid">
        <colgroup>
          <col />
          <col className="w-[90px]" />
          <col className="w-[28px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Item</th>
            <th className="qb-grid-head text-right">Qty</th>
            <th className="qb-grid-head" aria-label="Remove" />
          </tr>
        </thead>
        <tbody>
          {members.map((m, i) => (
            <tr key={i} className="qb-grid-row">
              <td className="qb-grid-cell-edit">
                <select value={m.productId ?? ''} onChange={(e) => setMember(i, { productId: e.target.value === '' ? null : Number(e.target.value) })} aria-label={`Item ${i + 1}`}>
                  <option value="">—</option>
                  {choices.map((p) => (
                    <option key={p.id} value={p.id}>{p.name}</option>
                  ))}
                </select>
              </td>
              <td className="qb-grid-cell-edit">
                <input className="num" inputMode="decimal" data-quantity value={m.qty} onChange={(e) => setMember(i, { qty: e.target.value })} aria-label={`Quantity ${i + 1}`} />
              </td>
              <td className="qb-grid-cell cell-flush text-center">
                <button type="button" className="h-row w-full text-qb-ink-faint hover:text-qb-negative" onClick={() => setMembers((ms) => ms.filter((_, j) => j !== i))} aria-label={`Remove item ${i + 1}`}>
                  ×
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setMembers((ms) => [...ms, blankMember()])}>
          Add item
        </button>
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
        <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onCancel}>Back</button>
        <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>{group ? 'Save' : 'Add group'}</button>
      </div>
    </form>
  );
}

/**
 * Groups: a named set of items with quantities. Choosing a group in a sales form's item column
 * fills in a line for each of its items.
 */
export default function ItemGroups({ products, onClose }) {
  const groups = useLedgerQuery(api.listItemGroups);
  const [editing, setEditing] = useState(null);
  const [notice, setNotice] = useState(null);
  const setActive = async (g, active) => {
    try {
      await api.setItemGroupActive(g.id, active);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <Dialog title={editing ? (editing === 'new' ? 'New group' : `Edit ${editing.name}`) : 'Groups'} onClose={onClose}>
      {editing ? (
        <GroupForm
          group={editing === 'new' ? null : editing}
          products={products}
          onCancel={() => setEditing(null)}
          onDone={(text) => {
            setEditing(null);
            setNotice({ kind: 'success', text });
          }}
        />
      ) : (
        <div className="flex min-w-[520px] flex-col gap-2 px-3 py-2 text-xs">
          <p className="text-qb-ink-muted">Choose a group in the item column of an invoice, sales receipt or estimate to fill in a line for each of its items.</p>
          {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Group</th>
                <th className="qb-grid-head">Items</th>
                <th className="qb-grid-head">Prints</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {(groups.data ?? []).length === 0 && (
                <tr>
                  <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">No groups yet.</td>
                </tr>
              )}
              {(groups.data ?? []).map((g) => (
                <tr key={g.id} className={`qb-grid-row ${g.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing(g)}>
                  <td className="qb-grid-cell font-semibold">{g.name}</td>
                  <td className="qb-grid-cell" title={g.members.map((m) => `${formatQuantity(m.quantityE4)} × ${m.productName}`).join(', ')}>
                    {g.members.map((m) => `${formatQuantity(m.quantityE4)} × ${m.productName}`).join(', ')}
                  </td>
                  <td className="qb-grid-cell">{g.printItems ? 'Each item' : 'One line'}</td>
                  <td className="qb-grid-cell cell-tight whitespace-nowrap">
                    <button type="button" className="qb-btn qb-btn-secondary mr-1 h-[20px] text-xs" onClick={() => setEditing(g)}>Edit…</button>
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] text-xs" onClick={() => setActive(g, !g.isActive)}>
                      {g.isActive ? 'Make inactive' : 'Make active'}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="flex justify-between border-t border-qb-border pt-2">
            <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={() => setEditing('new')}>New group…</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Close</button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
