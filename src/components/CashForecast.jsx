import { useEffect, useState } from 'react';
import { TableReport } from './MoreReports.jsx';
import { longDate, plainAmount } from './ReportParts.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';
import DateBox from './DateBox.jsx';

const WEEK_CHOICES = [4, 8, 13, 26];
const money = (cents) => (cents ? formatCents(cents) : '');

/** A document due in a column, for the rows opened under Money in / Money out. */
const itemText = (it) => `${it.partyName} · ${it.kind}${it.number ? ` #${it.number}` : ''} · due ${formatDate(it.dueDate)}`;

/**
 * Cash Flow Forecast: cash on hand, then week by week the money due in from open invoices and out
 * to open bills, and the cash left. Its money in and out are the A/R and A/P agings' open items, so
 * the totals tie out to them (cash_forecast.rs). Click Money in or Money out for the documents.
 */
export function CashFlowForecastReport() {
  const [asOf, setAsOf] = useState(todayIso());
  const [weeks, setWeeks] = useState(8);
  const { data, error, reload } = useLedgerQuery(() => (asOf ? api.getCashFlowForecast(asOf, weeks) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, weeks, reload]);
  const [open, setOpen] = useState({ IN: false, OUT: false });
  const toggle = (direction) => setOpen((o) => ({ ...o, [direction]: !o[direction] }));

  const periods = data?.periods ?? [];
  const docs = (direction) =>
    periods.flatMap((p, c) =>
      p.items
        .filter((it) => it.direction === direction)
        .map((it) => ({ cells: [<span key="d" className="pl-4 text-qb-ink-muted">{itemText(it)}</span>, ...periods.map((_, k) => (k === c ? formatCents(it.amountCents) : ''))] })),
    );
  const arrow = (direction) => <span aria-hidden="true" className="inline-block w-3 text-2xs text-qb-ink-muted">{open[direction] ? '▾' : '▸'}</span>;
  const rows = data
    ? [
        { kind: 'section', cells: ['Cash on hand', formatCents(data.startingCashCents), ...periods.slice(1).map(() => '')] },
        {
          cells: [<span key="l">{arrow('IN')}Money in (open invoices)</span>, ...periods.map((p) => money(p.moneyInCents))],
          onClick: () => toggle('IN'),
        },
        ...(open.IN ? docs('IN') : []),
        {
          cells: [<span key="l">{arrow('OUT')}Money out (open bills)</span>, ...periods.map((p) => money(p.moneyOutCents))],
          onClick: () => toggle('OUT'),
        },
        ...(open.OUT ? docs('OUT') : []),
        { kind: 'total', cells: ['Net', ...periods.map((p) => formatCents(p.netCents))] },
        { kind: 'grand', cells: ['Cash at the end', ...periods.map((p) => formatCents(p.endingCashCents))] },
      ]
    : [];
  const subtitle = data
    ? `From ${longDate(data.asOf)}, ${data.weeks} weeks · cash on hand ${formatCents(data.bankCents)} in the bank${data.undepositedCents ? ` + ${formatCents(data.undepositedCents)} not yet deposited` : ''}`
    : '';
  return (
    <TableReport
      title="Cash Flow Forecast"
      subtitle={subtitle}
      columns={[{ label: '' }, ...periods.map((p) => ({ label: p.label, align: 'right' }))]}
      rows={rows}
      error={error}
      toolbar={
        <>
          <label>
            <span className="qb-label">From</span>
            <DateBox className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">Weeks</span>
            <select className="qb-input w-[80px]" value={weeks} onChange={(e) => setWeeks(Number(e.target.value))}>
              {WEEK_CHOICES.map((w) => (
                <option key={w} value={w}>
                  {w}
                </option>
              ))}
            </select>
          </label>
        </>
      }
      footer={
        data
          ? `Money in is every open invoice (less unused customer credit) on its due date, ${formatCents(data.receivableCents)} in all, the A/R Aging total; money out is every open bill (less unused vendor credit), ${formatCents(data.payableCents)}, the A/P Aging total. Past due is expected now; Later is due after the last week. Click Money in or Money out for the documents.`
          : ''
      }
      onCsvRows={() => [
        ['Cash on hand', plainAmount(data.startingCashCents)],
        ['Money in', ...periods.map((p) => plainAmount(p.moneyInCents))],
        ['Money out', ...periods.map((p) => plainAmount(p.moneyOutCents))],
        ['Net', ...periods.map((p) => plainAmount(p.netCents))],
        ['Cash at the end', ...periods.map((p) => plainAmount(p.endingCashCents))],
      ]}
    />
  );
}

/**
 * The Dashboard's cash flow card: cash on hand and, for the next eight weeks, money due in and out
 * with the cash left at the end of each week. Click it for the Cash Flow Forecast report.
 */
export function CashForecastWidget({ navigate }) {
  const { data, error } = useLedgerQuery(() => api.getCashFlowForecast(todayIso(), 8));
  const weeks = data ? data.periods.filter((p) => p.kind === 'WEEK') : [];
  const pastDue = data?.periods.find((p) => p.kind === 'PAST_DUE');
  // Credits can make a week's total negative: its bar shows the size, faded, and the tooltip the sign.
  const scale = Math.max(1, ...weeks.flatMap((w) => [Math.abs(w.moneyInCents), Math.abs(w.moneyOutCents)]));
  return (
    <button
      type="button"
      className="qb-panel w-full px-2 py-1 text-left hover:bg-qb-row-hover"
      onClick={() => navigate?.('reports', 'open', { report: 'cash-forecast' })}
      title="Open the Cash Flow Forecast report"
    >
      <div className="flex items-baseline justify-between">
        <span className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Cash flow · next 8 weeks</span>
        <span className="text-xs text-qb-ink-muted">
          Cash on hand <span className="num text-md font-semibold text-qb-ink">{data ? formatCents(data.startingCashCents) : '—'}</span>
        </span>
      </div>
      {error && <div className="text-xs text-qb-negative">{error}</div>}
      {data && (
        <>
          <div className="mt-1 grid grid-cols-8 gap-1" role="img" aria-label="Money due in and out by week">
            {weeks.map((w) => (
              <div key={w.label} className="flex min-w-0 flex-col items-stretch" title={`${w.label}: in ${formatCents(w.moneyInCents)}, out ${formatCents(w.moneyOutCents)}, cash at the end ${formatCents(w.endingCashCents)}`}>
                <div className="flex h-10 items-end gap-[2px]">
                  <div className={`flex-1 rounded-t-[2px] bg-qb-green ${w.moneyInCents < 0 ? 'opacity-40' : ''}`} style={{ height: `${(Math.abs(w.moneyInCents) / scale) * 100}%` }} />
                  <div className={`flex-1 rounded-t-[2px] bg-qb-negative ${w.moneyOutCents < 0 ? 'opacity-40' : ''}`} style={{ height: `${(Math.abs(w.moneyOutCents) / scale) * 100}%` }} />
                </div>
                <div className="truncate text-2xs text-qb-ink-muted">{w.label.replace('Week of ', '')}</div>
                <div className={`num truncate text-2xs ${w.endingCashCents < 0 ? 'text-qb-negative' : ''}`}>{formatCents(w.endingCashCents)}</div>
              </div>
            ))}
          </div>
          <div className="mt-0.5 text-2xs text-qb-ink-muted">
            <span className="mr-1 inline-block h-2 w-2 rounded-[2px] bg-qb-green" />
            due in · <span className="mx-1 inline-block h-2 w-2 rounded-[2px] bg-qb-negative" />
            due out · cash left at each week's end
            {pastDue && (pastDue.moneyInCents || pastDue.moneyOutCents) ? ` · past due: ${formatCents(pastDue.moneyInCents)} in, ${formatCents(pastDue.moneyOutCents)} out (counted first)` : ''}
          </div>
        </>
      )}
    </button>
  );
}
