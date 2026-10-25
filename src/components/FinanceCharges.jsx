import { Fragment, useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/** 18000 thousandths of a percent → "18%". */
function rateLabel(milli) {
  return `${(milli / 1000).toLocaleString('en-US', { maximumFractionDigits: 3 })}%`;
}

/** Sales › Assess Finance Charges: charge interest on overdue invoices, one invoice per customer. */
export default function FinanceCharges({ accounts, navigate }) {
  const [asOf, setAsOf] = useState(todayIso);
  const preview = useLedgerQuery(() => api.previewFinanceCharges(asOf));
  // Per customer: { picked, amount (text) }; reset whenever the list is recalculated.
  const [rows, setRows] = useState({});
  const [open, setOpen] = useState(() => new Set());
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (asOf) preview.reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf]);

  const data = preview.data;
  const customers = data?.asOf === asOf ? data.customers : [];
  useEffect(() => {
    setRows(Object.fromEntries(customers.map((c) => [c.customerName, { picked: true, amount: centsToInput(c.chargeCents) }])));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data]);

  const settings = data?.settings;
  const account = settings?.incomeAccountId ? accounts.find((a) => a.id === settings.incomeAccountId) : null;
  const row = (c) => rows[c.customerName] ?? { picked: false, amount: centsToInput(c.chargeCents) };
  const amount = (c) => parseMoneyToCents(row(c).amount);
  const picked = customers.filter((c) => row(c).picked);
  const invalid = picked.some((c) => !(amount(c) > 0));
  const total = picked.reduce((sum, c) => sum + (amount(c) > 0 ? amount(c) : 0), 0);
  const update = (name, change) => setRows((r) => ({ ...r, [name]: { ...r[name], ...change } }));
  const toggleOpen = (name) =>
    setOpen((o) => {
      const next = new Set(o);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  const assess = async () => {
    if (!window.confirm(`Create ${picked.length} finance charge invoice${picked.length === 1 ? '' : 's'} dated ${formatDate(asOf)}, totalling ${formatCents(total)}?`)) return;
    setBusy(true);
    try {
      const made = await api.assessFinanceCharges({ asOf, charges: picked.map((c) => ({ customerName: c.customerName, chargeCents: amount(c) })) });
      setNotice({ kind: 'success', text: `Assessed ${made.length} finance charge${made.length === 1 ? '' : 's'}: invoice${made.length === 1 ? '' : 's'} ${made.map((i) => `#${i.invoiceNumber}`).join(', ')}.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Assess finance charges</h2>
          <span className="text-xs text-qb-ink-muted">Interest on overdue invoices, billed as an invoice to each customer</span>
        </div>
        <div className="flex flex-wrap items-end gap-3 px-3 py-2">
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Assessment date</span>
            <input type="date" className="qb-input" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
          </label>
          {settings && (
            <span className="pb-1 text-xs text-qb-ink-muted">
              {rateLabel(settings.annualRateMilliPercent)} a year · minimum {formatCents(settings.minimumCents)} · {settings.graceDays} grace day
              {settings.graceDays === 1 ? '' : 's'} · {settings.chargeFinanceCharges ? 'charged' : 'not charged'} on earlier finance charges · credited to{' '}
              {account ? account.fullName : 'no account yet'}
            </span>
          )}
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => navigate('settings', 'finance-charges')}>
            Finance charge settings…
          </button>
        </div>
      </section>

      {preview.error && <Notice kind="error" text={preview.error} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {settings && !settings.incomeAccountId && (
        <Notice kind="info" text="Choose the income account for finance charges, and an annual rate, in Settings › Finance charges before assessing." />
      )}

      {data && customers.length === 0 && (
        <div className="qb-panel px-3 py-2 text-sm text-qb-ink-muted">No customer has an overdue balance to charge as of {formatDate(asOf)}.</div>
      )}

      {customers.length > 0 && (
        <section className="qb-panel">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[32px]">
                  <input
                    type="checkbox"
                    className="accent-qb-green"
                    aria-label="Assess all"
                    checked={picked.length === customers.length}
                    onChange={(e) => setRows((r) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, { ...v, picked: e.target.checked }])))}
                  />
                </th>
                <th className="qb-grid-head">Customer</th>
                <th className="qb-grid-head">Last finance charge</th>
                <th className="qb-grid-head text-right">Overdue balance</th>
                <th className="qb-grid-head text-right">Interest</th>
                <th className="qb-grid-head w-[130px] text-right">Finance charge</th>
              </tr>
            </thead>
            <tbody>
              {customers.map((c) => {
                const r = row(c);
                const bad = r.picked && !(amount(c) > 0);
                return (
                  <Fragment key={c.customerName}>
                    <tr>
                      <td className="qb-grid-cell">
                        <input type="checkbox" className="accent-qb-green" aria-label={`Assess ${c.customerName}`} checked={r.picked} onChange={(e) => update(c.customerName, { picked: e.target.checked })} />
                      </td>
                      <td className="qb-grid-cell">
                        <button type="button" className="text-left hover:underline" onClick={() => toggleOpen(c.customerName)} aria-expanded={open.has(c.customerName)}>
                          {open.has(c.customerName) ? '▾' : '▸'} {c.customerName}
                        </button>
                      </td>
                      <td className="qb-grid-cell">{c.lastFinanceChargeDate ? formatDate(c.lastFinanceChargeDate) : '—'}</td>
                      <td className="qb-grid-cell num">{formatCents(c.overdueBalanceCents)}</td>
                      <td className="qb-grid-cell num" title={c.chargeCents > c.interestCents ? 'Raised to the minimum charge' : undefined}>
                        {formatCents(c.interestCents)}
                        {c.chargeCents > c.interestCents && <span className="ml-1 text-2xs text-qb-ink-faint">min</span>}
                      </td>
                      <td className="qb-grid-cell">
                        <input
                          className="qb-input num w-full text-right"
                          value={r.amount}
                          inputMode="decimal"
                          aria-label={`Finance charge for ${c.customerName}`}
                          aria-invalid={bad}
                          onChange={(e) => update(c.customerName, { amount: e.target.value })}
                        />
                      </td>
                    </tr>
                    {open.has(c.customerName) &&
                      c.invoices.map((i) => (
                        <tr key={i.invoiceId} className="text-xs text-qb-ink-muted">
                          <td className="qb-grid-cell" />
                          <td className="qb-grid-cell pl-5">
                            {i.isFinanceCharge ? 'Finance charge' : 'Invoice'} #{i.invoiceNumber}, due {formatDate(i.dueDate)}
                          </td>
                          <td className="qb-grid-cell">from {formatDate(i.fromDate)}</td>
                          <td className="qb-grid-cell num">{formatCents(i.balanceCents)}</td>
                          <td className="qb-grid-cell num">{i.days} day{i.days === 1 ? '' : 's'}</td>
                          <td className="qb-grid-cell" />
                        </tr>
                      ))}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
          <div className="flex items-center gap-3 border-t border-qb-border px-3 py-2">
            <span className="text-sm">
              {picked.length} selected · total <strong className="num">{formatCents(total)}</strong>
            </span>
            {invalid && <span className="text-xs text-qb-negative">Enter a charge above zero for each ticked customer.</span>}
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row-lg px-3" disabled={busy || picked.length === 0 || invalid || !settings?.incomeAccountId} onClick={assess}>
              Assess charges
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
