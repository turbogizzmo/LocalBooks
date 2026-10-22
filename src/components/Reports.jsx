import { useEffect, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import { classLabel, useClasses } from './ClassSelect.jsx';
import { PrintPortal, StatementDocument, useCompany } from './PrintDocuments.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { periodRange, todayIso } from '../lib/dates.js';

const PERIODS = [
  { id: 'thisMonth', label: 'This month' },
  { id: 'lastMonth', label: 'Last month' },
  { id: 'fiscalQuarter', label: 'This fiscal quarter' },
  { id: 'fiscalYearToDate', label: 'Fiscal year to date' },
  { id: 'fiscalYear', label: 'This fiscal year' },
  { id: 'lastFiscalYear', label: 'Last fiscal year' },
  { id: 'custom', label: 'Custom' },
];

/** Resolves a period preset; fiscal presets come from the company's fiscal year setting. */
function resolvePeriod(id, fiscal) {
  const fromFiscal = { fiscalQuarter: 'thisQuarter', fiscalYearToDate: 'yearToDate', fiscalYear: 'thisYear', lastFiscalYear: 'lastYear' }[id];
  if (fromFiscal && fiscal) return [fiscal[fromFiscal].start, fiscal[fromFiscal].end];
  if (fromFiscal) return periodRange(id === 'fiscalYearToDate' ? 'thisYearToDate' : id === 'lastFiscalYear' ? 'lastYear' : id === 'fiscalQuarter' ? 'thisQuarter' : 'thisYear');
  return periodRange(id);
}

/** "Export" dropdown: CSV through the native save dialog, or the system print dialog (Save as PDF). */
function ExportMenu({ onCsv, onNotice }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => !ref.current?.contains(e.target) && setOpen(false);
    const esc = (e) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', esc);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', esc);
    };
  }, [open]);

  const run = async (action) => {
    setOpen(false);
    try {
      await action();
    } catch (err) {
      onNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const print = printDocument;

  return (
    <div ref={ref} className="relative self-center">
      <button type="button" className="qb-btn qb-btn-secondary h-row" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        Export ▾
      </button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-30 mt-0.5 w-[180px] rounded-xs border border-qb-border-strong bg-white py-0.5 shadow-popover">
          <button type="button" role="menuitem" className="block w-full px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover" onClick={() => run(onCsv)}>
            Export to CSV
          </button>
          <button type="button" role="menuitem" className="block w-full px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover" onClick={() => run(print)}>
            Print / Save to PDF
          </button>
        </div>
      )}
    </div>
  );
}

async function saveCsv(report, params, filename, onNotice) {
  const csv = await api.reportCsv(report, params);
  const path = await api.saveReportToDisk(csv, filename);
  if (path) onNotice({ kind: 'success', text: `Saved ${path}` });
}

const LONG_DATE = new Intl.DateTimeFormat('en-US', { month: 'long', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
const longDate = (iso) => LONG_DATE.format(new Date(`${iso}T00:00:00Z`));

/** One statement line: name left (indented by depth), amounts right, rules by line kind. */
function StatementLine({ line, columns, onAccount }) {
  const indent = { paddingLeft: `${8 + line.depth * 16}px` };
  const strong = line.kind !== 'account';
  const rule =
    line.kind === 'net'
      ? 'border-t border-b-[3px] border-double border-qb-ink'
      : line.kind === 'total' || line.kind === 'subtotal'
        ? 'border-t border-qb-ink'
        : '';
  const spacing = line.kind === 'net' ? 'pb-2' : line.kind === 'total' && line.depth === 0 ? 'pb-1.5' : '';
  const drills = onAccount && line.accountId != null;
  return (
    <tr
      className={`${line.kind === 'account' || drills ? 'hover:bg-qb-row-hover' : ''} ${drills ? 'cursor-pointer' : ''} ${spacing}`}
      onClick={drills ? () => onAccount(line) : undefined}
      title={drills ? 'Open the register for this account' : undefined}
    >
      <td className={`py-0.5 pr-2 ${strong ? 'font-semibold' : ''} ${line.kind === 'net' ? 'text-md' : ''} ${drills ? 'hover:text-qb-info hover:underline' : ''}`} style={indent}>
        {line.label}
      </td>
      {(columns ? line.amountsCents : [line.amountCents]).map((cents, c) => (
        <td key={c} className={`${columns ? 'w-[120px]' : 'w-[160px]'} py-0.5 pr-2 align-bottom ${columns?.[c]?.isTotal || columns?.[c]?.kind === 'subtotal' ? 'bg-qb-canvas' : ''}`}>
          {cents !== null && cents !== undefined && (
            <div className={`num ${strong || columns?.[c]?.isTotal || columns?.[c]?.kind === 'subtotal' ? 'font-semibold' : ''} ${rule} ${line.kind === 'net' ? 'text-md' : ''}`}>{formatCents(cents)}</div>
          )}
        </td>
      ))}
    </tr>
  );
}

/** An on-screen statement; `columns` (from Profit and Loss by Class) gives one amount column each. */
function Statement({ title, subtitle, lines, footer, columns = null, onAccount = null }) {
  const company = useCompany();
  const wide = columns && columns.length > 2;
  return (
    <article className={`mx-auto w-full ${wide ? 'max-w-[1240px] overflow-x-auto' : 'max-w-[760px]'} bg-white px-6 py-4 shadow-panel`}>
      <header className="mb-3 flex items-start gap-4 border-b-[3px] border-double border-qb-ink pb-2">
        <div className="min-w-0 flex-1">
          <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
          <h2 className="text-xl font-semibold">{title}</h2>
          <div className="text-sm text-qb-ink-muted">{subtitle}</div>
        </div>
        {company.logo && <img src={company.logo} alt={`${company.name} logo`} className="max-h-[56px] max-w-[180px] object-contain" />}
      </header>
      <PrintPortal>
        <StatementDocument company={company} title={title} subtitle={subtitle} lines={lines} footer={footer} columns={columns?.map((c) => c.label) ?? null} />
      </PrintPortal>
      <table className="w-full border-collapse text-base">
        <thead>
          <tr className="border-b border-qb-border-strong">
            <th className="py-0.5 text-left text-xs font-semibold uppercase tracking-wide text-qb-ink-muted" />
            {(columns ?? [{ label: 'Total' }]).map((c) => (
              <th key={c.label} className={`py-0.5 pr-2 text-right text-xs font-semibold uppercase tracking-wide ${c.kind === 'subtotal' || c.isTotal ? 'text-qb-ink' : 'text-qb-ink-muted'} ${c.kind === 'other' ? 'italic' : ''}`}>{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {lines.map((line, i) => (
            <StatementLine key={`${i}-${line.label}`} line={line} columns={columns} onAccount={onAccount} />
          ))}
        </tbody>
      </table>
      {footer && <footer className="mt-3 border-t border-qb-border pt-1 text-xs text-qb-ink-muted">{footer}</footer>}
    </article>
  );
}

function ProfitAndLoss({ byClass = false, navigate }) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [period, setPeriod] = useState('fiscalYearToDate');
  const [[start, end], setRange] = useState(() => resolvePeriod('fiscalYearToDate', null));
  const [notice, setNotice] = useState(null);
  const classes = useClasses();
  // '' = all lines, 'none' = lines without a class, otherwise a class id (with its subclasses).
  const [classChoice, setClassChoice] = useState('');
  const classFilter = classChoice === '' ? null : classChoice === 'none' ? { classIds: [], unclassified: true } : { classIds: [Number(classChoice)], unclassified: false };
  const { data, error, reload } = useLedgerQuery(() => (byClass ? api.getProfitAndLossByClass(start, end) : api.getProfitAndLoss(start, end, classFilter)));

  // Once the fiscal calendar loads (or changes in Settings), re-resolve the selected preset.
  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolvePeriod(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (start && end && start <= end) reload();
  }, [start, end, classChoice, reload]);
  const classSubtitle = classChoice === '' ? '' : ` · Class: ${classChoice === 'none' ? 'Unclassified' : `${classLabel(classes, Number(classChoice))} (with subclasses)`}`;

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Report period</span>
          <select
            className="qb-input w-[160px]"
            value={period}
            onChange={(e) => {
              setPeriod(e.target.value);
              if (e.target.value !== 'custom') setRange(resolvePeriod(e.target.value, fiscal));
            }}
          >
            {PERIODS.map((p) => (
              <option key={p.id} value={p.id}>{p.label}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">From</span>
          <input type="date" className="qb-input w-[132px]" value={start} onChange={(e) => { setPeriod('custom'); setRange([e.target.value, end]); }} />
        </label>
        <label>
          <span className="qb-label">To</span>
          <input type="date" className="qb-input w-[132px]" value={end} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value]); }} aria-invalid={end < start} />
        </label>
        {!byClass && classes.length > 0 && (
          <label>
            <span className="qb-label">Class</span>
            <select className="qb-input w-[180px]" value={classChoice} onChange={(e) => setClassChoice(e.target.value)}>
              <option value="">All classes</option>
              {classes.map((c) => (
                <option key={c.id} value={c.id}>
                  {'\u00a0\u00a0'.repeat(c.depth)}
                  {c.name}
                </option>
              ))}
              <option value="none">Unclassified</option>
            </select>
          </label>
        )}
        {data && !byClass && (
          <div className="ml-auto flex gap-4 self-center text-sm">
            <span className="text-qb-ink-muted">Revenue <span className="num font-semibold text-qb-ink">{formatCents(data.totalRevenueCents)}</span></span>
            <span className="text-qb-ink-muted">Expenses <span className="num font-semibold text-qb-ink">{formatCents(data.totalExpensesCents)}</span></span>
            <span className="text-qb-ink-muted">Net income <span className={`num font-semibold ${data.netIncomeCents < 0 ? 'text-qb-negative' : 'text-qb-green-active'}`}>{data.netIncomeDisplay}</span></span>
          </div>
        )}
        <div className={data && !byClass ? '' : 'ml-auto'}>
          <ExportMenu
            onNotice={setNotice}
            onCsv={() =>
              byClass
                ? saveCsv('profit_and_loss_by_class', { startDate: start, endDate: end }, `Profit-and-Loss-by-Class_${start}_to_${end}.csv`, setNotice)
                : saveCsv('profit_and_loss', { startDate: start, endDate: end, classFilter }, `Profit-and-Loss_${start}_to_${end}.csv`, setNotice)
            }
          />
        </div>
      </div>
      {end < start && <Notice kind="error" text="The end date is before the start date." />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {byClass && data && data.columns.length === 1 && (
        <Notice kind="info" text="No income or expenses in this period carry a class. Add classes under Accounting › Classes, then choose one on each line." />
      )}
      {data && (
        <Statement
          title={byClass ? 'Profit and Loss by Class' : 'Profit and Loss'}
          subtitle={(data.startDate === data.endDate ? longDate(data.startDate) : `${longDate(data.startDate)} – ${longDate(data.endDate)}`) + (byClass ? '' : classSubtitle)}
          lines={data.lines}
          columns={byClass ? data.columns : null}
          onAccount={(line) =>
            navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: data.startDate, endDate: data.endDate, includeSubaccounts: line.kind === 'subtotal' })
          }
          footer={byClass ? 'Accrual basis · posted transactions only · lines without a class are in Unclassified' : 'Accrual basis · posted transactions only'}
        />
      )}
    </>
  );
}

function BalanceSheet({ navigate }) {
  const [asOf, setAsOf] = useState(todayIso());
  const [notice, setNotice] = useState(null);
  const { data, error, reload } = useLedgerQuery(() => api.getBalanceSheet(asOf));

  useEffect(() => {
    if (asOf) reload();
  }, [asOf, reload]);

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">As of</span>
          <input type="date" className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
        {data && (
          <div className="ml-auto self-center text-sm">
            {data.isBalanced ? (
              <span className="font-semibold text-qb-green-active">✓ Assets = Liabilities + Equity</span>
            ) : (
              <span className="font-semibold text-qb-negative">⚠ Out of balance by {formatCents(data.differenceCents)}</span>
            )}
          </div>
        )}
        <div className={data ? '' : 'ml-auto'}>
          <ExportMenu onNotice={setNotice} onCsv={() => saveCsv('balance_sheet', { asOf }, `Balance-Sheet_${asOf}.csv`, setNotice)} />
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <Statement
          title="Balance Sheet"
          subtitle={`As of ${longDate(data.asOf)}`}
          lines={data.lines}
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: null, endDate: data.asOf, includeSubaccounts: line.kind === 'subtotal' })}
          footer={`Accrual basis · fiscal year began ${longDate(data.fiscalYearStart)} · prior years' profit is shown in Retained Earnings`}
        />
      )}
    </>
  );
}

export default function Reports({ navigate }) {
  const [report, setReport] = useState('pl');
  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="no-print flex flex-none gap-px self-start overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist">
        {[
          ['pl', 'Profit and Loss'],
          ['plc', 'Profit and Loss by Class'],
          ['bs', 'Balance Sheet'],
        ].map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={report === id}
            onClick={() => setReport(id)}
            className={`px-3 py-0.5 font-semibold ${report === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
          >
            {label}
          </button>
        ))}
      </div>
      {report === 'pl' && <ProfitAndLoss key="pl" navigate={navigate} />}
      {report === 'plc' && <ProfitAndLoss key="plc" byClass navigate={navigate} />}
      {report === 'bs' && <BalanceSheet navigate={navigate} />}
    </div>
  );
}
