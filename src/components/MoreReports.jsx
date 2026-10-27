import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { ExportMenu, PERIODS, Statement, longDate, plainAmount, resolvePeriod, saveText, saveXlsx, statementSheet, tableSheet, toCsv, xlsxName } from './ReportParts.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/**
 * Report period controls (preset, from, to), following the fiscal year like the statements do.
 * `range` ([start, end], from a graph's drill-down) starts on those dates instead of a preset.
 */
export function usePeriod(initial = 'fiscalYearToDate', range = null) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [period, setPeriod] = useState(range ? 'custom' : initial);
  const [[start, end], setRange] = useState(() => range ?? resolvePeriod(initial, null));
  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolvePeriod(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps
  const controls = (
    <>
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
    </>
  );
  return { start, end, controls, valid: Boolean(start && end && start <= end) };
}

function AsOf({ value, onChange }) {
  return (
    <label>
      <span className="qb-label">As of</span>
      <input type="date" className="qb-input w-[132px]" value={value} onChange={(e) => onChange(e.target.value)} />
    </label>
  );
}

const rangeText = (start, end) => (start === end ? longDate(start) : `${longDate(start)} – ${longDate(end)}`);

/**
 * A tabular report: on screen in a report frame, in the print root for Print / Save to PDF, and as
 * CSV. `columns` are { label, align }; `rows` are { cells, kind } where kind is `section`, `total`
 * or `grand` for styling.
 */
export function TableReport({ title, subtitle, columns, rows, footer, toolbar, onCsvRows, error }) {
  const company = useCompany();
  const [notice, setNotice] = useState(null);
  const filename = `${title.replace(/[^A-Za-z0-9]+/g, '-')}.csv`;
  const csv = () => saveText(toCsv([[company.name], [title], [subtitle], [], columns.map((c) => c.label), ...onCsvRows()]), filename, setNotice);
  const xlsx = () => saveXlsx(tableSheet({ company: company.name, title, subtitle, columns, rows: onCsvRows(), footer }), xlsxName(filename), setNotice);
  const rowClass = (kind) => (kind === 'section' ? 'font-semibold bg-qb-canvas' : kind === 'total' ? 'font-semibold border-t border-qb-ink' : kind === 'grand' ? 'font-semibold border-t-2 border-double border-qb-ink' : '');
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {toolbar}
        <div className="ml-auto">
          <ExportMenu onNotice={setNotice} onCsv={csv} onXlsx={xlsx} />
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <PrintPortal>
        <article className="doc">
          <DocHeader company={company} title={title} subtitle={subtitle} />
          <table className="doc-table">
            <thead>
              <tr>
                {columns.map((c) => (
                  <th key={c.label} className={c.align === 'right' ? 'doc-num' : ''}>{c.label}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i} className={r.kind === 'section' ? 'doc-line-section' : r.kind ? 'doc-line-total' : ''}>
                  {r.cells.map((cell, c) =>
                    r.span && c > 0 ? null : (
                      <td key={c} className={columns[c]?.align === 'right' ? 'doc-num' : ''} colSpan={r.span && c === 0 ? r.span : undefined}>{cell}</td>
                    ),
                  )}
                </tr>
              ))}
            </tbody>
          </table>
          {footer && <p className="doc-note">{footer}</p>}
        </article>
      </PrintPortal>
      <article className="mx-auto w-full max-w-[1240px] overflow-x-auto bg-white px-6 py-4 shadow-panel">
        <header className="mb-3 border-b-[3px] border-double border-qb-ink pb-2">
          <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
          <h2 className="text-xl font-semibold">{title}</h2>
          <div className="text-sm text-qb-ink-muted">{subtitle}</div>
        </header>
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-qb-border-strong">
              {columns.map((c) => (
                <th key={c.label} className={`px-1.5 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted ${c.align === 'right' ? 'text-right' : 'text-left'}`}>{c.label}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td colSpan={columns.length} className="px-1.5 py-1 text-qb-ink-muted">Nothing to report for this period.</td>
              </tr>
            )}
            {rows.map((r, i) => (
              <tr key={i} className={`${rowClass(r.kind)} ${r.onClick ? 'cursor-pointer hover:bg-qb-row-hover' : ''}`} onClick={r.onClick}>
                {r.cells.map((cell, c) =>
                  r.span && c > 0 ? null : (
                    <td key={c} colSpan={r.span && c === 0 ? r.span : undefined} className={`px-1.5 py-0.5 ${columns[c]?.align === 'right' ? 'num text-right' : ''}`}>
                      {cell}
                    </td>
                  ),
                )}
              </tr>
            ))}
          </tbody>
        </table>
        {footer && <footer className="mt-3 border-t border-qb-border pt-1 text-xs text-qb-ink-muted">{footer}</footer>}
      </article>
    </>
  );
}

const money = (cents) => (cents ? formatCents(cents) : '');

// ------------------------------------------------------------------------------------------------

export function CashFlowReport({ navigate }) {
  const p = usePeriod();
  const [notice, setNotice] = useState(null);
  const company = useCompany();
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getCashFlow(p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const csv = () =>
    saveText(
      toCsv([[company.name], ['Statement of Cash Flows'], [rangeText(p.start, p.end)], [], ['', 'Total'], ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, plainAmount(l.amountCents)])]),
      `Statement-of-Cash-Flows_${p.start}_to_${p.end}.csv`,
      setNotice,
    );
  const xlsx = () =>
    saveXlsx(
      statementSheet({ company: company.name, title: 'Statement of Cash Flows', subtitle: rangeText(p.start, p.end), lines: data.lines, footer: 'Indirect method · cash is the total of the Bank accounts' }),
      `Statement-of-Cash-Flows_${p.start}_to_${p.end}.xlsx`,
      setNotice,
    );
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {p.controls}
        <div className="ml-auto">
          <ExportMenu onNotice={setNotice} onCsv={csv} onXlsx={xlsx} />
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <Statement
          title="Statement of Cash Flows"
          subtitle={rangeText(data.startDate, data.endDate)}
          lines={data.lines}
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: data.startDate, endDate: data.endDate })}
          footer="Indirect method · cash is the total of the Bank accounts"
        />
      )}
    </>
  );
}

export function TrialBalanceReport({ navigate }) {
  const [asOf, setAsOf] = useState(todayIso());
  const { data, error, reload } = useLedgerQuery(() => (asOf ? api.getTrialBalanceAsOf(asOf) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  const rows = data
    ? [
        ...data.rows.map((r) => ({
          cells: [r.accountNumber ? `${r.accountNumber} · ${r.fullName}` : r.fullName, money(r.debitCents), money(r.creditCents)],
          onClick: () => navigate?.('accounting/register', 'open', { accountId: r.accountId, endDate: data.asOf }),
        })),
        { kind: 'grand', cells: ['TOTAL', formatCents(data.totalDebitCents), formatCents(data.totalCreditCents)] },
      ]
    : [];
  return (
    <TableReport
      title="Trial Balance"
      subtitle={data ? `As of ${longDate(data.asOf)}` : ''}
      columns={[{ label: 'Account' }, { label: 'Debit', align: 'right' }, { label: 'Credit', align: 'right' }]}
      rows={rows}
      error={error}
      toolbar={<AsOf value={asOf} onChange={setAsOf} />}
      footer={data ? `Income and expense accounts show the fiscal year beginning ${longDate(data.fiscalYearStart)}; earlier years are in Retained Earnings.` : null}
      onCsvRows={() => [...data.rows.map((r) => [r.accountNumber ? `${r.accountNumber} ${r.fullName}` : r.fullName, plainAmount(r.debitCents || null), plainAmount(r.creditCents || null)]), ['TOTAL', plainAmount(data.totalDebitCents), plainAmount(data.totalCreditCents)]]}
    />
  );
}

export function GeneralLedgerReport({ navigate }) {
  const p = usePeriod('thisMonth');
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getGeneralLedger(p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const rows = [];
  for (const a of data?.accounts ?? []) {
    rows.push({ kind: 'section', span: 9, cells: [a.accountName], onClick: () => navigate?.('accounting/register', 'open', { accountId: a.accountId, startDate: data.startDate, endDate: data.endDate }) });
    rows.push({ cells: ['', '', '', 'Beginning balance', '', '', '', '', formatCents(a.openingBalanceCents)] });
    for (const r of a.rows) {
      rows.push({
        cells: [formatDate(r.entryDate), r.isReversal ? 'Void' : (SOURCE_LABEL[r.sourceType] ?? r.sourceType), r.entryNumber, r.partyName ?? '', r.memo ?? '', r.split ?? '', money(r.debitCents), money(r.creditCents), formatCents(r.balanceCents)],
      });
    }
    rows.push({ kind: 'total', cells: [`Total ${a.accountName}`, '', '', '', '', '', formatCents(a.totalDebitCents), formatCents(a.totalCreditCents), formatCents(a.endingBalanceCents)] });
  }
  return (
    <TableReport
      title="General Ledger"
      subtitle={data ? rangeText(data.startDate, data.endDate) : ''}
      columns={[
        { label: 'Date' },
        { label: 'Type' },
        { label: 'No.' },
        { label: 'Name' },
        { label: 'Memo' },
        { label: 'Split' },
        { label: 'Debit', align: 'right' },
        { label: 'Credit', align: 'right' },
        { label: 'Balance', align: 'right' },
      ]}
      rows={rows}
      error={error}
      toolbar={p.controls}
      footer="Balances on each account's normal side · income and expense accounts begin at their fiscal-year-to-date amount"
      onCsvRows={() =>
        (data?.accounts ?? []).flatMap((a) => [
          [a.accountName],
          ['', '', '', 'Beginning balance', '', '', '', '', plainAmount(a.openingBalanceCents)],
          ...a.rows.map((r) => [r.entryDate, SOURCE_LABEL[r.sourceType] ?? r.sourceType, r.entryNumber, r.partyName ?? '', r.memo ?? '', r.split ?? '', plainAmount(r.debitCents || null), plainAmount(r.creditCents || null), plainAmount(r.balanceCents)]),
          [`Total ${a.accountName}`, '', '', '', '', '', plainAmount(a.totalDebitCents), plainAmount(a.totalCreditCents), plainAmount(a.endingBalanceCents)],
        ])
      }
    />
  );
}

/** A/R or A/P aging by name. The detail lists (Open Invoices, Unpaid Bills, the aging details) are custom report presets. */
export function AgingReport({ payable, initial = null }) {
  const [asOf, setAsOf] = useState(initial?.asOf ?? todayIso());
  const { data, error, reload } = useLedgerQuery(() => (asOf ? api.getAging(payable, asOf) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  const title = `${payable ? 'A/P' : 'A/R'} Aging Summary`;
  const labels = data?.bucketLabels ?? ['Current', '1 - 30', '31 - 60', '61 - 90', '91 and over'];
  // QuickZoom: clicking a name lists its open documents under it, each in its aging column.
  const [opened, setOpened] = useState(() => new Set());
  const toggle = (name) =>
    setOpened((s) => {
      const next = new Set(s);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  const itemRows = (name) =>
    data.items
      .filter((it) => it.partyName === name)
      .map((it) => ({
        cells: [
          <span key="doc" className="pl-4 text-qb-ink-muted">
            {it.kind}
            {it.number ? ` #${it.number}` : ''} · {formatDate(it.date)}
            {it.daysPastDue > 0 ? ` · ${it.daysPastDue} day${it.daysPastDue === 1 ? '' : 's'} past due` : ''}
          </span>,
          ...[0, 1, 2, 3, 4].map((b) => (agingBucket(it.daysPastDue) === b ? formatCents(it.openCents) : '')),
          formatCents(it.openCents),
        ],
      }));
  const rows = data
    ? [
        ...data.rows.flatMap((r) => [
          {
            cells: [
              <span key="name" title={opened.has(r.partyName) ? 'Hide the open documents' : 'Show the open documents'}>
                <span aria-hidden="true" className="inline-block w-3 text-2xs text-qb-ink-muted">{opened.has(r.partyName) ? '▾' : '▸'}</span>
                {r.partyName}
              </span>,
              ...r.bucketsCents.map(money),
              formatCents(r.totalCents),
            ],
            onClick: () => toggle(r.partyName),
          },
          ...(opened.has(r.partyName) ? itemRows(r.partyName) : []),
        ]),
        { kind: 'grand', cells: ['TOTAL', ...data.totalsCents.map(formatCents), formatCents(data.totalCents)] },
      ]
    : [];
  return (
    <TableReport
      title={title}
      subtitle={data ? `As of ${longDate(data.asOf)}` : ''}
      columns={[{ label: payable ? 'Vendor' : 'Customer' }, ...labels.map((l) => ({ label: l, align: 'right' })), { label: 'Total', align: 'right' }]}
      rows={rows}
      error={error}
      toolbar={<AsOf value={asOf} onChange={setAsOf} />}
      footer={`Days past due · balances as of the report date · credit not yet used is negative · click a name for its open documents`}
      onCsvRows={() => [...data.rows.map((r) => [r.partyName, ...r.bucketsCents.map(plainAmount), plainAmount(r.totalCents)]), ['TOTAL', ...data.totalsCents.map(plainAmount), plainAmount(data.totalCents)]]}
    />
  );
}

/** The aging column for an item this many days past due, as `report_center::aging_bucket` sorts them. */
const agingBucket = (days) => (days <= 0 ? 0 : days <= 30 ? 1 : days <= 60 ? 2 : days <= 90 ? 3 : 4);

// `zoom`: the detail report a row opens, and how to find what the row is (by name, from a list).
const SUMMARY = {
  customer: { title: 'Sales by Customer Summary', column: 'Customer', zoom: { preset: 'income-customer-detail', list: () => api.listCustomers(), filter: (id) => ({ names: [{ kind: 'customer', id }] }) } },
  product: { title: 'Sales by Product/Service Summary', column: 'Product/Service', zoom: { preset: 'sales-product-detail', list: () => api.listProducts(), filter: (id) => ({ itemIds: [id] }) } },
  rep: { title: 'Sales by Rep Summary', column: 'Rep' },
  vendor: { title: 'Expenses by Vendor Summary', column: 'Vendor', zoom: { preset: 'expenses-vendor-detail', list: () => api.listVendors(), filter: (id) => ({ names: [{ kind: 'vendor', id }] }) } },
};

export function SummaryReport({ by, initial = null, onZoom = null }) {
  const p = usePeriod('fiscalYearToDate', initial?.startDate ? [initial.startDate, initial.endDate] : null);
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getSummaryReport(by, p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const cfg = SUMMARY[by];
  const zooms = onZoom && cfg.zoom;
  const { data: named } = useLedgerQuery(() => (zooms ? cfg.zoom.list() : Promise.resolve([])));
  const zoomTo = (label) => {
    const match = (named ?? []).find((n) => n.name === label);
    return match ? () => onZoom(cfg.zoom.preset, { title: `${cfg.title.replace(' Summary', ' Detail')}: ${label}`, filters: cfg.zoom.filter(match.id), startDate: data.startDate, endDate: data.endDate }) : undefined;
  };
  const withQty = by === 'product';
  const columns = [{ label: cfg.column }, ...(withQty ? [{ label: 'Quantity', align: 'right' }, { label: 'Avg price', align: 'right' }] : []), { label: 'Amount', align: 'right' }, { label: '% of total', align: 'right' }];
  const pct = (cents) => (data?.totalCents ? `${((cents / data.totalCents) * 100).toFixed(1)}%` : '');
  const rows = data
    ? [
        ...data.rows.map((r) => ({
          onClick: zooms ? zoomTo(r.label) : undefined,
          cells: [
            r.label,
            ...(withQty ? [formatQuantity(r.quantityE4), r.quantityE4 ? formatCents(Math.round((r.amountCents * 10_000) / r.quantityE4)) : ''] : []),
            formatCents(r.amountCents),
            pct(r.amountCents),
          ],
        })),
        { kind: 'grand', cells: ['TOTAL', ...(withQty ? ['', ''] : []), formatCents(data.totalCents), data.totalCents ? '100.0%' : ''] },
      ]
    : [];
  return (
    <TableReport
      title={cfg.title}
      subtitle={data ? rangeText(data.startDate, data.endDate) : ''}
      columns={columns}
      rows={rows}
      error={error}
      toolbar={p.controls}
      footer={
        (by === 'product'
          ? 'Invoices and sales receipts less credit memos, by document date'
          : by === 'rep'
            ? 'Invoices and sales receipts by the rep on each form, by document date (credit memos carry no rep)'
            : 'Accrual basis · posted transactions only') + (zooms ? ' · click a line for its transactions' : '')
      }
      onCsvRows={() => [...data.rows.map((r) => [r.label, ...(withQty ? [((r.quantityE4 ?? 0) / 10_000).toString(), ''] : []), plainAmount(r.amountCents), '']), ['TOTAL', ...(withQty ? ['', ''] : []), plainAmount(data.totalCents), '']]}
    />
  );
}

export function SalesTaxLiabilityReport() {
  const p = usePeriod('thisQuarter');
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getTaxLiability(p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const pct = (ppm) => (ppm == null ? '' : `${(ppm / 10_000).toString()}%`);
  const rows = data
    ? [
        ...data.rows.map((r) => ({ cells: [r.name, pct(r.ratePpm), formatCents(r.taxableSalesCents), formatCents(r.nonTaxableSalesCents), formatCents(r.taxCollectedCents)] })),
        { kind: 'grand', cells: ['TOTAL', '', formatCents(data.rows.reduce((s, r) => s + r.taxableSalesCents, 0)), formatCents(data.rows.reduce((s, r) => s + r.nonTaxableSalesCents, 0)), formatCents(data.taxCollectedCents)] },
        { kind: 'total', cells: ['Paid in the period', '', '', '', formatCents(data.paidCents)] },
        { kind: 'total', cells: [`Owed as of ${longDate(data.endDate)}`, '', '', '', formatCents(data.owedCents)] },
      ]
    : [];
  return (
    <TableReport
      title="Sales Tax Liability"
      subtitle={data ? rangeText(data.startDate, data.endDate) : ''}
      columns={[{ label: 'Tax rate' }, { label: 'Rate', align: 'right' }, { label: 'Taxable sales', align: 'right' }, { label: 'Non-taxable sales', align: 'right' }, { label: 'Tax', align: 'right' }]}
      rows={rows}
      error={error}
      toolbar={p.controls}
      footer="Invoices and sales receipts less credit memos, by document date · record payments under Sales › Sales Tax"
      onCsvRows={() => [
        ...data.rows.map((r) => [r.name, pct(r.ratePpm), plainAmount(r.taxableSalesCents), plainAmount(r.nonTaxableSalesCents), plainAmount(r.taxCollectedCents)]),
        ['Paid in the period', '', '', '', plainAmount(data.paidCents)],
        ['Owed at end of period', '', '', '', plainAmount(data.owedCents)],
      ]}
    />
  );
}

export function Vendors1099Report() {
  const thisYear = Number(todayIso().slice(0, 4));
  const [year, setYear] = useState(thisYear - 1);
  const [all, setAll] = useState(false);
  const { data, error, reload } = useLedgerQuery(() => api.get1099Report(year));
  useEffect(() => {
    reload();
  }, [year, reload]);
  const shown = (data ?? []).filter((r) => all || r.overThreshold);
  const total = shown.reduce((s, r) => s + r.paidCents, 0);
  const columns = [{ label: 'Vendor' }, { label: 'Tax ID' }, { label: 'Address' }, { label: 'Paid', align: 'right' }];
  // Tax IDs are masked on screen and in print; the Excel and CSV exports carry them in full for users who may see them.
  const fullInCsv = shown.some((r) => r.taxId);
  const rows = [
    ...shown.map((r) => ({ cells: [r.vendorName, r.taxIdMasked ?? 'Missing', r.address ?? '', formatCents(r.paidCents)] })),
    ...(shown.length ? [{ kind: 'grand', cells: ['TOTAL', '', '', formatCents(total)] }] : []),
  ];
  return (
    <TableReport
      title="1099 Summary"
      subtitle={`Payments in ${year} to vendors marked “Track payments for a 1099”`}
      columns={columns}
      rows={rows}
      error={error}
      toolbar={
        <>
          <label>
            <span className="qb-label">Calendar year</span>
            <select className="qb-input w-[100px]" value={year} onChange={(e) => setYear(Number(e.target.value))}>
              {Array.from({ length: 6 }, (_, i) => thisYear - i).map((y) => (
                <option key={y} value={y}>{y}</option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-1 self-center text-sm">
            <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
            Include vendors under the $600 threshold
          </label>
        </>
      }
      footer={`Payments by check, cash or bank transfer only: card and PayPal payments are reported by the processor on a 1099-K. Mark vendors for 1099 tracking in Expenses › Vendors. Tax IDs are masked here${
        fullInCsv ? '; the Excel and CSV exports have them in full, so keep those files safe' : ''
      }.`}
      onCsvRows={() => [...shown.map((r) => [r.vendorName, r.taxId ?? r.taxIdMasked ?? '', r.address ?? '', plainAmount(r.paidCents)]), ['TOTAL', '', '', plainAmount(total)]]}
    />
  );
}

const MONTH_NAMES = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

/** The name of fiscal month `period` (1-12) of a fiscal year starting on `start`. */
export function fiscalMonthLabel(start, period) {
  const m = (Number(start.slice(5, 7)) - 1 + period - 1) % 12;
  const y = Number(start.slice(0, 4)) + Math.floor((Number(start.slice(5, 7)) - 1 + period - 1) / 12);
  return `${MONTH_NAMES[m].slice(0, 3)} ${y}`;
}

export function BudgetVsActualReport({ navigate, initial = null }) {
  const budgets = useLedgerQuery(api.listBudgets);
  const [budgetId, setBudgetId] = useState(initial?.budgetId ?? null);
  const [[from, to], setPeriods] = useState(initial?.fromPeriod ? [initial.fromPeriod, initial.toPeriod] : [1, 12]);
  const list = budgets.data ?? [];
  const budget = list.find((b) => b.id === budgetId) ?? list[0];
  const { data, error, reload } = useLedgerQuery(() => (budget ? api.getBudgetVsActual(budget.id, from, to) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [budget?.id, from, to, reload]);
  const [notice, setNotice] = useState(null);
  const company = useCompany();
  if (budgets.data && list.length === 0) {
    return <Notice kind="info" text="There are no budgets yet. Create one under Accounting › Budgets, then come back to compare it with actual income and expenses." />;
  }
  const months = Array.from({ length: 12 }, (_, i) => i + 1);
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Budget</span>
          <select className="qb-input w-[220px]" value={budget?.id ?? ''} onChange={(e) => setBudgetId(Number(e.target.value))}>
            {list.map((b) => (
              <option key={b.id} value={b.id}>{b.name} (FY from {formatDate(b.fiscalYearStart)})</option>
            ))}
          </select>
        </label>
        {budget && (
          <>
            <label>
              <span className="qb-label">From month</span>
              <select className="qb-input w-[110px]" value={from} onChange={(e) => setPeriods([Number(e.target.value), Math.max(Number(e.target.value), to)])}>
                {months.map((m) => (
                  <option key={m} value={m}>{fiscalMonthLabel(budget.fiscalYearStart, m)}</option>
                ))}
              </select>
            </label>
            <label>
              <span className="qb-label">To month</span>
              <select className="qb-input w-[110px]" value={to} onChange={(e) => setPeriods([Math.min(from, Number(e.target.value)), Number(e.target.value)])}>
                {months.map((m) => (
                  <option key={m} value={m}>{fiscalMonthLabel(budget.fiscalYearStart, m)}</option>
                ))}
              </select>
            </label>
          </>
        )}
        <div className="ml-auto">
          <ExportMenu
            onNotice={setNotice}
            onCsv={() =>
              saveText(
                toCsv([
                  [company.name],
                  ['Profit and Loss Budget vs. Actual'],
                  [budget?.name ?? ''],
                  [],
                  ['Account', ...data.columns.map((c) => c.label)],
                  ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, ...l.amountsCents.map((a, i) => (data.columns[i]?.kind === 'percent' ? (a === null ? '' : (a / 100).toFixed(1)) : plainAmount(a)))]),
                ]),
                'Profit-and-Loss-Budget-vs-Actual.csv',
                setNotice,
              )
            }
            onXlsx={() =>
              saveXlsx(
                statementSheet({ company: company.name, title: 'Profit and Loss Budget vs. Actual', subtitle: `${budget.name} · ${rangeText(data.startDate, data.endDate)}`, lines: data.lines, columns: data.columns }),
                'Profit-and-Loss-Budget-vs-Actual.xlsx',
                setNotice,
              )
            }
          />
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <Statement
          title="Profit and Loss Budget vs. Actual"
          subtitle={`${budget.name} · ${rangeText(data.startDate, data.endDate)}`}
          lines={data.lines}
          columns={data.columns}
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: data.startDate, endDate: data.endDate, includeSubaccounts: line.kind === 'subtotal' })}
          footer="Accrual basis · posted transactions only · Over budget is actual less budget"
        />
      )}
    </>
  );
}

/** CSV rows of a multi-column statement (percent columns as plain numbers: 12.5% → 12.5). */
const columnCsvRows = (data) => [
  ['Account', ...data.columns.map((c) => c.label)],
  ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, ...l.amountsCents.map((a, i) => (data.columns[i]?.kind === 'percent' ? (a === null ? '' : (a / 100).toFixed(1)) : plainAmount(a)))]),
];

export function BalanceSheetBudgetVsActualReport({ navigate }) {
  const budgets = useLedgerQuery(api.listBudgets);
  const [budgetId, setBudgetId] = useState(null);
  const [period, setPeriod] = useState(12);
  const list = budgets.data ?? [];
  const budget = list.find((b) => b.id === budgetId) ?? list[0];
  const { data, error, reload } = useLedgerQuery(() => (budget ? api.getBalanceSheetBudgetVsActual(budget.id, period) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [budget?.id, period, reload]);
  const [notice, setNotice] = useState(null);
  const company = useCompany();
  if (budgets.data && list.length === 0) {
    return <Notice kind="info" text="There are no budgets yet. Create one under Accounting › Budgets, choose Balance Sheet accounts, and enter the balances you expect at each month end." />;
  }
  const title = 'Balance Sheet Budget vs. Actual';
  const subtitle = data ? `${budget.name} · As of ${longDate(data.endDate)}` : '';
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Budget</span>
          <select className="qb-input w-[220px]" value={budget?.id ?? ''} onChange={(e) => setBudgetId(Number(e.target.value))}>
            {list.map((b) => (
              <option key={b.id} value={b.id}>{b.name} (FY from {formatDate(b.fiscalYearStart)})</option>
            ))}
          </select>
        </label>
        {budget && (
          <label>
            <span className="qb-label">As of the end of</span>
            <select className="qb-input w-[110px]" value={period} onChange={(e) => setPeriod(Number(e.target.value))}>
              {Array.from({ length: 12 }, (_, i) => i + 1).map((m) => (
                <option key={m} value={m}>{fiscalMonthLabel(budget.fiscalYearStart, m)}</option>
              ))}
            </select>
          </label>
        )}
        <div className="ml-auto">
          <ExportMenu
            onNotice={setNotice}
            onCsv={() => saveText(toCsv([[company.name], [title], [subtitle], [], ...columnCsvRows(data)]), `Balance-Sheet-Budget-vs-Actual_${data.endDate}.csv`, setNotice)}
            onXlsx={() => saveXlsx(statementSheet({ company: company.name, title, subtitle, lines: data.lines, columns: data.columns }), `Balance-Sheet-Budget-vs-Actual_${data.endDate}.xlsx`, setNotice)}
          />
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <Statement
          title={title}
          subtitle={subtitle}
          lines={data.lines}
          columns={data.columns}
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: null, endDate: data.endDate, includeSubaccounts: line.kind === 'subtotal' })}
          footer="Budget: the month-end balances entered for balance sheet accounts; budgeted Net Income is the budget's income less expenses for the year to date · Over budget is actual less budget"
        />
      )}
    </>
  );
}

const pctOf = (part, whole) => (whole ? `${((part / whole) * 100).toFixed(1)}%` : '');

export function InventoryValuationReport() {
  const [asOf, setAsOf] = useState(todayIso());
  const { data, error, reload } = useLedgerQuery(() => (asOf ? api.getInventoryValuation(asOf) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  const rows = data
    ? [
        ...data.rows.map((r) => ({
          cells: [
            r.name,
            r.description ?? '',
            formatQuantity(r.qtyOnHandE4),
            r.averageCostCents == null ? '' : formatCents(r.averageCostCents),
            formatCents(r.assetValueCents),
            pctOf(r.assetValueCents, data.totalAssetValueCents),
            formatCents(r.salesPriceCents),
            formatCents(r.retailValueCents),
            pctOf(r.retailValueCents, data.totalRetailValueCents),
          ],
        })),
        { kind: 'grand', cells: ['TOTAL', '', '', '', formatCents(data.totalAssetValueCents), data.totalAssetValueCents ? '100.0%' : '', '', formatCents(data.totalRetailValueCents), data.totalRetailValueCents ? '100.0%' : ''] },
      ]
    : [];
  return (
    <TableReport
      title="Inventory Valuation Summary"
      subtitle={data ? `As of ${longDate(data.asOf)}` : ''}
      columns={[
        { label: 'Item' },
        { label: 'Description' },
        { label: 'On hand', align: 'right' },
        { label: 'Avg cost', align: 'right' },
        { label: 'Asset value', align: 'right' },
        { label: '% of tot asset', align: 'right' },
        { label: 'Sales price', align: 'right' },
        { label: 'Retail value', align: 'right' },
        { label: '% of tot retail', align: 'right' },
      ]}
      rows={rows}
      error={error}
      toolbar={<AsOf value={asOf} onChange={setAsOf} />}
      footer="Moving average cost · stock movements dated on or before the report date, so the total agrees with the inventory asset accounts · track products under Sales › Products & Services"
      onCsvRows={() => [
        ...data.rows.map((r) => [r.name, r.description ?? '', (r.qtyOnHandE4 / 10_000).toString(), plainAmount(r.averageCostCents), plainAmount(r.assetValueCents), '', plainAmount(r.salesPriceCents), plainAmount(r.retailValueCents), '']),
        ['TOTAL', '', '', '', plainAmount(data.totalAssetValueCents), '', '', plainAmount(data.totalRetailValueCents), ''],
      ]}
    />
  );
}

const MOVEMENT_LABEL = { PURCHASE: 'Purchase', SALE: 'Sale', ADJUSTMENT: 'Adjustment', COST_TRUE_UP: 'Cost true-up', VOID: 'Void' };

export function InventoryValuationDetailReport({ navigate }) {
  const p = usePeriod('thisMonth');
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getInventoryValuationDetail(p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const type = (r) => (r.kind === 'VOID' ? 'Void' : (SOURCE_LABEL[r.sourceType] ?? MOVEMENT_LABEL[r.kind] ?? r.kind));
  const rows = [];
  for (const item of data?.items ?? []) {
    rows.push({ kind: 'section', span: 9, cells: [item.name] });
    rows.push({ cells: ['', '', '', 'Beginning balance', '', '', formatQuantity(item.openingQtyE4), '', formatCents(item.openingValueCents)] });
    for (const r of item.rows) {
      rows.push({
        cells: [
          formatDate(r.movementDate),
          type(r),
          r.entryNumber,
          r.partyName ?? '',
          formatQuantity(r.qtyChangeE4),
          formatCents(r.valueChangeCents),
          formatQuantity(r.qtyOnHandE4),
          r.averageCostCents == null ? '' : formatCents(r.averageCostCents),
          formatCents(r.assetValueCents),
        ],
        onClick: () => navigate?.('accounting/journal', 'open-entry', { entryId: r.journalEntryId }),
      });
    }
    rows.push({ kind: 'total', cells: [`Total ${item.name}`, '', '', '', '', '', formatQuantity(item.closingQtyE4), '', formatCents(item.closingValueCents)] });
  }
  if (data) rows.push({ kind: 'grand', cells: ['TOTAL', '', '', '', '', '', '', '', formatCents(data.totalValueCents)] });
  return (
    <TableReport
      title="Inventory Valuation Detail"
      subtitle={data ? rangeText(data.startDate, data.endDate) : ''}
      columns={[
        { label: 'Date' },
        { label: 'Type' },
        { label: 'No.' },
        { label: 'Name' },
        { label: 'Qty', align: 'right' },
        { label: 'Cost', align: 'right' },
        { label: 'On hand', align: 'right' },
        { label: 'Avg cost', align: 'right' },
        { label: 'Asset value', align: 'right' },
      ]}
      rows={rows}
      error={error}
      toolbar={p.controls}
      footer="Every stock movement at moving average cost · No. is the journal entry number · a void reverses the movement it cancels"
      onCsvRows={() =>
        (data?.items ?? []).flatMap((item) => [
          [item.name],
          ['', '', '', 'Beginning balance', '', '', (item.openingQtyE4 / 10_000).toString(), '', plainAmount(item.openingValueCents)],
          ...item.rows.map((r) => [r.movementDate, type(r), r.entryNumber, r.partyName ?? '', (r.qtyChangeE4 / 10_000).toString(), plainAmount(r.valueChangeCents), (r.qtyOnHandE4 / 10_000).toString(), plainAmount(r.averageCostCents), plainAmount(r.assetValueCents)]),
          [`Total ${item.name}`, '', '', '', '', '', (item.closingQtyE4 / 10_000).toString(), '', plainAmount(item.closingValueCents)],
        ])
      }
    />
  );
}

export function StockStatusReport() {
  const p = usePeriod('fiscalYearToDate');
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getStockStatus(p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const rows = (data?.rows ?? []).map((r) => ({
    cells: [
      r.name,
      r.sku ?? '',
      r.description ?? '',
      formatQuantity(r.qtyOnHandE4),
      r.qtyOnOrderE4 ? formatQuantity(r.qtyOnOrderE4) : '',
      r.nextDelivery ? formatDate(r.nextDelivery) : '',
      formatQuantity(r.qtySoldE4),
      formatQuantity(r.salesPerWeekE4),
    ],
  }));
  return (
    <TableReport
      title="Inventory Stock Status by Item"
      subtitle={data ? rangeText(data.startDate, data.endDate) : ''}
      columns={[
        { label: 'Item' },
        { label: 'SKU' },
        { label: 'Description' },
        { label: 'On hand', align: 'right' },
        { label: 'On PO', align: 'right' },
        { label: 'Next deliv', align: 'right' },
        { label: 'Sold', align: 'right' },
        { label: 'Sales/week', align: 'right' },
      ]}
      rows={rows}
      error={error}
      toolbar={p.controls}
      footer="On hand at the end of the period · On PO: open purchase orders (Next deliv is the earliest expected date) · Sold and Sales/week: units sold in the period, less voided sales"
      onCsvRows={() =>
        data.rows.map((r) => [r.name, r.sku ?? '', r.description ?? '', (r.qtyOnHandE4 / 10_000).toString(), (r.qtyOnOrderE4 / 10_000).toString(), r.nextDelivery ?? '', (r.qtySoldE4 / 10_000).toString(), (r.salesPerWeekE4 / 10_000).toString()])
      }
    />
  );
}

export function MissingChecksReport({ navigate }) {
  const accounts = useLedgerQuery(api.listAccounts);
  const banks = (accounts.data ?? []).filter((a) => a.accountType === 'BANK' && a.isActive);
  const [accountId, setAccountId] = useState(null);
  const account = banks.find((a) => a.id === accountId) ?? banks[0];
  const [range, setRange] = useState(['', '']);
  const [from, to] = range;
  const { data, error, reload } = useLedgerQuery(() => (account ? api.getMissingChecks(account.id, from, to) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [account?.id, from, to, reload]);
  if (accounts.data && banks.length === 0) {
    return <Notice kind="info" text="There are no bank accounts yet. Missing Checks lists the checks written from a bank account." />;
  }
  const rows = [];
  (data?.rows ?? []).forEach((r, i) => {
    for (const gap of data.gaps.filter((g) => g.before === i)) {
      const span = gap.first === gap.last ? `${gap.first}` : `${gap.first} – ${gap.last}`;
      rows.push({ kind: 'section', span: 7, cells: [`*** Missing number${gap.first === gap.last ? '' : 's'} ${span} ***`] });
    }
    rows.push({
      cells: [r.number, formatDate(r.date), r.kind, r.partyName ?? '', r.isDuplicate ? '*** Duplicate number ***' : r.isVoided ? 'VOID' : (r.memo ?? ''), r.isVoided ? '' : formatCents(r.amountCents), r.isVoided ? 'Void' : ''],
      onClick: r.journalEntryId ? () => navigate?.('accounting/journal', 'open-entry', { entryId: r.journalEntryId }) : undefined,
    });
  });
  const summary = data
    ? `${data.rows.length} numbered check${data.rows.length === 1 ? '' : 's'} · ${data.missingCount} missing · ${data.duplicateCount} with a duplicate number${data.unnumberedCount ? ` · ${data.unnumberedCount} payment${data.unnumberedCount === 1 ? '' : 's'} with a reference that is not a check number` : ''}`
    : '';
  return (
    <TableReport
      title="Missing Checks"
      subtitle={data ? `${data.accountName}${from || to ? ` · ${from ? longDate(from) : 'start'} – ${to ? longDate(to) : 'today'}` : ''}` : ''}
      columns={[{ label: 'Num' }, { label: 'Date' }, { label: 'Type' }, { label: 'Name' }, { label: 'Memo' }, { label: 'Amount', align: 'right' }, { label: '' }]}
      rows={rows}
      error={error}
      toolbar={
        <>
          <label>
            <span className="qb-label">Bank account</span>
            <select className="qb-input w-[200px]" value={account?.id ?? ''} onChange={(e) => setAccountId(Number(e.target.value))}>
              {banks.map((a) => (
                <option key={a.id} value={a.id}>{a.accountNumber ? `${a.accountNumber} · ` : ''}{a.fullName}</option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">From (optional)</span>
            <input type="date" className="qb-input w-[132px]" value={from} onChange={(e) => setRange([e.target.value, to])} />
          </label>
          <label>
            <span className="qb-label">To (optional)</span>
            <input type="date" className="qb-input w-[132px]" value={to} onChange={(e) => setRange([from, e.target.value])} aria-invalid={Boolean(from && to && to < from)} />
          </label>
          {data && <span className="self-center text-sm text-qb-ink-muted">{summary}</span>}
        </>
      }
      footer="Checks, bill payments, sales tax payments and refunds from the account with a numeric check number, in number order · voided checks keep their number · one check paying several bills of a vendor counts once"
      onCsvRows={() => [
        ...data.rows.flatMap((r, i) => [
          ...data.gaps.filter((g) => g.before === i).map((g) => [`Missing ${g.first === g.last ? g.first : `${g.first} - ${g.last}`}`]),
          [r.number, r.date, r.kind, r.partyName ?? '', r.isDuplicate ? 'Duplicate number' : (r.memo ?? ''), r.isVoided ? '' : plainAmount(r.amountCents), r.isVoided ? 'Void' : ''],
        ]),
        [],
        [summary],
      ]}
    />
  );
}
