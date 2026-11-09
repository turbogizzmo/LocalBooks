// Customers › Create Batch Invoices (batch_invoices.rs) and a group's payment reminders
// (payment_reminders.rs).
import { useState } from 'react';
import Notice from './Notice.jsx';
import DateBox from './DateBox.jsx';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/** The same lines invoiced to every customer of a group, or to customers picked by hand. */
export function BatchInvoiceDialog({ initialGroupId = null, onClose, onDone }) {
  const groups = useLedgerQuery(api.listCustomerGroups).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const accounts = useLedgerQuery(api.listAccounts).data ?? [];
  const products = useLedgerQuery(api.listProducts).data ?? [];
  const defaultIncome = accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null;
  const [groupId, setGroupId] = useState(initialGroupId);
  const [picked, setPicked] = useState(() => new Set());
  const [filter, setFilter] = useState('');
  const [date, setDate] = useState(todayIso());
  const [dueDays, setDueDays] = useState('30');
  const [lines, setLines] = useState(() => [blankLine(null)]);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const group = groups.find((g) => g.id === groupId) ?? null;
  const fromGroup = new Set(group?.memberIds ?? []);
  const chosen = customers.filter((c) => (fromGroup.has(c.id) || picked.has(c.id)) && c.isActive);
  const { payload, invalidCount, total, filled } = summarizeLines(lines);
  const ready = chosen.length > 0 && filled.length > 0 && invalidCount === 0 && Boolean(date);

  const toggle = (id) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.createBatchInvoices({
        groupId,
        customerIds: [...picked],
        issueDate: date,
        dueDays: Number.parseInt(dueDays, 10) || 0,
        lines: payload.map(({ amountCents, ...line }) => line),
      });
      const parts = [`Created ${result.invoices.length} invoice${result.invoices.length === 1 ? '' : 's'}`];
      if (result.emailedLater) parts.push(`${result.emailedLater} waiting in Send Forms`);
      if (result.printedLater) parts.push(`${result.printedLater} waiting in Print Forms`);
      if (result.skipped.length) parts.push(`skipped ${result.skipped.map(([n, why]) => `${n} (${why})`).join(', ')}`);
      onDone(`${parts.join('; ')}.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title="Create batch invoices" onClose={onClose} wide>
      <div className="flex min-h-0 flex-col gap-2 overflow-auto p-3">
        <p className="text-sm text-qb-ink-muted">One invoice for each customer, with the same lines. Each is due on the customer’s own terms (or after the days below) and waits in Send Forms or Print Forms as the customer prefers.</p>
        <div className="grid grid-cols-[260px_minmax(0,1fr)] gap-3">
          <div className="flex flex-col gap-1">
            <label>
              <span className="qb-label">Customer group</span>
              <select className="qb-input w-full" value={groupId ?? ''} onChange={(e) => setGroupId(e.target.value === '' ? null : Number(e.target.value))}>
                <option value="">No group</option>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name} ({g.memberCount})
                  </option>
                ))}
              </select>
            </label>
            <input className="qb-input w-full" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Add customers…" aria-label="Find a customer" />
            <div className="max-h-[220px] overflow-auto rounded-xs border border-qb-border px-1 scrollbar-thin">
              {customers
                .filter((c) => c.isActive && c.name.toLowerCase().includes(filter.trim().toLowerCase()))
                .map((c) => (
                  <label key={c.id} className="flex items-center gap-1.5 py-0.5 text-sm" title={fromGroup.has(c.id) ? 'In the group' : undefined}>
                    <input type="checkbox" checked={fromGroup.has(c.id) || picked.has(c.id)} disabled={fromGroup.has(c.id)} onChange={() => toggle(c.id)} />
                    {c.name}
                  </label>
                ))}
            </div>
            <span className="text-xs text-qb-ink-muted">{chosen.length} customer{chosen.length === 1 ? '' : 's'}</span>
          </div>
          <div className="flex min-w-0 flex-col gap-2">
            <div className="flex items-end gap-2">
              <label>
                <span className="qb-label">Invoice date</span>
                <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
              </label>
              <label title="For customers without terms of their own">
                <span className="qb-label">Due after (days)</span>
                <input className="qb-input num w-[80px]" inputMode="numeric" value={dueDays} onChange={(e) => setDueDays(e.target.value)} />
              </label>
              <span className="ml-auto text-sm">
                Each invoice <span className="num font-semibold">{formatCents(total)}</span> · all <span className="num font-semibold">{formatCents(total * chosen.length)}</span>
              </span>
            </div>
            <div className="overflow-x-auto scrollbar-thin">
              <div className="min-w-[960px]">
                <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products} defaultAccountId={defaultIncome} />
              </div>
            </div>
            <button type="button" className="qb-btn qb-btn-secondary self-start" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
              Add line
            </button>
          </div>
        </div>
        {error && <Notice kind="error" text={error} />}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={!ready || busy} onClick={create}>
            Create {chosen.length || ''} invoice{chosen.length === 1 ? '' : 's'}
          </button>
        </div>
      </div>
    </Dialog>
  );
}

/** A group's overdue invoices to remind about: tick them and queue them for Email later. */
export function PaymentRemindersDialog({ group, onClose, onDone }) {
  const overdue = useLedgerQuery(() => api.groupOverdueInvoices(group.id));
  const [skip, setSkip] = useState(() => new Set());
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const list = overdue.data ?? [];
  const chosen = list.filter((o) => !skip.has(o.invoiceId));
  const total = chosen.reduce((sum, o) => sum + o.balanceCents, 0);

  const send = async () => {
    setBusy(true);
    setError(null);
    try {
      const n = await api.sendPaymentReminders(group.id, chosen.map((o) => o.invoiceId));
      onDone(`${n} overdue invoice${n === 1 ? '' : 's'} for ${group.name} queued in Send Forms. The next reminder is in ${group.payments.everyDays ?? '—'} days.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={`Payment reminders · ${group.name}`} onClose={onClose}>
      <div className="flex min-h-0 flex-col gap-2 p-3">
        <p className="text-sm text-qb-ink-muted">
          Invoices at least {group.payments.minDaysOverdue} day{group.payments.minDaysOverdue === 1 ? '' : 's'} overdue. Sending queues each ticked invoice in Sales › Send Forms, which emails it with its invoice message.
        </p>
        {overdue.error && <Notice kind="error" text={overdue.error} />}
        <div className="max-h-[320px] overflow-auto rounded-xs border border-qb-border scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[28px]" aria-label="Send" />
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head w-[80px]">Invoice</th>
                <th className="qb-grid-head w-[92px]">Due</th>
                <th className="qb-grid-head w-[70px] text-right">Days late</th>
                <th className="qb-grid-head w-[100px] text-right">Open</th>
              </tr>
            </thead>
            <tbody>
              {overdue.data && list.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">Nothing overdue in this group.</td>
                </tr>
              )}
              {list.map((o) => (
                <tr key={o.invoiceId} className="qb-grid-row">
                  <td className="qb-grid-cell text-center">
                    <input
                      type="checkbox"
                      checked={!skip.has(o.invoiceId)}
                      onChange={() =>
                        setSkip((s) => {
                          const next = new Set(s);
                          if (next.has(o.invoiceId)) next.delete(o.invoiceId);
                          else next.add(o.invoiceId);
                          return next;
                        })
                      }
                      aria-label={`Remind about invoice ${o.invoiceNumber}`}
                    />
                  </td>
                  <td className="qb-grid-cell" title={o.customerEmail ?? 'No email address'}>
                    {o.customerName}
                    {!o.customerEmail && <span className="ml-1 text-2xs text-qb-negative">no email</span>}
                    {o.queued && <span className="ml-1 text-2xs text-qb-ink-muted">already queued</span>}
                  </td>
                  <td className="qb-grid-cell">#{o.invoiceNumber}</td>
                  <td className="qb-grid-cell">{formatDate(o.dueDate)}</td>
                  <td className="qb-grid-cell num">{o.daysOverdue}</td>
                  <td className="qb-grid-cell num">{formatCents(o.balanceCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {error && <Notice kind="error" text={error} />}
        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-sm">
            {chosen.length} to send · <span className="num font-semibold">{formatCents(total)}</span>
          </span>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Close
          </button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || chosen.length === 0} onClick={send}>
            Send reminders
          </button>
        </div>
      </div>
    </Dialog>
  );
}
