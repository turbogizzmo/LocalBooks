import { useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';

export const FIELD_LISTS = [
  { key: 'CUSTOMER', label: 'Customers' },
  { key: 'VENDOR', label: 'Vendors' },
  { key: 'ITEM', label: 'Items' },
];

/** Runs a list action and reports how it went. */
function useRunner() {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const run = async (action, success) => {
    setBusy(true);
    try {
      const result = await action();
      setNotice({ kind: 'success', text: success(result) });
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, notice, setNotice, run };
}

function Panel({ title, hint, notice, onDismiss, error, children, toolbar }) {
  return (
    <section className="qb-panel flex min-h-0 flex-col">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">{title}</h2>
        <span className="min-w-0 truncate text-xs text-qb-ink-muted" title={hint}>{hint}</span>
        {toolbar && <span className="ml-auto flex flex-none items-center gap-2">{toolbar}</span>}
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={onDismiss} />}
      {error && <Notice kind="error" text={error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">{children}</div>
    </section>
  );
}

function InactiveToggle({ value, onChange, count }) {
  return (
    <label className="flex items-center gap-1 text-xs">
      <input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} />
      Show inactive{count > 0 ? ` (${count})` : ''}
    </label>
  );
}

/** Initials and an optional name, like QuickBooks' Sales Rep list. */
function SalesReps() {
  const { data, error } = useLedgerQuery(api.listSalesReps);
  const reps = data ?? [];
  const { busy, notice, setNotice, run } = useRunner();
  const [initials, setInitials] = useState('');
  const [name, setName] = useState('');
  const [editing, setEditing] = useState(null);
  const [showInactive, setShowInactive] = useState(false);

  const add = async (e) => {
    e.preventDefault();
    if (!initials.trim()) return setNotice({ kind: 'error', text: "Enter the rep's initials." });
    if (await run(() => api.createSalesRep({ initials, name }), (r) => `Added rep ${r.initials}.`)) {
      setInitials('');
      setName('');
    }
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.updateSalesRep(editing.id, { initials: editing.initials, name: editing.name }), (r) => `Saved rep ${r.initials}.`)) setEditing(null);
  };
  const shown = reps.filter((r) => showInactive || r.isActive);

  return (
    <Panel
      title="Sales Reps"
      hint="Initials print in the Rep box on invoices, sales receipts and estimates; a customer can have a default rep."
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load reps: ${error}`}
      toolbar={<InactiveToggle value={showInactive} onChange={setShowInactive} count={reps.filter((r) => !r.isActive).length} />}
    >
      <table className="qb-grid">
        <colgroup>
          <col className="w-[84px]" />
          <col />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Initials</th>
            <th className="qb-grid-head">Name</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          <tr className="bg-qb-row-selected">
            <td className="qb-grid-cell-edit">
              <form id="new-rep" onSubmit={add} />
              <input form="new-rep" value={initials} onChange={(e) => setInitials(e.target.value)} maxLength={5} placeholder="JS" aria-label="Initials" />
            </td>
            <td className="qb-grid-cell-edit">
              <input form="new-rep" value={name} onChange={(e) => setName(e.target.value)} maxLength={41} placeholder="Full name (optional)" aria-label="Rep name" />
            </td>
            <td className="qb-grid-cell cell-tight">
              <button type="submit" form="new-rep" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Add</button>
            </td>
          </tr>
          {data && shown.length === 0 && (
            <tr>
              <td colSpan={3} className="qb-grid-cell text-qb-ink-muted">No sales reps. Add one and a Rep box appears on your sales forms.</td>
            </tr>
          )}
          {shown.map((r) =>
            editing?.id === r.id ? (
              <tr key={r.id} className="qb-grid-row is-active">
                <td className="qb-grid-cell-edit">
                  <form id="edit-rep" onSubmit={save} />
                  <input form="edit-rep" value={editing.initials} maxLength={5} onChange={(e) => setEditing({ ...editing, initials: e.target.value })} aria-label="Initials" autoFocus
                    onKeyDown={(e) => e.key === 'Escape' && setEditing(null)} />
                </td>
                <td className="qb-grid-cell-edit">
                  <input form="edit-rep" value={editing.name} maxLength={41} onChange={(e) => setEditing({ ...editing, name: e.target.value })} aria-label="Rep name"
                    onKeyDown={(e) => e.key === 'Escape' && setEditing(null)} />
                </td>
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form="edit-rep" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Save</button>
                </td>
              </tr>
            ) : (
              <tr key={r.id} className={`qb-grid-row ${r.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing({ id: r.id, initials: r.initials, name: r.name ?? '' })}>
                <td className="qb-grid-cell font-semibold">{r.initials}</td>
                <td className="qb-grid-cell">
                  {r.name ?? ''}
                  {!r.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                </td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${r.initials}`}
                    items={[
                      { label: 'Edit', onSelect: () => setEditing({ id: r.id, initials: r.initials, name: r.name ?? '' }) },
                      r.isActive
                        ? { label: 'Make inactive', onSelect: () => run(() => api.setSalesRepActive(r.id, false), () => `${r.initials} is inactive; forms that have it keep it.`) }
                        : { label: 'Make active', onSelect: () => run(() => api.setSalesRepActive(r.id, true), () => `${r.initials} is active again.`) },
                      {
                        label: 'Delete',
                        danger: true,
                        disabled: r.isUsed,
                        hint: r.isUsed ? 'On customers or sales forms: make it inactive instead' : undefined,
                        onSelect: () => run(() => api.deleteSalesRep(r.id), () => `Deleted rep ${r.initials}.`),
                        confirm: { title: `Delete rep ${r.initials}?`, body: 'No customer or sales form uses this rep.', confirmLabel: 'Delete rep' },
                      },
                    ]}
                  />
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </Panel>
  );
}

/** The Ship Via list. */
function ShipMethods() {
  const { data, error } = useLedgerQuery(api.listShipMethods);
  const methods = data ?? [];
  const { busy, notice, setNotice, run } = useRunner();
  const [name, setName] = useState('');
  const [editing, setEditing] = useState(null);
  const [showInactive, setShowInactive] = useState(false);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a shipping method.' });
    if (await run(() => api.createShipMethod(name), (m) => `Added ${m.name}.`)) setName('');
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.renameShipMethod(editing.id, editing.name), (m) => `Saved ${m.name}.`)) setEditing(null);
  };
  const shown = methods.filter((m) => showInactive || m.isActive);

  return (
    <Panel
      title="Ship Via"
      hint="Shipping methods for the Ship via box; enter a ship date beside it."
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load shipping methods: ${error}`}
      toolbar={<InactiveToggle value={showInactive} onChange={setShowInactive} count={methods.filter((m) => !m.isActive).length} />}
    >
      <table className="qb-grid">
        <colgroup>
          <col />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Shipping method</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          <tr className="bg-qb-row-selected">
            <td className="qb-grid-cell-edit">
              <form id="new-ship" onSubmit={add} />
              <input form="new-ship" value={name} onChange={(e) => setName(e.target.value)} maxLength={31} placeholder="e.g. UPS Ground" aria-label="Shipping method" />
            </td>
            <td className="qb-grid-cell cell-tight">
              <button type="submit" form="new-ship" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Add</button>
            </td>
          </tr>
          {data && shown.length === 0 && (
            <tr>
              <td colSpan={2} className="qb-grid-cell text-qb-ink-muted">No shipping methods. Add one and Ship date and Ship via boxes appear on your sales forms.</td>
            </tr>
          )}
          {shown.map((m) =>
            editing?.id === m.id ? (
              <tr key={m.id} className="qb-grid-row is-active">
                <td className="qb-grid-cell-edit">
                  <form id="edit-ship" onSubmit={save} />
                  <input form="edit-ship" value={editing.name} maxLength={31} onChange={(e) => setEditing({ ...editing, name: e.target.value })} aria-label="Shipping method" autoFocus
                    onKeyDown={(e) => e.key === 'Escape' && setEditing(null)} />
                </td>
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form="edit-ship" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Save</button>
                </td>
              </tr>
            ) : (
              <tr key={m.id} className={`qb-grid-row ${m.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing({ id: m.id, name: m.name })}>
                <td className="qb-grid-cell">
                  {m.name}
                  {!m.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                </td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${m.name}`}
                    items={[
                      { label: 'Rename', onSelect: () => setEditing({ id: m.id, name: m.name }) },
                      m.isActive
                        ? { label: 'Make inactive', onSelect: () => run(() => api.setShipMethodActive(m.id, false), () => `${m.name} is inactive.`) }
                        : { label: 'Make active', onSelect: () => run(() => api.setShipMethodActive(m.id, true), () => `${m.name} is active again.`) },
                      {
                        label: 'Delete',
                        danger: true,
                        disabled: m.isUsed,
                        hint: m.isUsed ? 'On sales forms: make it inactive instead' : undefined,
                        onSelect: () => run(() => api.deleteShipMethod(m.id), () => `Deleted ${m.name}.`),
                        confirm: { title: `Delete ${m.name}?`, body: 'No sales form uses this shipping method.', confirmLabel: 'Delete' },
                      },
                    ]}
                  />
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </Panel>
  );
}

/** Define Fields: up to 15 text fields per list. */
function CustomFields() {
  const { data, error } = useLedgerQuery(api.listCustomFields);
  const fields = data ?? [];
  const { busy, notice, setNotice, run } = useRunner();
  const [list, setList] = useState('CUSTOMER');
  const [label, setLabel] = useState('');
  const [onForms, setOnForms] = useState(false);
  const [editing, setEditing] = useState(null);
  const shown = fields.filter((f) => f.list === list);
  const formsAllowed = list !== 'VENDOR';
  const noun = FIELD_LISTS.find((l) => l.key === list).label.toLowerCase();

  const add = async (e) => {
    e.preventDefault();
    if (!label.trim()) return setNotice({ kind: 'error', text: 'Enter a label for the field.' });
    if (await run(() => api.createCustomField(list, { label, onSalesForms: formsAllowed && onForms }), (f) => `Added ${f.label} to ${noun}.`)) {
      setLabel('');
      setOnForms(false);
    }
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.updateCustomField(editing.id, { label: editing.label, onSalesForms: editing.onSalesForms }), (f) => `Saved ${f.label}.`)) setEditing(null);
  };
  const formsHint = list === 'CUSTOMER' ? 'Filled in from the customer on new sales forms, editable there, and printed' : 'Printed under each line that sells the item';

  return (
    <Panel
      title="Custom Fields"
      hint="Your own text fields on customers, vendors and items: up to 15 per list, filled in on each record's Edit window."
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load custom fields: ${error}`}
      toolbar={
        <span className="flex overflow-hidden rounded-xs border border-qb-border" role="tablist" aria-label="List">
          {FIELD_LISTS.map((l) => (
            <button
              key={l.key}
              type="button"
              role="tab"
              aria-selected={list === l.key}
              className={`px-2 py-0.5 text-xs ${list === l.key ? 'bg-qb-green text-white' : 'bg-white hover:bg-qb-canvas'}`}
              onClick={() => {
                setList(l.key);
                setEditing(null);
              }}
            >
              {l.label} ({fields.filter((f) => f.list === l.key).length})
            </button>
          ))}
        </span>
      }
    >
      <table className="qb-grid">
        <colgroup>
          <col className="w-[36px]" />
          <col />
          {formsAllowed && <col className="w-[120px]" />}
          <col className="w-[80px]" />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head text-right">#</th>
            <th className="qb-grid-head">Label</th>
            {formsAllowed && <th className="qb-grid-head" title={formsHint}>On sales forms</th>}
            <th className="qb-grid-head text-right">In use</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {shown.length < 15 && (
            <tr className="bg-qb-row-selected">
              <td className="qb-grid-cell" />
              <td className="qb-grid-cell-edit">
                <form id="new-field" onSubmit={add} />
                <input form="new-field" value={label} onChange={(e) => setLabel(e.target.value)} maxLength={31} placeholder={list === 'ITEM' ? 'e.g. Color' : 'e.g. Region'} aria-label="Field label" />
              </td>
              {formsAllowed && (
                <td className="qb-grid-cell">
                  <label className="flex items-center gap-1 text-xs" title={formsHint}>
                    <input type="checkbox" form="new-field" className="accent-qb-green" checked={onForms} onChange={(e) => setOnForms(e.target.checked)} />
                    Show
                  </label>
                </td>
              )}
              <td className="qb-grid-cell" />
              <td className="qb-grid-cell cell-tight">
                <button type="submit" form="new-field" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Add</button>
              </td>
            </tr>
          )}
          {data && shown.length === 0 && (
            <tr>
              <td colSpan={formsAllowed ? 5 : 4} className="qb-grid-cell text-qb-ink-muted">No custom fields for {noun} yet.</td>
            </tr>
          )}
          {shown.map((f) =>
            editing?.id === f.id ? (
              <tr key={f.id} className="qb-grid-row is-active">
                <td className="qb-grid-cell num">{f.position}</td>
                <td className="qb-grid-cell-edit">
                  <form id="edit-field" onSubmit={save} />
                  <input form="edit-field" value={editing.label} maxLength={31} onChange={(e) => setEditing({ ...editing, label: e.target.value })} aria-label="Field label" autoFocus
                    onKeyDown={(e) => e.key === 'Escape' && setEditing(null)} />
                </td>
                {formsAllowed && (
                  <td className="qb-grid-cell">
                    <label className="flex items-center gap-1 text-xs">
                      <input type="checkbox" className="accent-qb-green" checked={editing.onSalesForms} onChange={(e) => setEditing({ ...editing, onSalesForms: e.target.checked })} />
                      Show
                    </label>
                  </td>
                )}
                <td className="qb-grid-cell" />
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form="edit-field" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Save</button>
                </td>
              </tr>
            ) : (
              <tr key={f.id} className="qb-grid-row" onDoubleClick={() => setEditing({ id: f.id, label: f.label, onSalesForms: f.onSalesForms })}>
                <td className="qb-grid-cell num text-qb-ink-muted">{f.position}</td>
                <td className="qb-grid-cell">{f.label}</td>
                {formsAllowed && <td className="qb-grid-cell text-sm">{f.onSalesForms ? 'Shown' : ''}</td>}
                <td className="qb-grid-cell num text-qb-ink-muted">{f.valueCount || ''}</td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${f.label}`}
                    items={[
                      { label: 'Edit', onSelect: () => setEditing({ id: f.id, label: f.label, onSalesForms: f.onSalesForms }) },
                      {
                        label: 'Delete',
                        danger: true,
                        disabled: f.isPrinted,
                        hint: f.isPrinted ? 'Printed on issued forms: take it off sales forms instead' : undefined,
                        onSelect: () => run(() => api.deleteCustomField(f.id), () => `Deleted ${f.label}.`),
                        confirm: {
                          title: `Delete ${f.label}?`,
                          body: f.valueCount ? `This also clears it on ${f.valueCount} ${f.valueCount === 1 ? 'record' : 'records'}.` : 'No record has a value in this field.',
                          confirmLabel: 'Delete field',
                        },
                      },
                    ]}
                  />
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
      {shown.length >= 15 && <p className="px-2 py-1 text-xs text-qb-ink-muted">This list has the most fields it can have (15).</p>}
    </Panel>
  );
}

/** Sales reps, shipping methods and custom fields: lists that are optional and only show on forms once used. */
export default function SalesLists() {
  return (
    <div className="grid h-full min-h-0 grid-cols-1 gap-2 p-2 lg:grid-cols-[minmax(0,1fr)_minmax(0,0.8fr)_minmax(0,1.3fr)]">
      <SalesReps />
      <ShipMethods />
      <CustomFields />
    </div>
  );
}

/** The record's custom fields as inputs; renders nothing when the list has none. */
export function CustomFieldInputs({ fields, onChange }) {
  if (!fields || fields.length === 0) return null;
  return (
    <fieldset className="col-span-2 grid grid-cols-2 gap-2 border-t border-qb-border pt-2">
      <legend className="px-0.5 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Custom fields</legend>
      {fields.map((f) => (
        <label key={f.fieldId} className="flex flex-col gap-0.5">
          <span className="font-semibold text-qb-ink-muted">
            {f.label}
            {f.onSalesForms && <span className="ml-1 font-normal text-qb-ink-faint" title="Printed on sales forms">(on forms)</span>}
          </span>
          <input className="qb-input" value={f.value} maxLength={100} onChange={(e) => onChange(fields.map((x) => (x.fieldId === f.fieldId ? { ...x, value: e.target.value } : x)))} />
        </label>
      ))}
    </fieldset>
  );
}
