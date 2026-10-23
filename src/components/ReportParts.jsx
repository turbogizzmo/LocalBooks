import { useEffect, useRef, useState } from 'react';
import { PrintPortal, StatementDocument, useCompany } from './PrintDocuments.jsx';
import { api, errorText, printDocument } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { periodRange } from '../lib/dates.js';

/** Building blocks shared by every report: period presets, the Export menu, and statement layout. */

export const PERIODS = [
  { id: 'thisMonth', label: 'This month' },
  { id: 'lastMonth', label: 'Last month' },
  { id: 'fiscalQuarter', label: 'This fiscal quarter' },
  { id: 'fiscalYearToDate', label: 'Fiscal year to date' },
  { id: 'fiscalYear', label: 'This fiscal year' },
  { id: 'lastFiscalYear', label: 'Last fiscal year' },
  { id: 'custom', label: 'Custom' },
];

/** Resolves a period preset; fiscal presets come from the company's fiscal year setting. */
export function resolvePeriod(id, fiscal) {
  const fromFiscal = { fiscalQuarter: 'thisQuarter', fiscalYearToDate: 'yearToDate', fiscalYear: 'thisYear', lastFiscalYear: 'lastYear' }[id];
  if (fromFiscal && fiscal) return [fiscal[fromFiscal].start, fiscal[fromFiscal].end];
  if (fromFiscal) return periodRange(id === 'fiscalYearToDate' ? 'thisYearToDate' : id === 'lastFiscalYear' ? 'lastYear' : id === 'fiscalQuarter' ? 'thisQuarter' : 'thisYear');
  return periodRange(id);
}

/** "Export" dropdown: CSV through the native save dialog, or the system print dialog (Save as PDF). */
export function ExportMenu({ onCsv, onNotice }) {
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

export async function saveCsv(report, params, filename, onNotice) {
  const csv = await api.reportCsv(report, params);
  const path = await api.saveReportToDisk(csv, filename);
  if (path) onNotice({ kind: 'success', text: `Saved ${path}` });
}

const LONG_DATE = new Intl.DateTimeFormat('en-US', { month: 'long', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
/** Amounts in a "% Change" column are basis points (12.5% = 1250); every other column is money. */
export const formatColumnAmount = (column, cents) => (column?.kind === 'percent' ? `${(cents / 100).toFixed(1)}%` : formatCents(cents));

export const longDate = (iso) => LONG_DATE.format(new Date(`${iso}T00:00:00Z`));

/** One statement line: name left (indented by depth), amounts right, rules by line kind. */
export function StatementLine({ line, columns, onAccount }) {
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
      <td className={`py-0.5 pr-2 ${columns ? 'whitespace-nowrap' : ''} ${strong ? 'font-semibold' : ''} ${line.kind === 'net' ? 'text-md' : ''} ${drills ? 'hover:text-qb-info hover:underline' : ''}`} style={indent}>
        {line.label}
      </td>
      {(columns ? line.amountsCents : [line.amountCents]).map((cents, c) => (
        <td key={c} className={`${columns ? 'w-[120px]' : 'w-[160px]'} py-0.5 pr-2 align-bottom ${columns?.[c]?.isTotal || columns?.[c]?.kind === 'subtotal' ? 'bg-qb-canvas' : ''}`}>
          {cents !== null && cents !== undefined && (
            <div className={`num ${strong || columns?.[c]?.isTotal || columns?.[c]?.kind === 'subtotal' ? 'font-semibold' : ''} ${rule} ${line.kind === 'net' ? 'text-md' : ''}`}>
              {formatColumnAmount(columns?.[c], cents)}
            </div>
          )}
        </td>
      ))}
    </tr>
  );
}

/** An on-screen statement; `columns` (from Profit and Loss by Class) gives one amount column each. */
export function Statement({ title, subtitle, lines, footer, columns = null, onAccount = null }) {
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
        <StatementDocument company={company} title={title} subtitle={subtitle} lines={lines} footer={footer} columns={columns} />
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

/** One CSV document from rows of cells (UTF-8 with a byte-order mark so spreadsheets read symbols). */
export function toCsv(rows) {
  const quote = (v) => `"${String(v ?? '').replace(/"/g, '""')}"`;
  return `\ufeff${rows.map((r) => r.map(quote).join(',')).join('\r\n')}\r\n`;
}

/** Cents as a plain spreadsheet number: 123456 → 1234.56. */
export const plainAmount = (cents) => (cents === null || cents === undefined ? '' : (cents / 100).toFixed(2));

export async function saveText(text, filename, onNotice) {
  const path = await api.saveReportToDisk(text, filename);
  if (path) onNotice({ kind: 'success', text: `Saved ${path}` });
}
