import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import DateBox from './DateBox.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatMoney, formatQuantity } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const key = (c) => `${c.source}:${c.lineId}`;

/** Thousandths of a percent → "15", "12.5". */
const percentText = (milli) => (milli / 1000).toLocaleString('en-US', { maximumFractionDigits: 3, useGrouping: false });

/**
 * A markup percent as the backend reads it: digits with up to 3 decimals (and an optional %), 0 to
 * 1,000%, in thousandths of a percent; blank means the default from Preferences. `null` when invalid.
 */
function parseMarkup(text, fallback) {
  const t = text.trim().replace(/%$/, '').trim();
  if (t === '') return fallback;
  const m = /^(\d{0,4})(?:\.(\d{0,3}))?$/.exec(t);
  if (!m || (m[1] === '' && !m[2])) return null;
  const milli = Number(m[1] || '0') * 1000 + Number((m[2] ?? '').padEnd(3, '0'));
  return milli <= 1_000_000 ? milli : null;
}

/** round_half_away(amount × milli / 100,000), as the backend works it out per cost. */
const markupOf = (cents, milli) => Math.round((cents * milli) / 100_000);

/**
 * Sales › Bill Costs: invoice billable expenses (checks, expenses and bill lines marked Bill? for a
 * customer) to that customer, with the markup from Preferences › Time & Expenses, and billable time
 * from the timesheet (one line per entry: its service item, hours and rate; no markup).
 */
export default function BillCosts({ navigate, intent }) {
  const costs = useLedgerQuery(() => api.listUnbilledCosts(null));
  const time = useLedgerQuery(() => api.listUnbilledTime(null));
  const defaults = useLedgerQuery(api.getBillingDefaults).data;
  // Opened from the sales money bar's Unbilled list: start on that row's customer.
  const [customerId, setCustomerId] = useState(intent?.data?.customerId ?? null);
  useEffect(() => {
    if (intent?.data?.customerId != null) setCustomerId(intent.data.customerId);
  }, [intent]);
  const [picked, setPicked] = useState(() => new Set());
  const [markup, setMarkup] = useState('');
  const [issueDate, setIssueDate] = useState(todayIso);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  const all = costs.data ?? [];
  const allTime = time.data ?? [];
  const customers = useMemo(() => {
    const seen = new Map();
    for (const c of [...all, ...allTime]) if (!seen.has(c.customerId)) seen.set(c.customerId, c.customerName);
    return [...seen.entries()].sort((a, b) => a[1].localeCompare(b[1]));
  }, [all, allTime]);
  useEffect(() => {
    // No customer yet, or one with nothing left to bill (opened from a list that has since changed).
    // Waits for both lists, so a customer with only unbilled time isn't passed over.
    if (!costs.data || !time.data) return;
    if (customers.length && !customers.some(([id]) => id === customerId)) setCustomerId(customers[0][0]);
  }, [customers, customerId, costs.data, time.data]);
  useEffect(() => {
    if (defaults && markup === '') setMarkup(percentText(defaults.markupMilliPercent));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [defaults]);
  const shown = all.filter((c) => c.customerId === customerId);
  useEffect(() => setPicked(new Set(shown.map(key))), [customerId, costs.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const shownTime = allTime.filter((t) => t.customerId === customerId);
  const [pickedTime, setPickedTime] = useState(() => new Set());
  // Time without a service item can't be billed (its line needs the item's income account).
  useEffect(() => setPickedTime(new Set(shownTime.filter((t) => t.productServiceId !== null).map((t) => t.id))), [customerId, time.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const chosenTime = shownTime.filter((t) => pickedTime.has(t.id));
  const timeTotal = chosenTime.reduce((s, t) => s + t.amountCents, 0);
  const toggleTime = (t) =>
    setPickedTime((p) => {
      const next = new Set(p);
      if (next.has(t.id)) next.delete(t.id);
      else next.add(t.id);
      return next;
    });

  const chosen = shown.filter((c) => picked.has(key(c)));
  const milli = parseMarkup(markup, defaults?.markupMilliPercent ?? 0);
  const badMarkup = milli === null;
  const subtotal = chosen.reduce((s, c) => s + c.amountCents, 0);
  const markupTotal = badMarkup ? 0 : chosen.reduce((s, c) => s + markupOf(c.amountCents, milli), 0);
  const toggle = (c) =>
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(key(c))) next.delete(key(c));
      else next.add(key(c));
      return next;
    });

  const bill = async () => {
    const name = customers.find(([id]) => id === customerId)?.[1];
    const what = [chosen.length ? `${chosen.length} cost${chosen.length === 1 ? '' : 's'}` : null, chosenTime.length ? `${chosenTime.length} time entr${chosenTime.length === 1 ? 'y' : 'ies'}` : null].filter(Boolean).join(' and ');
    if (!(await askConfirm(`Invoice ${what} to ${name} for ${formatCents(subtotal + markupTotal + timeTotal)}?`, { confirmLabel: 'Create invoice' }))) return;
    setBusy(true);
    try {
      const invoice = await api.billCosts({
        customerId,
        costs: chosen.map((c) => ({ source: c.source, lineId: c.lineId })),
        time: chosenTime.map((t) => t.id),
        issueDate,
        dueDate: addDays(issueDate, 30),
        markupPercent: markup,
      });
      setNotice({ kind: 'success', text: `Invoice #${invoice.invoiceNumber} to ${invoice.customerName} for ${formatCents(invoice.totalCents)}.` });
      costs.reload();
      time.reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const reimbursedTo = defaults?.asIncome ? 'an income account (counted as income)' : 'each cost’s expense account (reducing the expense)';
  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Bill costs</h2>
          <span className="text-xs text-qb-ink-muted">Billable expenses invoiced to the customer or job they were for</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => navigate('settings', 'preferences')}>
            Preferences…
          </button>
        </div>
        <div className="flex flex-wrap items-end gap-3 px-3 py-2">
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Customer:Job</span>
            <select className="qb-input w-[220px]" value={customerId ?? ''} onChange={(e) => setCustomerId(e.target.value ? Number(e.target.value) : null)} disabled={!customers.length}>
              {customers.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Invoice date</span>
            <DateBox className="qb-input" value={issueDate} onChange={(e) => setIssueDate(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Markup %</span>
            <input className="qb-input num w-[80px]" inputMode="decimal" value={markup} onChange={(e) => setMarkup(e.target.value)} aria-invalid={badMarkup} />
          </label>
          <span className="pb-1 text-2xs text-qb-ink-faint">Credited to {reimbursedTo}; the markup to the markup account in Preferences › Time &amp; Expenses.</span>
        </div>
      </section>
      {costs.error && <Notice kind="error" text={costs.error} />}
      {time.error && <Notice kind="error" text={time.error} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      <section className="qb-panel min-h-0 flex-1 overflow-auto scrollbar-thin">
        {shownTime.length > 0 && (
          <table className="qb-grid border-b border-qb-border">
            <thead>
              <tr>
                <th className="qb-grid-head w-[28px]" aria-label="Bill" />
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Time by</th>
                <th className="qb-grid-head">Service</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head text-right">Hours</th>
                <th className="qb-grid-head text-right">Rate</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {shownTime.map((t) => (
                <tr key={`t${t.id}`} className="qb-grid-row">
                  <td className="qb-grid-cell text-center">
                    <input
                      type="checkbox"
                      className="accent-qb-green"
                      checked={pickedTime.has(t.id)}
                      disabled={t.productServiceId === null}
                      title={t.productServiceId === null ? 'Give this time a service item on the timesheet to bill it' : undefined}
                      onChange={() => toggleTime(t)}
                      aria-label={`Bill time of ${formatDate(t.date)} by ${t.employeeName}`}
                    />
                  </td>
                  <td className="qb-grid-cell">{formatDate(t.date)}</td>
                  <td className="qb-grid-cell">{t.employeeName}</td>
                  <td className="qb-grid-cell">{t.serviceName ?? <span className="text-qb-negative">No service item</span>}</td>
                  <td className="qb-grid-cell">{t.description ?? ''}</td>
                  <td className="qb-grid-cell num">{formatQuantity(t.hoursE4)}</td>
                  <td className="qb-grid-cell num">{formatCents(t.hourlyRateCents)}</td>
                  <td className="qb-grid-cell num">{formatCents(t.amountCents)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {shown.length === 0 ? (
          <p className="px-3 py-2 text-sm text-qb-ink-muted">
            No unbilled costs. Give an expense, check or bill line a Customer:Job and tick Bill? to bill it here; billable time from the timesheet shows above.
          </p>
        ) : (
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[28px]" aria-label="Bill" />
                <th className="qb-grid-head">Date</th>
                <th className="qb-grid-head">Source</th>
                <th className="qb-grid-head">Vendor</th>
                <th className="qb-grid-head">Account</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head text-right">Markup</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((c) => (
                <tr key={key(c)} className="qb-grid-row">
                  <td className="qb-grid-cell text-center">
                    <input type="checkbox" className="accent-qb-green" checked={picked.has(key(c))} onChange={() => toggle(c)} aria-label={`Bill ${c.memo ?? c.accountName} of ${formatDate(c.date)}`} />
                  </td>
                  <td className="qb-grid-cell">{formatDate(c.date)}</td>
                  <td className="qb-grid-cell">{c.source === 'BILL' ? 'Bill' : 'Check / expense'}</td>
                  <td className="qb-grid-cell">{c.vendorName ?? '—'}</td>
                  <td className="qb-grid-cell">{c.accountName}</td>
                  <td className="qb-grid-cell">{c.memo ?? ''}</td>
                  <td className="qb-grid-cell num" title={c.currencyCode ? `${formatMoney(c.foreignAmountCents, c.currencyCode)} at the expense's rate` : undefined}>
                    {formatCents(c.amountCents)}
                    {c.currencyCode && <div className="text-2xs text-qb-ink-muted">{formatMoney(c.foreignAmountCents, c.currencyCode)}</div>}
                  </td>
                  <td className="qb-grid-cell num text-qb-ink-muted">{badMarkup ? '—' : formatCents(markupOf(c.amountCents, milli))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      <div className="flex items-center gap-3">
        <span className="text-sm">
          {chosen.length + chosenTime.length} picked · costs <span className="num font-semibold">{formatCents(subtotal)}</span> · markup <span className="num font-semibold">{formatCents(markupTotal)}</span>
          {chosenTime.length > 0 && (
            <>
              {' '}· time <span className="num font-semibold">{formatCents(timeTotal)}</span>
            </>
          )}{' '}
          · total <span className="num font-semibold">{formatCents(subtotal + markupTotal + timeTotal)}</span>
        </span>
        <button type="button" className="qb-btn qb-btn-primary ml-auto" onClick={bill} disabled={busy || chosen.length + chosenTime.length === 0 || badMarkup}>
          {busy ? 'Creating…' : 'Create invoice'}
        </button>
      </div>
    </div>
  );
}
