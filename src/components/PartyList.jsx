import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { ratePercent, useSalesTax } from './SalesTax.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

const CONFIG = {
  customer: { list: api.listCustomers, create: api.createCustomer, noun: 'customer', title: 'Customers', balance: 'Open balance (A/R)' },
  vendor: { list: api.listVendors, create: api.createVendor, noun: 'vendor', title: 'Vendors', balance: 'Open balance (A/P)' },
};

/** The full customer or vendor record: contact details, default terms and (vendors) 1099 status. */
function PartyDialog({ kind, id, onClose, onSaved }) {
  const noun = CONFIG[kind].noun;
  const [form, setForm] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const salesTax = useSalesTax();

  useEffect(() => {
    api.getParty(kind, id).then(
      (p) => setForm({ ...p, termsDays: p.termsDays ?? '' }),
      (err) => setError(errorText(err)),
    );
  }, [kind, id]);

  const set = (key) => (e) => setForm((f) => ({ ...f, [key]: e.target.type === 'checkbox' ? e.target.checked : e.target.value }));
  const submit = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      const terms = String(form.termsDays).trim();
      const saved = await api.updateParty(kind, id, {
        name: form.name,
        companyName: form.companyName,
        email: form.email,
        phone: form.phone,
        address: form.address,
        termsDays: terms === '' ? null : Number(terms),
        notes: form.notes,
        taxId: form.taxId,
        is1099: Boolean(form.is1099),
        taxRateId: form.taxRateId ?? null,
        isTaxExempt: Boolean(form.isTaxExempt),
        taxExemption: form.taxExemption,
      });
      onSaved(`Saved ${noun} ${saved.name}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const field = (label, key, props = {}) => (
    <label className="flex flex-col gap-0.5">
      <span className="font-semibold text-qb-ink-muted">{label}</span>
      <input className="qb-input" value={form[key] ?? ''} onChange={set(key)} {...props} />
    </label>
  );

  return (
    <Dialog title={form ? `${form.name}` : `Edit ${noun}`} onClose={onClose}>
      {!form ? (
        <div className="px-3 py-2 text-sm">{error ? <Notice kind="error" text={error} /> : 'Loading…'}</div>
      ) : (
        <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
          <div className="grid grid-cols-2 gap-2">
            {field(`${noun[0].toUpperCase()}${noun.slice(1)} name`, 'name')}
            {field('Company name', 'companyName')}
            {field('Email', 'email', { type: 'email' })}
            {field('Phone', 'phone', { type: 'tel' })}
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">{kind === 'customer' ? 'Billing address' : 'Address'}</span>
              <textarea className="qb-input h-[54px] py-1" value={form.address ?? ''} onChange={set('address')} />
            </label>
            {field('Terms (days)', 'termsDays', { inputMode: 'numeric', placeholder: kind === 'customer' ? 'e.g. 30 for Net 30' : 'e.g. 30' })}
            {kind === 'vendor' && field('Tax ID (EIN or SSN)', 'taxId')}
            {kind === 'vendor' && (
              <label className="col-span-2 flex items-center gap-1.5 text-sm">
                <input type="checkbox" className="accent-qb-green" checked={Boolean(form.is1099)} onChange={set('is1099')} />
                Track payments for a 1099
              </label>
            )}
            {kind === 'customer' && salesTax.enabled && (
              <>
                <label className="flex flex-col gap-0.5">
                  <span className="font-semibold text-qb-ink-muted">Default sales tax</span>
                  <select
                    className="qb-input"
                    value={form.taxRateId ?? ''}
                    onChange={(e) => setForm((f) => ({ ...f, taxRateId: e.target.value === '' ? null : Number(e.target.value) }))}
                    disabled={form.isTaxExempt}
                  >
                    <option value="">Company default</option>
                    {salesTax.activeRates.map((r) => (
                      <option key={r.id} value={r.id}>{r.name} ({ratePercent(r.ratePpm)})</option>
                    ))}
                  </select>
                </label>
                <label className="flex items-center gap-1.5 self-end pb-1 text-sm">
                  <input type="checkbox" className="accent-qb-green" checked={Boolean(form.isTaxExempt)} onChange={set('isTaxExempt')} />
                  Tax exempt
                </label>
                {form.isTaxExempt && field('Exemption (reason or certificate no.)', 'taxExemption')}
              </>
            )}
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Notes</span>
              <textarea className="qb-input h-[44px] py-1" value={form.notes ?? ''} onChange={set('notes')} />
            </label>
          </div>
          {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
          <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
            <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Save</button>
          </div>
        </form>
      )}
    </Dialog>
  );
}

/** Customer or vendor list with an inline "add" row at the top of the grid. */
export default function PartyList({ kind }) {
  const cfg = CONFIG[kind];
  const { data, error } = useLedgerQuery(cfg.list);
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [query, setQuery] = useState('');
  const [showInactive, setShowInactive] = useState(false);
  const [editing, setEditing] = useState(null);

  const setActive = async (p, active) => {
    try {
      await api.setPartyActive(kind, p.id, active);
      setNotice({ kind: 'success', text: active ? `${p.name} is active again.` : `${p.name} is inactive: hidden from pickers, with its history kept.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: `Enter a ${cfg.noun} name.` });
    setBusy(true);
    try {
      const created = await cfg.create(name.trim(), email.trim());
      setNotice({ kind: 'success', text: `Added ${cfg.noun} ${created.name}.` });
      setName('');
      setEmail('');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const rows = (data ?? []).filter((p) => (showInactive || p.isActive) && p.name.toLowerCase().includes(query.trim().toLowerCase()));
  const inactiveCount = (data ?? []).filter((p) => !p.isActive).length;
  const total = rows.reduce((s, p) => s + p.openBalanceCents, 0);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {editing && (
        <PartyDialog
          kind={kind}
          id={editing}
          onClose={() => setEditing(null)}
          onSaved={(text) => {
            setEditing(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">{cfg.title}</h2>
          <input className="qb-input h-row w-[220px] text-sm" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Find a ${cfg.noun}`} aria-label="Filter" />
          <label className="flex items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive{inactiveCount > 0 ? ` (${inactiveCount})` : ''}
          </label>
          <span className="ml-auto text-sm text-qb-ink-muted">
            {rows.length} {cfg.noun}s · open <span className="num font-semibold text-qb-ink">{formatCents(total)}</span>
          </span>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <form onSubmit={add}>
            <table className="qb-grid">
              <colgroup>
                <col />
                <col className="w-[280px]" />
                <col className="w-[160px]" />
                <col className="w-[96px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head">Name</th>
                  <th className="qb-grid-head">Email</th>
                  <th className="qb-grid-head text-right">{cfg.balance}</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  <td className="qb-grid-cell-edit">
                    <input value={name} onChange={(e) => setName(e.target.value)} placeholder={`New ${cfg.noun} name`} aria-label="Name" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="name@example.com" aria-label="Email" />
                  </td>
                  <td className="qb-grid-cell" />
                  <td className="qb-grid-cell cell-tight">
                    <button type="submit" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                      Add
                    </button>
                  </td>
                </tr>
                {data && rows.length === 0 && (
                  <tr>
                    <td colSpan={4} className="qb-grid-cell text-qb-ink-muted">
                      {query ? `No ${cfg.noun}s match “${query}”.` : `No ${cfg.noun}s yet. They are also created automatically when you post a transaction.`}
                    </td>
                  </tr>
                )}
                {rows.map((p) => (
                  <tr key={p.id} className={`qb-grid-row ${p.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing(p.id)}>
                    <td className="qb-grid-cell font-semibold">
                      {p.name}
                      {!p.isActive && <span className="ml-1 text-2xs font-normal uppercase tracking-wide">Inactive</span>}
                    </td>
                    <td className="qb-grid-cell text-qb-ink-muted">{p.email ?? ''}</td>
                    <td className={`qb-grid-cell num ${p.openBalanceCents < 0 ? 'text-qb-green-active' : ''}`} title={p.openBalanceCents < 0 ? 'Credit available' : undefined}>
                      {p.openBalanceCents ? formatCents(p.openBalanceCents) : ''}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${p.name}`}
                        items={[
                          { label: 'Edit…', onSelect: () => setEditing(p.id) },
                          p.isActive ? { label: 'Make inactive', onSelect: () => setActive(p, false) } : { label: 'Make active', onSelect: () => setActive(p, true) },
                        ]}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </form>
        </div>
      </section>
    </div>
  );
}
