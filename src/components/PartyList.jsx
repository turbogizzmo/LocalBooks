import { useEffect, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate, tableExports } from './ReportParts.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { ratePercent, useSalesTax } from './SalesTax.jsx';
import { CustomFieldInputs } from './SalesLists.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

const CONFIG = {
  customer: { list: api.listCustomers, create: api.createCustomer, noun: 'customer', title: 'Customers', balance: 'Open balance (A/R)' },
  vendor: { list: api.listVendors, create: api.createVendor, noun: 'vendor', title: 'Vendors', balance: 'Open balance (A/P)' },
};

/**
 * A vendor's tax ID. Shown masked until opened; locked (masked, read-only) for users who may not
 * see sensitive data.
 */
function TaxIdField({ form, setForm }) {
  const [open, setOpen] = useState(!form.taxIdMasked);
  if (form.taxIdLocked) {
    return (
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Tax ID (EIN or SSN)</span>
        <input className="qb-input bg-qb-canvas" value={form.taxIdMasked ?? ''} placeholder="None" readOnly aria-describedby="tax-id-locked" />
        <span id="tax-id-locked" className="text-2xs text-qb-ink-faint">Only users who can see sensitive data can view or change it.</span>
      </label>
    );
  }
  return (
    <label className="flex flex-col gap-0.5">
      <span className="font-semibold text-qb-ink-muted">Tax ID (EIN or SSN)</span>
      {open ? (
        <input className="qb-input" value={form.taxId ?? ''} onChange={(e) => setForm((f) => ({ ...f, taxId: e.target.value }))} autoComplete="off" spellCheck={false} />
      ) : (
        <span className="flex gap-1">
          <input className="qb-input min-w-0 flex-1 bg-qb-canvas" value={form.taxIdMasked} readOnly />
          <button type="button" className="qb-btn qb-btn-secondary flex-none px-2" onClick={() => setOpen(true)}>
            Show
          </button>
        </span>
      )}
    </label>
  );
}

/** Whether a routing number passes its ABA check digit (weights 3, 7, 1). */
export const routingNumberOk = (routing) =>
  /^\d{9}$/.test(routing) && [...routing].reduce((sum, d, i) => sum + Number(d) * [3, 7, 1][i % 3], 0) % 10 === 0;

/**
 * A vendor's bank details for paying by ACH file. Masked until opened; locked (masked, read-only)
 * for users who may not see sensitive data. `bank.draft` holds what the user typed; `null` there
 * removes the details on save.
 */
function BankDetailsFields({ bank, setBank }) {
  const view = bank.view;
  if (view?.locked) {
    return (
      <div className="col-span-2 flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Bank details (ACH)</span>
        <input className="qb-input bg-qb-canvas" value={`Routing ${view.routingMasked} · Account ${view.accountMasked} · ${view.accountType.toLowerCase()}`} readOnly aria-describedby="bank-locked" />
        <span id="bank-locked" className="text-2xs text-qb-ink-faint">Only users who can see sensitive data can view or change them.</span>
      </div>
    );
  }
  const draft = bank.draft;
  if (draft === undefined) {
    return (
      <div className="col-span-2 flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Bank details (ACH)</span>
        <span className="flex gap-1">
          <input
            className="qb-input min-w-0 flex-1 bg-qb-canvas"
            value={view ? `Routing ${view.routingMasked} · Account ${view.accountMasked} · ${view.accountType.toLowerCase()} · ${view.secCode === 'PPD' ? 'person' : 'business'}` : ''}
            placeholder="None: add them to pay this vendor by ACH file"
            readOnly
          />
          <button
            type="button"
            className="qb-btn qb-btn-secondary flex-none px-2"
            onClick={() =>
              setBank((b) => ({
                ...b,
                draft: {
                  routingNumber: view?.routingNumber ?? '',
                  accountNumber: view?.accountNumber ?? '',
                  accountType: view?.accountType ?? 'CHECKING',
                  secCode: view?.secCode ?? 'CCD',
                },
              }))
            }
          >
            {view ? 'Show / change' : 'Add'}
          </button>
        </span>
      </div>
    );
  }
  if (draft === null) {
    return (
      <div className="col-span-2 flex items-center gap-2">
        <span className="font-semibold text-qb-ink-muted">Bank details (ACH)</span>
        <span className="text-qb-ink-muted">Removed when you save.</span>
        <button type="button" className="qb-btn qb-btn-secondary px-2" onClick={() => setBank((b) => ({ ...b, draft: undefined }))}>
          Undo
        </button>
      </div>
    );
  }
  const set = (key) => (e) => setBank((b) => ({ ...b, draft: { ...b.draft, [key]: e.target.value } }));
  const routing = draft.routingNumber.replace(/[\s-]/g, '');
  const routingBad = routing !== '' && !routingNumberOk(routing);
  return (
    <fieldset className="col-span-2 grid grid-cols-2 gap-2 rounded border border-qb-border p-2">
      <legend className="px-1 font-semibold text-qb-ink-muted">Bank details (ACH)</legend>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Routing number</span>
        <input className="qb-input num" value={draft.routingNumber} onChange={set('routingNumber')} inputMode="numeric" autoComplete="off" spellCheck={false} aria-invalid={routingBad} placeholder="9 digits" />
        {routingBad && <span className="text-2xs text-qb-negative">This routing number fails its check digit.</span>}
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Account number</span>
        <input className="qb-input num" value={draft.accountNumber} onChange={set('accountNumber')} autoComplete="off" spellCheck={false} />
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Account type</span>
        <select className="qb-input" value={draft.accountType} onChange={set('accountType')}>
          <option value="CHECKING">Checking</option>
          <option value="SAVINGS">Savings</option>
        </select>
      </label>
      <label className="flex flex-col gap-0.5">
        <span className="font-semibold text-qb-ink-muted">Pay as</span>
        <select className="qb-input" value={draft.secCode} onChange={set('secCode')}>
          <option value="CCD">A business (CCD)</option>
          <option value="PPD">A person (PPD)</option>
        </select>
      </label>
      <div className="col-span-2 flex gap-2">
        <button type="button" className="qb-btn qb-btn-secondary px-2" onClick={() => setBank((b) => ({ ...b, draft: undefined }))}>
          Keep as before
        </button>
        {view && (
          <button type="button" className="qb-btn qb-btn-secondary px-2" onClick={() => setBank((b) => ({ ...b, draft: null }))}>
            Remove bank details
          </button>
        )}
      </div>
    </fieldset>
  );
}

/** The full customer or vendor record: contact details, default terms and (vendors) 1099 status. */
function PartyDialog({ kind, id, onClose, onSaved }) {
  const noun = CONFIG[kind].noun;
  const [form, setForm] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const salesTax = useSalesTax();
  const { data: formStyles } = useLedgerQuery(api.listFormStyles);
  const reps = useLedgerQuery(api.listSalesReps).data ?? [];
  const [fields, setFields] = useState([]);
  // Vendors' bank details: `view` as stored, `draft` while being changed (undefined = unchanged).
  const [bank, setBank] = useState({ view: null, draft: undefined });

  useEffect(() => {
    api.getParty(kind, id).then(
      (p) => setForm({ ...p, termsDays: p.termsDays ?? '', originalTaxId: p.taxId }),
      (err) => setError(errorText(err)),
    );
    api.getCustomValues(kind.toUpperCase(), id).then(setFields, () => setFields([]));
    if (kind === 'vendor') {
      api.getVendorBank(id).then((view) => setBank({ view, draft: undefined }), (err) => setError(errorText(err)));
    }
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
        // Unchanged (or not ours to change): the stored value stays as it is.
        keepTaxId: Boolean(form.taxIdLocked) || (form.taxId ?? '') === (form.originalTaxId ?? ''),
        is1099: Boolean(form.is1099),
        taxRateId: form.taxRateId ?? null,
        isTaxExempt: Boolean(form.isTaxExempt),
        taxExemption: form.taxExemption,
        formStyleId: form.formStyleId ?? null,
        salesRepId: form.salesRepId ?? null,
        customFields: fields.map(({ fieldId, value }) => ({ fieldId, value })),
      });
      if (kind === 'vendor' && bank.draft !== undefined) {
        const view = await api.setVendorBank(id, bank.draft);
        setBank({ view, draft: undefined });
      }
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
            {kind === 'vendor' && <TaxIdField form={form} setForm={setForm} />}
            {kind === 'vendor' && <BankDetailsFields bank={bank} setBank={setBank} />}
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
            {kind === 'customer' && (formStyles ?? []).length > 1 && (
              <label className="flex flex-col gap-0.5">
                <span className="font-semibold text-qb-ink-muted">Form style</span>
                <select className="qb-input" value={form.formStyleId ?? ''} onChange={(e) => setForm((f) => ({ ...f, formStyleId: e.target.value === '' ? null : Number(e.target.value) }))}>
                  <option value="">Default style (follows the default)</option>
                  {formStyles.map((st) => (
                    <option key={st.id} value={st.id}>
                      {st.name}
                      {st.isDefault ? ' (the default now)' : ''}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {kind === 'customer' && (reps.length > 0 || form.salesRepId) && (
              <label className="flex flex-col gap-0.5">
                <span className="font-semibold text-qb-ink-muted">Sales rep</span>
                <select className="qb-input" value={form.salesRepId ?? ''} onChange={(e) => setForm((f) => ({ ...f, salesRepId: e.target.value === '' ? null : Number(e.target.value) }))}>
                  <option value="">None</option>
                  {reps
                    .filter((r) => r.isActive || r.id === form.salesRepId)
                    .map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.initials}
                        {r.name ? ` (${r.name})` : ''}
                        {r.isActive ? '' : ' (inactive)'}
                      </option>
                    ))}
                </select>
              </label>
            )}
            <CustomFieldInputs fields={fields} onChange={setFields} />
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

  const nameRef = useRef(null);
  // Jobs are customers named "Customer:Job"; ordering by each part keeps a job under its customer.
  const byPath = (a, b) => {
    const pa = a.name.split(':');
    const pb = b.name.split(':');
    for (let i = 0; i < Math.min(pa.length, pb.length); i += 1) {
      const c = pa[i].localeCompare(pb[i], undefined, { sensitivity: 'base' });
      if (c) return c;
    }
    return pa.length - pb.length;
  };
  const rows = (data ?? [])
    .filter((p) => (showInactive || p.isActive) && p.name.toLowerCase().includes(query.trim().toLowerCase()))
    .sort(kind === 'customer' ? byPath : () => 0);
  const addJob = (p) => {
    setName(`${p.name}:`);
    nameRef.current?.focus();
  };
  const inactiveCount = (data ?? []).filter((p) => !p.isActive).length;
  const sorter = useSort(rows, {
    name: (p) => p.name,
    email: (p) => p.email,
    balance: (p) => p.openBalanceCents || null,
  });
  const total = rows.reduce((s, p) => s + p.openBalanceCents, 0);
  const company = useCompany();
  const exports = tableExports({
    company: company.name,
    title: `${cfg.title} List`,
    subtitle: `As of ${longDate(todayIso())}${query.trim() ? ` · matching "${query.trim()}"` : ''}`,
    columns: [
      { label: 'Name', kind: 'text' },
      { label: 'Email', kind: 'text' },
      { label: cfg.balance, kind: 'money' },
    ],
    rows: () => [
      ...rows.map((p) => ({ depth: p.name.split(':').length - 1, cells: [`${p.name}${p.isActive ? '' : ' (inactive)'}`, p.email ?? '', p.openBalanceCents || null] })),
      { kind: 'total', cells: ['TOTAL', '', total] },
    ],
    filename: `${cfg.title}-List`,
    onNotice: setNotice,
  });

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
          <ExportMenu printable={false} onNotice={setNotice} {...exports} />
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
                  <SortTh sorter={sorter} col="name">Name</SortTh>
                  <SortTh sorter={sorter} col="email">Email</SortTh>
                  <SortTh sorter={sorter} col="balance" className="text-right">{cfg.balance}</SortTh>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  <td className="qb-grid-cell-edit">
                    <input ref={nameRef} value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === 'customer' ? 'New customer, or Customer:Job' : `New ${cfg.noun} name`} aria-label="Name" />
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
                {sorter.sorted.map((p) => (
                  <tr key={p.id} className={`qb-grid-row ${p.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing(p.id)}>
                    <td className={`qb-grid-cell ${p.name.includes(':') ? '' : 'font-semibold'}`} style={{ paddingLeft: 6 + (p.name.split(':').length - 1) * 16 }} title={p.name}>
                      {p.name.split(':').at(-1)}
                      {p.name.includes(':') && <span className="ml-1 text-2xs uppercase tracking-wide text-qb-ink-muted">Job</span>}
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
                          ...(kind === 'customer' ? [{ label: 'Add job…', onSelect: () => addJob(p) }] : []),
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
