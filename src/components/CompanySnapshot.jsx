import { useEffect, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatCompactCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { moveWidget } from '../lib/companySnapshot.js';
import Notice from './Notice.jsx';

export const WIDGETS = {
  ACCOUNT_BALANCES: { title: 'Account balances', total: 'In the bank', report: 'Balance Sheet', asOf: true },
  WHO_OWES: { title: 'Who owes you', total: 'Total owed', report: 'A/R Aging Summary', asOf: true },
  VENDORS_TO_PAY: { title: 'Vendors to pay', total: 'Total to pay', report: 'A/P Aging Summary', asOf: true },
  INCOME_EXPENSE_TREND: { title: 'Income and expense trend', total: 'Net income', report: 'Profit and Loss' },
  TOP_CUSTOMERS: { title: 'Top customers', total: 'Total income', report: 'Sales by Customer Summary' },
  TOP_ITEMS: { title: 'Top items', total: 'Total sales', report: 'Sales by Product/Service Summary' },
  TOP_VENDORS: { title: 'Top vendors', total: 'Total expenses', report: 'Expenses by Vendor Summary' },
  INCOME_BREAKDOWN: { title: 'Income breakdown', total: 'Total income', report: 'Profit and Loss' },
  EXPENSE_BREAKDOWN: { title: 'Expense breakdown', total: 'Total expenses', report: 'Profit and Loss' },
  YEAR_OVER_YEAR: { title: 'Year over year', total: 'Net income', report: 'Profit and Loss' },
  TODOS: { title: 'To-dos', total: 'Open to-dos', report: 'To Do Notes', asOf: true, count: true },
  NOTES: { title: 'Note', note: true },
};

const PERIODS = [
  ['TODAY', 'Today (year to date)'],
  ['THIS_MONTH', 'This month'],
  ['LAST_MONTH', 'Last month'],
  ['THIS_QUARTER', 'This quarter'],
  ['LAST_QUARTER', 'Last quarter'],
  ['THIS_FISCAL_YEAR', 'This fiscal year'],
  ['LAST_FISCAL_YEAR', 'Last fiscal year'],
  ['LAST_12_MONTHS', 'Last 12 months'],
];

const GREEN = '#0b5d57';
const BLUE = '#22609e';

/**
 * Income and expense columns by month, with the month's net in each column's tooltip. Built from
 * boxes rather than a scaled drawing, so it keeps its height and text size at any width: it sits in
 * a narrow Company Snapshot card and across the whole Insights tab.
 */
export function Trend({ points, height = 96 }) {
  const max = Math.max(1, ...points.flatMap((p) => [p.incomeCents, p.expenseCents]));
  const pct = (cents) => `${(Math.max(0, cents) * 100) / max}%`;
  const empty = points.every((p) => p.incomeCents <= 0 && p.expenseCents <= 0);
  return (
    <div role="img" aria-label="Income and expense by month">
      <div className="relative flex items-stretch gap-px border-b border-qb-border" style={{ height }}>
        {empty && (
          <div className="absolute inset-0 flex items-center justify-center text-xs text-qb-ink-muted">
            No income or expenses in these months.
          </div>
        )}
        {points.map((p) => (
          <div
            key={p.month}
            className="flex min-w-0 flex-1 items-end justify-center gap-px"
            title={`${p.month}: income ${formatCents(p.incomeCents)}, expense ${formatCents(p.expenseCents)}, net ${formatCents(p.netCents)}`}
          >
            <div className="w-full max-w-[14px]" style={{ height: pct(p.incomeCents), backgroundColor: GREEN }} />
            <div className="w-full max-w-[14px]" style={{ height: pct(p.expenseCents), backgroundColor: BLUE }} />
          </div>
        ))}
      </div>
      <div className="flex gap-px text-2xs text-qb-ink-muted">
        {points.map((p) => (
          <div key={p.month} className="min-w-0 flex-1 text-center">
            {p.month.slice(5)}
          </div>
        ))}
      </div>
    </div>
  );
}

/** The note widget's text box; it saves when it loses focus. */
function NoteBox({ widget, onSave }) {
  const [text, setText] = useState(widget.note ?? '');
  useEffect(() => setText(widget.note ?? ''), [widget.note]);
  return (
    <textarea
      className="qb-input min-h-24 w-full resize-y text-sm"
      aria-label="Note"
      maxLength={2000}
      placeholder="Write a note to keep on this page"
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        if (text.trim() !== (widget.note ?? '')) onSave(text);
      }}
    />
  );
}

/** Year over year: this period, the same dates a year earlier, and the change. */
function YearOverYear({ lines }) {
  return (
    <table className="w-full">
      <thead>
        <tr className="text-xs text-qb-ink-muted">
          <th className="py-0.5 text-left font-normal" />
          <th className="py-0.5 text-right font-normal">This period</th>
          <th className="py-0.5 text-right font-normal">A year earlier</th>
          <th className="py-0.5 text-right font-normal">Change</th>
        </tr>
      </thead>
      <tbody>
        {lines.map((l) => {
          const change = l.amountCents - (l.previousCents ?? 0);
          // More expenses is the worse way; for income and net, less is.
          const worse = l.label === 'Expenses' ? change > 0 : change < 0;
          return (
            <tr key={l.label}>
              <td className="py-0.5">{l.label}</td>
              <td className="num py-0.5 text-right">{formatCents(l.amountCents)}</td>
              <td className="num py-0.5 text-right text-qb-ink-muted">{formatCents(l.previousCents ?? 0)}</td>
              <td className={`num py-0.5 text-right ${worse ? 'text-qb-negative' : ''}`}>{formatCents(change)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function WidgetCard({ widget, index, count, onMove, onRemove, onPeriod, onOpen, onNote }) {
  const meta = WIDGETS[widget.kind];
  // The aging widgets have an overdue column between the name and the amount.
  const aging = widget.kind === 'WHO_OWES' || widget.kind === 'VENDORS_TO_PAY';
  const dates = meta.asOf ? `as of ${formatDate(widget.endDate)}` : `${formatDate(widget.startDate)} – ${formatDate(widget.endDate)}`;
  return (
    <section className="qb-panel flex flex-col" aria-label={meta.title}>
      <div className="flex items-center gap-1 border-b border-qb-border px-2 py-1">
        <h2 className="flex-1 text-md font-semibold">{meta.title}</h2>
        <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-1.5" aria-label={`Move ${meta.title} earlier`} disabled={index === 0} onClick={() => onMove(-1)}>↑</button>
        <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-1.5" aria-label={`Move ${meta.title} later`} disabled={index === count - 1} onClick={() => onMove(1)}>↓</button>
        <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-1.5" aria-label={`Remove ${meta.title}`} onClick={onRemove}>✕</button>
      </div>
      {!meta.note && <div className="flex items-center gap-2 px-2 py-1 text-xs text-qb-ink-muted">
        <select className="qb-input h-row text-xs" aria-label={`Period for ${meta.title}`} value={widget.period} onChange={(e) => onPeriod(e.target.value)}>
          {PERIODS.map(([id, label]) => (
            <option key={id} value={id}>{label}</option>
          ))}
        </select>
        <span>{dates}</span>
      </div>}
      <div className="flex-1 px-2 pb-1 text-sm">
        {meta.note ? (
          <div className="pt-1">
            <NoteBox widget={widget} onSave={onNote} />
          </div>
        ) : widget.kind === 'INCOME_EXPENSE_TREND' ? (
          <Trend points={widget.trend} />
        ) : widget.kind === 'YEAR_OVER_YEAR' ? (
          <YearOverYear lines={widget.lines} />
        ) : (
          <table className="w-full">
            <tbody>
              {widget.lines.map((l) => (
                <tr key={l.label}>
                  <td className="truncate py-0.5">{l.label}</td>
                  {l.overdueCents !== null && l.overdueCents !== undefined && (
                    <td className={`py-0.5 text-right text-xs ${l.overdueCents > 0 ? 'text-qb-negative' : 'text-qb-ink-muted'}`}>{l.overdueCents > 0 ? `${formatCompactCents(l.overdueCents)} overdue` : ''}</td>
                  )}
                  {meta.count ? (
                    <td className={`py-0.5 text-right text-xs ${l.date && l.date < widget.endDate ? 'text-qb-negative' : 'text-qb-ink-muted'}`}>{l.date ? `due ${formatDate(l.date)}` : 'no date'}</td>
                  ) : (
                    <td className="num py-0.5 text-right">{formatCents(l.amountCents)}</td>
                  )}
                </tr>
              ))}
              {widget.othersCents !== 0 && (
                <tr className="text-qb-ink-muted">
                  <td className="py-0.5" colSpan={aging ? 2 : 1}>Others</td>
                  <td className="num py-0.5 text-right">{formatCents(widget.othersCents)}</td>
                </tr>
              )}
              {widget.lines.length === 0 && (
                <tr>
                  <td className="py-1 text-qb-ink-muted" colSpan={aging ? 3 : 2}>{meta.count ? 'Nothing to do.' : 'Nothing in these dates.'}</td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
      {!meta.note && (
        <>
          <div className="flex items-center gap-2 border-t border-qb-border px-2 py-1 text-sm">
            <span className="font-semibold">{meta.total}</span>
            <span className="num ml-auto font-semibold">{meta.count ? widget.totalCents : formatCents(widget.totalCents)}</span>
          </div>
          <button type="button" className="mx-2 mb-1 self-start text-xs text-qb-info hover:underline" onClick={onOpen}>
            Open {meta.report}
          </button>
        </>
      )}
    </section>
  );
}

/**
 * Reports › Company Snapshot (`company_snapshot.rs`): widgets the user adds, removes, orders and
 * dates. Each figure comes from the same calculation as its report, which Open … shows in full.
 */
export default function CompanySnapshot({ onOpenReport }) {
  const { data: widgets, error, reload } = useLedgerQuery(api.getCompanySnapshot);
  const [notice, setNotice] = useState(null);
  const [adding, setAdding] = useState('');
  const act = async (fn) => {
    try {
      await fn();
      setNotice(null);
      reload();
    } catch (err) {
      setNotice(errorText(err));
    }
  };
  const list = widgets ?? [];
  return (
    <div className="mx-auto flex w-full max-w-[1400px] flex-col gap-2">
      <div className="flex items-center gap-2">
        <h1 className="text-xl font-semibold">Company Snapshot</h1>
        <span className="text-xs text-qb-ink-muted">Each figure matches its report for the same dates</span>
        <label className="ml-auto flex items-center gap-1 text-sm">
          <span className="text-qb-ink-muted">Add</span>
          <select
            className="qb-input h-row"
            value={adding}
            onChange={(e) => {
              const kind = e.target.value;
              setAdding('');
              if (kind) act(() => api.addSnapshotWidget(kind));
            }}
          >
            <option value="">a widget…</option>
            {Object.entries(WIDGETS).map(([kind, w]) => (
              <option key={kind} value={kind}>{w.title}</option>
            ))}
          </select>
        </label>
      </div>
      {error && <Notice kind="error" text={error} />}
      {notice && <Notice kind="error" text={notice} onDismiss={() => setNotice(null)} />}
      {widgets && list.length === 0 && <p className="text-sm text-qb-ink-muted">No widgets. Add one from the list above.</p>}
      <div className="grid gap-2" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))' }}>
        {list.map((w, i) => (
          <WidgetCard
            key={w.id}
            widget={w}
            index={i}
            count={list.length}
            onMove={(by) => act(() => api.arrangeSnapshotWidgets(moveWidget(list.map((x) => x.id), i, by)))}
            onRemove={() => act(() => api.removeSnapshotWidget(w.id))}
            onPeriod={(period) => act(() => api.setSnapshotWidgetPeriod(w.id, period))}
            onNote={(text) => act(() => api.setSnapshotWidgetNote(w.id, text))}
            onOpen={() => onOpenReport?.(w.report, WIDGETS[w.kind].asOf ? { asOf: w.endDate, endDate: w.endDate } : { startDate: w.startDate, endDate: w.endDate })}
          />
        ))}
      </div>
    </div>
  );
}
