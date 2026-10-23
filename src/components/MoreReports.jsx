import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { ExportMenu, PERIODS, Statement, longDate, plainAmount, resolvePeriod, saveText, toCsv } from './ReportParts.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/** Report period controls (preset, from, to), following the fiscal year like the statements do. */
function usePeriod(initial = 'fiscalYearToDate') {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [period, setPeriod] = useState(initial);
  const [[start, end], setRange] = useState(() => resolvePeriod(initial, null));
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
function TableReport({ title, subtitle, columns, rows, footer, toolbar, onCsvRows, error }) {
  const company = useCompany();
  const [notice, setNotice] = useState(null);
  const filename = `${title.replace(/[^A-Za-z0-9]+/g, '-')}.csv`;
  const csv = () => saveText(toCsv([[company.name], [title], [subtitle], [], columns.map((c) => c.label), ...onCsvRows()]), filename, setNotice);
  const rowClass = (kind) => (kind === 'section' ? 'font-semibold bg-qb-canvas' : kind === 'total' ? 'font-semibold border-t border-qb-ink' : kind === 'grand' ? 'font-semibold border-t-2 border-double border-qb-ink' : '');
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {toolbar}
        <div className="ml-auto">
          <ExportMenu onNotice={setNotice} onCsv={csv} />
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
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {p.controls}
        <div className="ml-auto">
          <ExportMenu onNotice={setNotice} onCsv={csv} />
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

/** A/R or A/P aging, as a summary by name or a detail list of open items. */
export function AgingReport({ payable, detail }) {
  const [asOf, setAsOf] = useState(todayIso());
  const { data, error, reload } = useLedgerQuery(() => (asOf ? api.getAging(payable, asOf) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  const side = payable ? 'A/P' : 'A/R';
  const title = detail ? (payable ? 'Unpaid Bills (A/P Aging Detail)' : 'Open Invoices (A/R Aging Detail)') : `${side} Aging Summary`;
  const labels = data?.bucketLabels ?? ['Current', '1 - 30', '31 - 60', '61 - 90', '91 and over'];

  if (!detail) {
    const rows = data
      ? [...data.rows.map((r) => ({ cells: [r.partyName, ...r.bucketsCents.map(money), formatCents(r.totalCents)] })), { kind: 'grand', cells: ['TOTAL', ...data.totalsCents.map(formatCents), formatCents(data.totalCents)] }]
      : [];
    return (
      <TableReport
        title={title}
        subtitle={data ? `As of ${longDate(data.asOf)}` : ''}
        columns={[{ label: payable ? 'Vendor' : 'Customer' }, ...labels.map((l) => ({ label: l, align: 'right' })), { label: 'Total', align: 'right' }]}
        rows={rows}
        error={error}
        toolbar={<AsOf value={asOf} onChange={setAsOf} />}
        footer={`Days past due · balances as of the report date · credit not yet used is negative`}
        onCsvRows={() => [...data.rows.map((r) => [r.partyName, ...r.bucketsCents.map(plainAmount), plainAmount(r.totalCents)]), ['TOTAL', ...data.totalsCents.map(plainAmount), plainAmount(data.totalCents)]]}
      />
    );
  }

  const rows = [];
  let current = null;
  let subtotal = 0;
  for (const item of data?.items ?? []) {
    if (item.partyName !== current) {
      if (current !== null) rows.push({ kind: 'total', cells: [`Total ${current}`, '', '', '', '', '', formatCents(subtotal)] });
      rows.push({ kind: 'section', span: 7, cells: [item.partyName] });
      current = item.partyName;
      subtotal = 0;
    }
    subtotal += item.openCents;
    rows.push({ cells: [item.kind, item.number ?? '', formatDate(item.date), formatDate(item.dueDate), item.daysPastDue > 0 ? item.daysPastDue : '', '', formatCents(item.openCents)] });
  }
  if (current !== null) rows.push({ kind: 'total', cells: [`Total ${current}`, '', '', '', '', '', formatCents(subtotal)] });
  if (data) rows.push({ kind: 'grand', cells: ['TOTAL', '', '', '', '', '', formatCents(data.totalCents)] });
  return (
    <TableReport
      title={title}
      subtitle={data ? `As of ${longDate(data.asOf)}` : ''}
      columns={[{ label: 'Type' }, { label: 'No.' }, { label: 'Date' }, { label: 'Due date' }, { label: 'Past due', align: 'right' }, { label: '' }, { label: 'Open balance', align: 'right' }]}
      rows={rows}
      error={error}
      toolbar={<AsOf value={asOf} onChange={setAsOf} />}
      footer="Balances as of the report date, after payments and credits dated on or before it"
      onCsvRows={() => (data?.items ?? []).map((i) => [i.partyName, i.kind, i.number ?? '', i.date, i.dueDate, i.daysPastDue > 0 ? i.daysPastDue : '', plainAmount(i.openCents)])}
    />
  );
}

const SUMMARY = {
  customer: { title: 'Sales by Customer Summary', column: 'Customer' },
  product: { title: 'Sales by Product/Service Summary', column: 'Product/Service' },
  vendor: { title: 'Expenses by Vendor Summary', column: 'Vendor' },
};

export function SummaryReport({ by }) {
  const p = usePeriod();
  const { data, error, reload } = useLedgerQuery(() => (p.valid ? api.getSummaryReport(by, p.start, p.end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [p.start, p.end, reload]);
  const cfg = SUMMARY[by];
  const withQty = by === 'product';
  const columns = [{ label: cfg.column }, ...(withQty ? [{ label: 'Quantity', align: 'right' }, { label: 'Avg price', align: 'right' }] : []), { label: 'Amount', align: 'right' }, { label: '% of total', align: 'right' }];
  const pct = (cents) => (data?.totalCents ? `${((cents / data.totalCents) * 100).toFixed(1)}%` : '');
  const rows = data
    ? [
        ...data.rows.map((r) => ({
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
      footer={by === 'product' ? 'Invoices and sales receipts less credit memos, by document date' : 'Accrual basis · posted transactions only'}
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
  const rows = [
    ...shown.map((r) => ({ cells: [r.vendorName, r.taxId ?? 'Missing', r.address ?? '', formatCents(r.paidCents)] })),
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
      footer="Payments by check, cash or bank transfer only: card and PayPal payments are reported by the processor on a 1099-K. Mark vendors for 1099 tracking in Expenses › Vendors."
      onCsvRows={() => [...shown.map((r) => [r.vendorName, r.taxId ?? '', r.address ?? '', plainAmount(r.paidCents)]), ['TOTAL', '', '', plainAmount(total)]]}
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

export function BudgetVsActualReport({ navigate }) {
  const budgets = useLedgerQuery(api.listBudgets);
  const [budgetId, setBudgetId] = useState(null);
  const [[from, to], setPeriods] = useState([1, 12]);
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
                  ['Budget vs. Actuals'],
                  [budget?.name ?? ''],
                  [],
                  ['Account', ...data.columns.map((c) => c.label)],
                  ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, ...l.amountsCents.map((a, i) => (data.columns[i]?.kind === 'percent' ? (a === null ? '' : (a / 100).toFixed(1)) : plainAmount(a)))]),
                ]),
                'Budget-vs-Actuals.csv',
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
          title="Budget vs. Actuals"
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
