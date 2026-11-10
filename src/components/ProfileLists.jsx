import { useEffect, useState } from 'react';
import RowMenu from './RowMenu.jsx';
import { InactiveToggle, Panel, useRunner } from './SalesLists.jsx';
import { formatPpm, kindLabel, PAYMENT_KINDS, termsInputFrom, termsSummary } from './ProfileFields.jsx';
import { api, useLedgerQuery } from '../lib/api.js';

/** The lists, in the order of the Lists › Customer & Vendor Profile Lists menu. */
export const PROFILE_LISTS = [
  { key: 'terms', label: 'Terms' },
  { key: 'payment-methods', label: 'Payment Method' },
  { key: 'customer-types', label: 'Customer Type', list: 'CUSTOMER_TYPE', noun: 'customer type', max: 31, placeholder: 'e.g. Retail', hint: 'Group customers by kind, such as Retail or Wholesale; chosen on each customer.' },
  { key: 'vendor-types', label: 'Vendor Type', list: 'VENDOR_TYPE', noun: 'vendor type', max: 31, placeholder: 'e.g. Supplier', hint: 'Group vendors by kind, such as Supplier or Contractor; chosen on each vendor.' },
  { key: 'job-types', label: 'Job Type', list: 'JOB_TYPE', noun: 'job type', max: 31, placeholder: 'e.g. Remodel', hint: 'Group jobs by kind of work; chosen on each job (a customer under another customer).' },
  { key: 'customer-messages', label: 'Customer Message', list: 'CUSTOMER_MESSAGE', noun: 'customer message', max: 101, placeholder: 'e.g. Thank you for your business.', hint: 'A line printed at the foot of an invoice, sales receipt or estimate; picked on each form.' },
];

function actionsFor(entry, run, { onEdit, setActive, remove, usedHint }) {
  return [
    { label: 'Edit', onSelect: onEdit },
    entry.isActive
      ? { label: 'Make inactive', onSelect: () => run(() => setActive(false), () => `${entry.name} is inactive; what already has it keeps it.`) }
      : { label: 'Make active', onSelect: () => run(() => setActive(true), () => `${entry.name} is active again.`) },
    {
      label: 'Delete',
      danger: true,
      disabled: entry.isUsed,
      hint: entry.isUsed ? `${usedHint}: make it inactive instead` : undefined,
      onSelect: () => run(remove, () => `Deleted ${entry.name}.`),
      confirm: { title: `Delete ${entry.name}?`, body: 'Nothing uses it yet.', confirmLabel: 'Delete' },
    },
  ];
}

const blankTerms = { name: '', netDays: '30', percent: '', discountDays: '' };

const termsInput = (t) => termsInputFrom(t);

function TermsRow({ value, onChange, formId, busy, label }) {
  const input = (key, props) => (
    <input form={formId} value={value[key]} onChange={(e) => onChange({ ...value, [key]: e.target.value })} {...props} />
  );
  return (
    <>
      <td className="qb-grid-cell-edit">{input('name', { maxLength: 31, placeholder: 'e.g. 2% 10 Net 30', 'aria-label': 'Term name', autoFocus: label === 'Save' })}</td>
      <td className="qb-grid-cell-edit">{input('netDays', { className: 'num', inputMode: 'numeric', 'aria-label': 'Net days' })}</td>
      <td className="qb-grid-cell-edit">{input('percent', { className: 'num', inputMode: 'decimal', placeholder: '—', 'aria-label': 'Discount percent' })}</td>
      <td className="qb-grid-cell-edit">{input('discountDays', { className: 'num', inputMode: 'numeric', placeholder: '—', 'aria-label': 'Discount days' })}</td>
      <td className="qb-grid-cell cell-tight">
        <button type="submit" form={formId} className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>{label}</button>
      </td>
    </>
  );
}

function TermsList() {
  const { data, error } = useLedgerQuery(api.listTerms);
  const terms = data ?? [];
  const { busy, notice, setNotice, run } = useRunner();
  const [draft, setDraft] = useState(blankTerms);
  const [editing, setEditing] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const shown = terms.filter((t) => showInactive || t.isActive);

  const add = async (e) => {
    e.preventDefault();
    if (!draft.name.trim()) return setNotice({ kind: 'error', text: 'Enter a name for the term.' });
    if (await run(() => api.createTerms(termsInput(draft)), (t) => `Added ${t.name}.`)) setDraft(blankTerms);
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.updateTerms(editing.id, termsInput(editing)), (t) => `Saved ${t.name}. Invoices and bills already entered keep their due dates.`)) setEditing(null);
  };
  const edit = (t) => setEditing({ id: t.id, name: t.name, netDays: String(t.netDays), percent: t.discountPpm ? formatPpm(t.discountPpm) : '', discountDays: t.discountDays ?? '' });

  return (
    <Panel
      title="Terms"
      hint="When invoices and bills are due, and any early-payment discount. Customers and vendors have a default term; the form's Terms box sets the due date."
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load terms: ${error}`}
      toolbar={<InactiveToggle value={showInactive} onChange={setShowInactive} count={terms.filter((t) => !t.isActive).length} />}
    >
      <table className="qb-grid">
        <colgroup>
          <col />
          <col className="w-[84px]" />
          <col className="w-[96px]" />
          <col className="w-[110px]" />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Term</th>
            <th className="qb-grid-head text-right">Net days</th>
            <th className="qb-grid-head text-right">Discount %</th>
            <th className="qb-grid-head text-right">Discount days</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          <tr className="bg-qb-row-selected">
            <TermsRow value={draft} onChange={setDraft} formId="new-terms" busy={busy} label="Add" />
          </tr>
          {shown.map((t) =>
            editing?.id === t.id ? (
              <tr key={t.id} className="qb-grid-row is-active" onKeyDown={(e) => e.key === 'Escape' && setEditing(null)}>
                <TermsRow value={editing} onChange={setEditing} formId="edit-terms" busy={busy} label="Save" />
              </tr>
            ) : (
              <tr key={t.id} className={`qb-grid-row ${t.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => edit(t)} title={termsSummary(t)}>
                <td className="qb-grid-cell">
                  {t.name}
                  {!t.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                </td>
                <td className="qb-grid-cell num">{t.netDays}</td>
                <td className="qb-grid-cell num">{t.discountPpm ? `${formatPpm(t.discountPpm)}%` : ''}</td>
                <td className="qb-grid-cell num">{t.discountDays ?? ''}</td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${t.name}`}
                    items={actionsFor(t, run, {
                      onEdit: () => edit(t),
                      setActive: (active) => api.setTermsActive(t.id, active),
                      remove: () => api.deleteTerms(t.id),
                      usedHint: 'On customers, vendors, invoices or bills',
                    })}
                  />
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
      <form id="new-terms" onSubmit={add} />
      <form id="edit-terms" onSubmit={save} />
    </Panel>
  );
}

function PaymentMethodList() {
  const { data, error } = useLedgerQuery(api.listPaymentMethods);
  const methods = data ?? [];
  const { busy, notice, setNotice, run } = useRunner();
  const [name, setName] = useState('');
  const [kind, setKind] = useState('CREDIT_CARD');
  const [editing, setEditing] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const shown = methods.filter((m) => showInactive || m.isActive);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a payment method.' });
    if (await run(() => api.createPaymentMethod({ name, kind }), (m) => `Added ${m.name}.`)) setName('');
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.updatePaymentMethod(editing.id, { name: editing.name, kind: editing.kind }), (m) => `Saved ${m.name}.`)) setEditing(null);
  };
  const kindSelect = (value, onChange, formId, disabled) => (
    <select form={formId} value={value} onChange={(e) => onChange(e.target.value)} aria-label="Kind of payment" disabled={disabled} title={disabled ? 'Already on payments, so its kind stays' : undefined}>
      {PAYMENT_KINDS.map((k) => (
        <option key={k.id} value={k.id}>{k.label}</option>
      ))}
    </select>
  );

  return (
    <Panel
      title="Payment Method"
      hint="The names in the payment method boxes on payments, sales receipts and expenses ('Visa', 'Check'); each is one kind of payment."
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load payment methods: ${error}`}
      toolbar={<InactiveToggle value={showInactive} onChange={setShowInactive} count={methods.filter((m) => !m.isActive).length} />}
    >
      <table className="qb-grid">
        <colgroup>
          <col />
          <col className="w-[170px]" />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Payment method</th>
            <th className="qb-grid-head">Kind</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          <tr className="bg-qb-row-selected">
            <td className="qb-grid-cell-edit">
              <input form="new-method" value={name} onChange={(e) => setName(e.target.value)} maxLength={31} placeholder="e.g. Visa" aria-label="Payment method" />
            </td>
            <td className="qb-grid-cell-edit">{kindSelect(kind, setKind, 'new-method', false)}</td>
            <td className="qb-grid-cell cell-tight">
              <button type="submit" form="new-method" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Add</button>
            </td>
          </tr>
          {shown.map((m) =>
            editing?.id === m.id ? (
              <tr key={m.id} className="qb-grid-row is-active" onKeyDown={(e) => e.key === 'Escape' && setEditing(null)}>
                <td className="qb-grid-cell-edit">
                  <input form="edit-method" value={editing.name} maxLength={31} onChange={(e) => setEditing({ ...editing, name: e.target.value })} aria-label="Payment method" autoFocus />
                </td>
                <td className="qb-grid-cell-edit">{kindSelect(editing.kind, (k) => setEditing({ ...editing, kind: k }), 'edit-method', m.isUsed)}</td>
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form="edit-method" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Save</button>
                </td>
              </tr>
            ) : (
              <tr key={m.id} className={`qb-grid-row ${m.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing({ id: m.id, name: m.name, kind: m.kind })}>
                <td className="qb-grid-cell">
                  {m.name}
                  {!m.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                </td>
                <td className="qb-grid-cell text-qb-ink-muted">{kindLabel(m.kind)}</td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${m.name}`}
                    items={actionsFor(m, run, {
                      onEdit: () => setEditing({ id: m.id, name: m.name, kind: m.kind }),
                      setActive: (active) => api.setPaymentMethodActive(m.id, active),
                      remove: () => api.deletePaymentMethod(m.id),
                      usedHint: 'On payments',
                    })}
                  />
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
      <form id="new-method" onSubmit={add} />
      <form id="edit-method" onSubmit={save} />
    </Panel>
  );
}

/** A customer, vendor or job type list, or the customer messages: names only. */
function EntryList({ spec }) {
  const { data, error } = useLedgerQuery(api.listProfileEntries);
  const entries = (data ?? []).filter((e) => e.list === spec.list);
  const { busy, notice, setNotice, run } = useRunner();
  const [name, setName] = useState('');
  const [editing, setEditing] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const shown = entries.filter((e) => showInactive || e.isActive);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: `Enter a ${spec.noun}.` });
    if (await run(() => api.createProfileEntry(spec.list, name), (x) => `Added ${x.name}.`)) setName('');
  };
  const save = async (e) => {
    e.preventDefault();
    if (await run(() => api.renameProfileEntry(editing.id, editing.name), (x) => `Saved ${x.name}.`)) setEditing(null);
  };

  return (
    <Panel
      title={spec.label}
      hint={spec.hint}
      notice={notice}
      onDismiss={() => setNotice(null)}
      error={error && `Could not load the list: ${error}`}
      toolbar={<InactiveToggle value={showInactive} onChange={setShowInactive} count={entries.filter((x) => !x.isActive).length} />}
    >
      <table className="qb-grid">
        <colgroup>
          <col />
          <col className="w-[72px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">{spec.label}</th>
            <th className="qb-grid-head" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          <tr className="bg-qb-row-selected">
            <td className="qb-grid-cell-edit">
              <form id={`new-${spec.key}`} onSubmit={add} />
              <input form={`new-${spec.key}`} value={name} onChange={(e) => setName(e.target.value)} maxLength={spec.max} placeholder={spec.placeholder} aria-label={spec.label} />
            </td>
            <td className="qb-grid-cell cell-tight">
              <button type="submit" form={`new-${spec.key}`} className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Add</button>
            </td>
          </tr>
          {data && shown.length === 0 && (
            <tr>
              <td colSpan={2} className="qb-grid-cell text-qb-ink-muted">Nothing here yet.</td>
            </tr>
          )}
          {shown.map((x) =>
            editing?.id === x.id ? (
              <tr key={x.id} className="qb-grid-row is-active">
                <td className="qb-grid-cell-edit">
                  <form id={`edit-${spec.key}`} onSubmit={save} />
                  <input form={`edit-${spec.key}`} value={editing.name} maxLength={spec.max} onChange={(e) => setEditing({ ...editing, name: e.target.value })} aria-label={spec.label} autoFocus
                    onKeyDown={(e) => e.key === 'Escape' && setEditing(null)} />
                </td>
                <td className="qb-grid-cell cell-tight">
                  <button type="submit" form={`edit-${spec.key}`} className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>Save</button>
                </td>
              </tr>
            ) : (
              <tr key={x.id} className={`qb-grid-row ${x.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing({ id: x.id, name: x.name })}>
                <td className="qb-grid-cell">
                  {x.name}
                  {!x.isActive && <span className="ml-1 text-2xs uppercase tracking-wide">Inactive</span>}
                </td>
                <td className="qb-grid-cell cell-tight text-center">
                  <RowMenu
                    label={`Actions for ${x.name}`}
                    items={actionsFor(x, run, {
                      onEdit: () => setEditing({ id: x.id, name: x.name }),
                      setActive: (active) => api.setProfileEntryActive(x.id, active),
                      remove: () => api.deleteProfileEntry(x.id),
                      usedHint: 'In use',
                    })}
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

/** Lists › Customer & Vendor Profile Lists: one list at a time, chosen by the tabs or the menu. */
export default function ProfileLists({ intent }) {
  const [tab, setTab] = useState(() => (PROFILE_LISTS.some((l) => l.key === intent?.action) ? intent.action : 'terms'));
  useEffect(() => {
    if (PROFILE_LISTS.some((l) => l.key === intent?.action)) setTab(intent.action);
  }, [intent]);
  const spec = PROFILE_LISTS.find((l) => l.key === tab);
  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-2">
      <div className="flex flex-none overflow-hidden self-start rounded-xs border border-qb-border" role="tablist" aria-label="Profile list">
        {PROFILE_LISTS.map((l) => (
          <button
            key={l.key}
            type="button"
            role="tab"
            aria-selected={tab === l.key}
            className={`px-2 py-0.5 text-xs ${tab === l.key ? 'bg-qb-green-active text-white' : 'bg-white hover:bg-qb-canvas'}`}
            onClick={() => setTab(l.key)}
          >
            {l.label}
          </button>
        ))}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-1 lg:max-w-[760px]">
        {tab === 'terms' && <TermsList />}
        {tab === 'payment-methods' && <PaymentMethodList />}
        {spec.list && <EntryList key={spec.key} spec={spec} />}
      </div>
    </div>
  );
}
