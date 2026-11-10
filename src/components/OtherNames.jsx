import { useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { OtherNameInfoPanel, useInfoPanel } from './InfoPanel.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

/**
 * Expenses › Other Names (Lists › Other Names List): people and businesses who are neither
 * customers nor vendors, such as an owner taking a draw or a one-off payee. A check or expense
 * written to one of these names is paid to them. Click a name for its information panel.
 */
const FIELDS = [
  ['name', 'Name'],
  ['companyName', 'Company name'],
  ['email', 'Email'],
  ['phone', 'Phone'],
];

function OtherNameDialog({ record, onClose, onSaved }) {
  const [form, setForm] = useState({ name: '', companyName: '', email: '', phone: '', address: '', notes: '', ...record });
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const set = (key) => (e) => setForm((f) => ({ ...f, [key]: e.target.value }));
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      const input = { name: form.name, companyName: form.companyName, email: form.email, phone: form.phone, address: form.address, notes: form.notes };
      const saved = record?.id ? await api.updateOtherName(record.id, input) : await api.createOtherName(input);
      onSaved(`${record?.id ? 'Saved' : 'Added'} ${saved.name}.`, saved);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog title={record?.id ? record.name : 'New other name'} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          {FIELDS.map(([key, label]) => (
            <label key={key} className="flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">{label}</span>
              <input className="qb-input" value={form[key] ?? ''} onChange={set(key)} type={key === 'email' ? 'email' : key === 'phone' ? 'tel' : 'text'} autoFocus={key === 'name'} />
            </label>
          ))}
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Address</span>
            <textarea className="qb-input h-[54px] py-1" value={form.address ?? ''} maxLength={500} onChange={set('address')} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Notes</span>
            <textarea className="qb-input h-[54px] py-1" value={form.notes ?? ''} maxLength={10000} onChange={set('notes')} />
          </label>
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>
            Save
          </button>
        </div>
      </form>
    </Dialog>
  );
}

export default function OtherNames() {
  const { data, error } = useLedgerQuery(api.listOtherNames);
  const [editing, setEditing] = useState(null); // a record, or {} for a new one
  const [selected, setSelected] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const [notice, setNotice] = useState(null);
  const info = useInfoPanel('other');
  const rows = (data ?? []).filter((o) => showInactive || o.isActive);
  const setActive = async (o, active) => {
    try {
      await api.setOtherNameActive(o.id, active);
      setNotice({ kind: 'success', text: active ? `${o.name} is active again.` : `${o.name} is inactive: hidden from payee lists, with its history kept.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  return (
    <div className="flex h-full gap-2 p-2">
      {editing && (
        <OtherNameDialog
          record={editing.id ? editing : null}
          onClose={() => setEditing(null)}
          onSaved={(text, saved) => {
            setEditing(null);
            setSelected(saved.id);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Other Names</h2>
          <span className="min-w-0 truncate text-xs text-qb-ink-muted">Neither customers nor vendors, such as an owner taking a draw. Write a check to one by typing the name as its payee.</span>
          <label className="ml-auto flex flex-none items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive
          </label>
          {info.button}
          <button type="button" className="qb-btn qb-btn-primary h-row flex-none px-2" onClick={() => setEditing({})}>
            New other name
          </button>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head">Name</th>
                <th className="qb-grid-head">Company</th>
                <th className="qb-grid-head">Phone</th>
                <th className="qb-grid-head">Email</th>
                <th className="qb-grid-head w-[120px] text-right">Paid to them</th>
                <th className="qb-grid-head w-[30px]" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">
                    No other names yet. Add one for anyone you pay who isn’t a vendor, such as an owner.
                  </td>
                </tr>
              )}
              {rows.map((o) => (
                <tr
                  key={o.id}
                  className={`qb-grid-row ${o.id === selected ? 'is-active' : ''} ${o.isActive ? '' : 'text-qb-ink-faint'}`}
                  aria-selected={o.id === selected}
                  onClick={() => setSelected(o.id)}
                  onDoubleClick={() => setEditing(o)}
                >
                  <td className="qb-grid-cell font-semibold">
                    {o.name}
                    {!o.isActive && <span className="ml-1 text-2xs font-normal uppercase tracking-wide">Inactive</span>}
                  </td>
                  <td className="qb-grid-cell">{o.companyName ?? ''}</td>
                  <td className="qb-grid-cell">{o.phone ?? ''}</td>
                  <td className="qb-grid-cell">{o.email ?? ''}</td>
                  <td className="qb-grid-cell num">{o.paidCents ? formatCents(o.paidCents) : ''}</td>
                  <td className="qb-grid-cell cell-tight text-center">
                    <RowMenu
                      label={`Actions for ${o.name}`}
                      items={[
                        { label: 'Edit…', onSelect: () => setEditing(o) },
                        o.isActive ? { label: 'Make inactive', onSelect: () => setActive(o, false) } : { label: 'Make active', onSelect: () => setActive(o, true) },
                      ]}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
      {info.shown && selected !== null && <OtherNameInfoPanel id={selected} onHide={info.hide} />}
    </div>
  );
}
