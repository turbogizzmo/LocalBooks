import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import DateBox from './DateBox.jsx';
import { BillableSummary, BillableTables, billableTotals, parseMarkup, percentText, useBillablePicks } from './BillablePicker.jsx';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, todayIso } from '../lib/dates.js';

/**
 * Sales › Bill Costs: invoice billable expenses (checks, expenses, card charges and bill lines
 * marked Bill? for a customer) to that customer, with the markup from Preferences › Time &
 * Expenses, and billable time from the timesheet (one line per entry: its service item, hours and
 * rate; no markup). The invoice form's Add billable costs… shows the same rows (BillablePicker.jsx).
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
  const shownTime = allTime.filter((t) => t.customerId === customerId);
  // Everything billable is picked again for another customer or when the lists reload.
  const resetKey = useMemo(() => ({}), [customerId, costs.data, time.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const picks = useBillablePicks(shown, shownTime, resetKey);
  const { chosen, chosenTime } = picks;
  const milli = parseMarkup(markup, defaults?.markupMilliPercent ?? 0);
  const badMarkup = milli === null;
  const totals = billableTotals(chosen, chosenTime, milli);

  const bill = async () => {
    const name = customers.find(([id]) => id === customerId)?.[1];
    const what = [chosen.length ? `${chosen.length} cost${chosen.length === 1 ? '' : 's'}` : null, chosenTime.length ? `${chosenTime.length} time entr${chosenTime.length === 1 ? 'y' : 'ies'}` : null].filter(Boolean).join(' and ');
    if (!(await askConfirm(`Invoice ${what} to ${name} for ${formatCents(totals.total)}?`, { confirmLabel: 'Create invoice' }))) return;
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
        <BillableTables shown={shown} shownTime={shownTime} picks={picks} milli={milli} />
      </section>
      <div className="flex items-center gap-3">
        <BillableSummary chosen={chosen} chosenTime={chosenTime} totals={totals} />
        <button type="button" className="qb-btn qb-btn-primary ml-auto" onClick={bill} disabled={busy || chosen.length + chosenTime.length === 0 || badMarkup}>
          {busy ? 'Creating…' : 'Create invoice'}
        </button>
      </div>
    </div>
  );
}
