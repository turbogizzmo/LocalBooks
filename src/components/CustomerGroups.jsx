// Customer Center › Groups (customer_groups.rs): groups picked by hand, or kept up to date by
// rules on the customers' details, each with an optional statement reminder.
import { useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { BatchInvoiceDialog, PaymentRemindersDialog } from './BatchInvoices.jsx';

/** The rule kinds, as the editor offers them. */
export const RULE_FIELDS = [
  ['balance', 'Open balance'],
  ['customerType', 'Customer type'],
  ['terms', 'Terms'],
  ['rep', 'Sales rep'],
  ['state', 'State'],
  ['zip', 'ZIP code'],
  ['taxCode', 'Tax code'],
  ['creditLimit', 'Credit limit'],
  ['status', 'Status'],
];

/** A new rule of a kind, with a starting value. */
function newRule(field) {
  switch (field) {
    case 'balance':
      return { field, cents: 0, atMost: false };
    case 'state':
      return { field, code: '' };
    case 'zip':
      return { field, prefix: '' };
    case 'taxCode':
      return { field, id: null };
    case 'creditLimit':
      return { field, over: true };
    case 'status':
      return { field, active: true };
    default:
      return { field, id: null };
  }
}

/** "Open balance at least $500.00", for the list. */
export function describeRule(rule, names) {
  const name = (list, id) => names[list]?.find((x) => x.id === id)?.name ?? '?';
  switch (rule.field) {
    case 'balance':
      return `Open balance ${rule.atMost ? 'at most' : 'at least'} ${formatCents(rule.cents)}`;
    case 'customerType':
      return `Type is ${name('types', rule.id)}`;
    case 'terms':
      return `Terms are ${name('terms', rule.id)}`;
    case 'rep':
      return `Rep is ${name('reps', rule.id)}`;
    case 'state':
      return `State is ${rule.code.toUpperCase()}`;
    case 'zip':
      return `ZIP starts with ${rule.prefix}`;
    case 'taxCode':
      return rule.id == null ? 'Tax exempt' : `Tax code is ${name('taxes', rule.id)}`;
    case 'creditLimit':
      return rule.over ? 'Over their credit limit' : 'Has a credit limit';
    case 'status':
      return rule.active ? 'Active' : 'Inactive';
    default:
      return rule.field;
  }
}

function useNames() {
  const types = (useLedgerQuery(api.listProfileEntries).data ?? []).filter((e) => e.list === 'CUSTOMER_TYPE');
  const terms = useLedgerQuery(api.listTerms).data ?? [];
  const reps = useLedgerQuery(api.listSalesReps).data ?? [];
  const taxes = useLedgerQuery(api.listTaxRates).data ?? [];
  return { types, terms, reps, taxes };
}

function RuleRow({ rule, names, onChange, onRemove, index }) {
  const pick = (list) => (
    <select className="qb-input min-w-0 flex-1" value={rule.id ?? ''} onChange={(e) => onChange({ ...rule, id: e.target.value === '' ? null : Number(e.target.value) })} aria-label={`Rule ${index + 1} value`}>
      <option value="">Choose…</option>
      {names[list].map((x) => (
        <option key={x.id} value={x.id}>
          {x.name}
        </option>
      ))}
    </select>
  );
  let value;
  switch (rule.field) {
    case 'balance':
      value = (
        <>
          <select className="qb-input w-[90px]" value={rule.atMost ? 'most' : 'least'} onChange={(e) => onChange({ ...rule, atMost: e.target.value === 'most' })} aria-label={`Rule ${index + 1} comparison`}>
            <option value="least">at least</option>
            <option value="most">at most</option>
          </select>
          <input
            className="qb-input num w-[110px]"
            data-money
            inputMode="decimal"
            defaultValue={centsToInput(rule.cents)}
            onBlur={(e) => {
              const cents = parseMoneyToCents(e.target.value);
              if (cents !== null) onChange({ ...rule, cents });
            }}
            aria-label={`Rule ${index + 1} amount`}
          />
        </>
      );
      break;
    case 'customerType':
      value = pick('types');
      break;
    case 'terms':
      value = pick('terms');
      break;
    case 'rep':
      value = pick('reps');
      break;
    case 'state':
      value = <input className="qb-input w-[60px] uppercase" maxLength={2} value={rule.code} onChange={(e) => onChange({ ...rule, code: e.target.value })} placeholder="OR" aria-label={`Rule ${index + 1} state`} />;
      break;
    case 'zip':
      value = <input className="qb-input w-[100px]" maxLength={10} value={rule.prefix} onChange={(e) => onChange({ ...rule, prefix: e.target.value })} placeholder="972" aria-label={`Rule ${index + 1} ZIP code`} />;
      break;
    case 'taxCode':
      value = (
        <select className="qb-input min-w-0 flex-1" value={rule.id ?? 'exempt'} onChange={(e) => onChange({ ...rule, id: e.target.value === 'exempt' ? null : Number(e.target.value) })} aria-label={`Rule ${index + 1} tax code`}>
          <option value="exempt">Tax exempt</option>
          {names.taxes.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
      );
      break;
    case 'creditLimit':
      value = (
        <select className="qb-input" value={rule.over ? 'over' : 'has'} onChange={(e) => onChange({ ...rule, over: e.target.value === 'over' })} aria-label={`Rule ${index + 1} credit limit`}>
          <option value="over">over their limit</option>
          <option value="has">has a limit</option>
        </select>
      );
      break;
    default:
      value = (
        <select className="qb-input" value={rule.active ? 'active' : 'inactive'} onChange={(e) => onChange({ ...rule, active: e.target.value === 'active' })} aria-label={`Rule ${index + 1} status`}>
          <option value="active">active</option>
          <option value="inactive">inactive</option>
        </select>
      );
  }
  return (
    <div className="flex items-center gap-1">
      <select className="qb-input w-[130px]" value={rule.field} onChange={(e) => onChange(newRule(e.target.value))} aria-label={`Rule ${index + 1}`}>
        {RULE_FIELDS.map(([id, label]) => (
          <option key={id} value={id}>
            {label}
          </option>
        ))}
      </select>
      {value}
      <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-negative" onClick={onRemove} aria-label={`Remove rule ${index + 1}`}>
        ×
      </button>
    </div>
  );
}

/** Adds or changes a group. */
function GroupDialog({ group, customers, names, onClose, onSaved }) {
  const [name, setName] = useState(group?.name ?? '');
  const [automatic, setAutomatic] = useState(group ? group.automatic : false);
  const [rules, setRules] = useState(group?.rules?.length ? group.rules : [newRule('state')]);
  const [members, setMembers] = useState(() => new Set(group && !group.automatic ? group.memberIds : []));
  const [filter, setFilter] = useState('');
  const [statement, setStatement] = useState(group?.statement ?? { everyDays: null, lastSent: null, openOnly: false, styleId: null, message: null });
  const [payments, setPayments] = useState(group?.payments ?? { everyDays: null, minDaysOverdue: 1, lastSent: null });
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const shown = customers.filter((c) => c.name.toLowerCase().includes(filter.trim().toLowerCase()));
  const toggle = (id) =>
    setMembers((m) => {
      const next = new Set(m);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const save = async (e) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const input = { name, rules: automatic ? rules : [], memberIds: automatic ? [] : [...members], statement, payments };
      const saved = await api.saveCustomerGroup(input, group?.id ?? null);
      onSaved(`Group “${saved.name}” saved: ${saved.memberCount} customer${saved.memberCount === 1 ? '' : 's'}.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={group ? `Edit group · ${group.name}` : 'New customer group'} onClose={onClose}>
      <form onSubmit={save} className="flex min-h-0 flex-col gap-2 overflow-auto p-3">
        <label>
          <span className="qb-label">Name</span>
          <input className="qb-input w-full" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />
        </label>
        <div className="flex gap-3 text-sm" role="radiogroup" aria-label="Members">
          <label className="flex items-center gap-1">
            <input type="radio" checked={!automatic} onChange={() => setAutomatic(false)} />
            Pick the customers
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" checked={automatic} onChange={() => setAutomatic(true)} />
            Automatic: customers who meet every rule
          </label>
        </div>
        {automatic ? (
          <div className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
            {rules.map((rule, i) => (
              <RuleRow key={i} index={i} rule={rule} names={names} onChange={(r) => setRules((rs) => rs.map((x, j) => (j === i ? r : x)))} onRemove={() => setRules((rs) => rs.filter((_, j) => j !== i))} />
            ))}
            <button type="button" className="self-start text-sm text-qb-link hover:underline" onClick={() => setRules((rs) => [...rs, newRule('balance')])}>
              Add a rule
            </button>
            <p className="text-2xs text-qb-ink-faint">Members change on their own as customers’ details and balances change.</p>
          </div>
        ) : (
          <div className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
            <input className="qb-input w-full" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Find a customer" aria-label="Find a customer" />
            <div className="max-h-[180px] overflow-auto scrollbar-thin">
              {shown.map((c) => (
                <label key={c.id} className="flex items-center gap-1.5 py-0.5 text-sm">
                  <input type="checkbox" checked={members.has(c.id)} onChange={() => toggle(c.id)} />
                  {c.name}
                  {!c.isActive && <span className="text-2xs uppercase text-qb-ink-faint">Inactive</span>}
                </label>
              ))}
            </div>
            <span className="text-2xs text-qb-ink-muted">{members.size} picked</span>
          </div>
        )}
        <fieldset className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
          <legend className="px-1 text-xs font-semibold text-qb-ink-muted">Statement reminder</legend>
          <label className="flex items-center gap-1.5 text-sm">
            <input type="checkbox" checked={statement.everyDays != null} onChange={(e) => setStatement((s) => ({ ...s, everyDays: e.target.checked ? 30 : null }))} />
            Remind me to send this group’s statements every
            <input
              className="qb-input num w-[56px]"
              inputMode="numeric"
              disabled={statement.everyDays == null}
              value={statement.everyDays ?? ''}
              onChange={(e) => setStatement((s) => ({ ...s, everyDays: Number.parseInt(e.target.value, 10) || 1 }))}
              aria-label="Days between statements"
            />
            days
          </label>
          {statement.everyDays != null && (
            <>
              <span className="text-2xs text-qb-ink-muted">The group comes up in Reminders when its statements are due; send them from Customer Statements and click Mark sent.</span>
              {statement.lastSent && <span className="text-2xs text-qb-ink-muted">Last sent {formatDate(statement.lastSent)}.</span>}
            </>
          )}
        </fieldset>
        <fieldset className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
          <legend className="px-1 text-xs font-semibold text-qb-ink-muted">Payment reminders</legend>
          <label className="flex flex-wrap items-center gap-1.5 text-sm">
            <input type="checkbox" checked={payments.everyDays != null} onChange={(e) => setPayments((p) => ({ ...p, everyDays: e.target.checked ? 14 : null }))} />
            Every
            <input
              className="qb-input num w-[56px]"
              inputMode="numeric"
              disabled={payments.everyDays == null}
              value={payments.everyDays ?? ''}
              onChange={(e) => setPayments((p) => ({ ...p, everyDays: Number.parseInt(e.target.value, 10) || 1 }))}
              aria-label="Days between payment reminders"
            />
            days, remind about invoices at least
            <input
              className="qb-input num w-[56px]"
              inputMode="numeric"
              disabled={payments.everyDays == null}
              value={payments.minDaysOverdue}
              onChange={(e) => setPayments((p) => ({ ...p, minDaysOverdue: Math.max(0, Number.parseInt(e.target.value, 10) || 0) }))}
              aria-label="Days overdue before a reminder"
            />
            days overdue
          </label>
          {payments.lastSent && <span className="text-2xs text-qb-ink-muted">Last sent {formatDate(payments.lastSent)}.</span>}
        </fieldset>
        {error && <Notice kind="error" text={error} />}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || !name.trim() || (automatic && rules.length === 0)}>
            Save group
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/** The Groups view: each group with its rules or members and its statement reminder. */
export default function CustomerGroups({ navigate, remindGroupId = null }) {
  const groups = useLedgerQuery(api.listCustomerGroups);
  const [batching, setBatching] = useState(null); // { groupId }
  const [reminding, setReminding] = useState(null); // group id
  const [askedReminders, setAskedReminders] = useState(null);
  if (remindGroupId && askedReminders !== remindGroupId) {
    setAskedReminders(remindGroupId);
    setReminding(remindGroupId);
  }
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const names = useNames();
  const [editing, setEditing] = useState(null); // { group } (null group = new)
  const [notice, setNotice] = useState(null);
  const [open, setOpen] = useState(null);
  const byId = useMemo(() => new Map(customers.map((c) => [c.id, c])), [customers]);

  const remove = async (g) => {
    if (!(await askConfirm(`Delete the group “${g.name}”? Its customers stay as they are.`, { confirmLabel: 'Delete', danger: true }))) return;
    try {
      await api.deleteCustomerGroup(g.id);
      setNotice({ kind: 'success', text: `Group “${g.name}” deleted.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const list = groups.data ?? [];
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {batching && (
        <BatchInvoiceDialog
          initialGroupId={batching.groupId}
          onClose={() => setBatching(null)}
          onDone={(text) => {
            setBatching(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {reminding && list.some((g) => g.id === reminding) && (
        <PaymentRemindersDialog
          group={list.find((g) => g.id === reminding)}
          onClose={() => setReminding(null)}
          onDone={(text) => {
            setReminding(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {editing && (
        <GroupDialog
          group={editing.group}
          customers={customers}
          names={names}
          onClose={() => setEditing(null)}
          onSaved={(text) => {
            setEditing(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      <div className="flex flex-none items-center gap-2 px-2 py-1">
        <span className="text-sm text-qb-ink-muted">Groups of customers, picked by hand or kept up to date by rules. Pick a group in Customer Statements to send its statements.</span>
        <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setEditing({ group: null })}>
          New group
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {groups.error && <Notice kind="error" text={groups.error} />}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Group</th>
              <th className="qb-grid-head">Who is in it</th>
              <th className="qb-grid-head w-[90px] text-right">Customers</th>
              <th className="qb-grid-head w-[240px]">Reminders</th>
              <th className="qb-grid-head w-[30px]" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {groups.data && list.length === 0 && (
              <tr>
                <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No groups yet. Click New group to make one.</td>
              </tr>
            )}
            {list.map((g) => [
              <tr key={g.id} className={`qb-grid-row cursor-pointer ${open === g.id ? 'is-active' : ''}`} onClick={() => setOpen(open === g.id ? null : g.id)} onDoubleClick={() => setEditing({ group: g })}>
                <td className="qb-grid-cell font-semibold">
                  {g.name}
                  {g.automatic && <span className="ml-1 text-2xs font-normal uppercase tracking-wide text-qb-info">Automatic</span>}
                </td>
                <td className="qb-grid-cell text-qb-ink-muted" title={g.automatic ? g.rules.map((r) => describeRule(r, names)).join(' and ') : undefined}>
                  {g.automatic ? g.rules.map((r) => describeRule(r, names)).join(' and ') : 'Picked by hand'}
                </td>
                <td className="qb-grid-cell num">{g.memberCount}</td>
                <td className="qb-grid-cell text-sm text-qb-ink-muted">
                  {g.statement.everyDays ? `Statements every ${g.statement.everyDays} days` : ''}
                  {g.statement.everyDays && g.payments.everyDays ? ' · ' : ''}
                  {g.payments.everyDays ? `Payments every ${g.payments.everyDays} days` : ''}
                  {!g.statement.everyDays && !g.payments.everyDays && '—'}
                </td>
                <td className="qb-grid-cell cell-tight text-center" onClick={(e) => e.stopPropagation()}>
                  <RowMenu
                    label={`Actions for ${g.name}`}
                    items={[
                      { label: 'Edit…', onSelect: () => setEditing({ group: g }) },
                      { label: 'Batch invoice this group…', onSelect: () => setBatching({ groupId: g.id }) },
                      { label: 'Payment reminders…', onSelect: () => setReminding(g.id) },
                      navigate && { label: 'Statements for this group…', onSelect: () => navigate('sales/statements', 'group', { groupId: g.id }) },
                      { label: 'Delete…', onSelect: () => remove(g) },
                    ].filter(Boolean)}
                  />
                </td>
              </tr>,
              open === g.id && (
                <tr key={`${g.id}-members`}>
                  <td colSpan={5} className="bg-qb-row-header px-6 py-1 text-sm">
                    {g.memberIds.length === 0 ? <span className="text-qb-ink-muted">No customers right now.</span> : g.memberIds.map((id) => byId.get(id)?.name ?? `#${id}`).join(' · ')}
                  </td>
                </tr>
              ),
            ])}
          </tbody>
        </table>
      </div>
    </div>
  );
}
