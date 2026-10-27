import { useContext, useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { classLabel, useClasses } from './ClassSelect.jsx';
import { ExportMenu, PERIODS, Statement, longDate, plainAmount, resolvePeriod, saveCsv, saveReportXlsx, saveText, saveXlsx, statementSheet, toCsv } from './ReportParts.jsx';
import { useCompany } from './PrintDocuments.jsx';
import CustomReport, { isCustomReport, useCustomReports, withCustomReports } from './CustomReport.jsx';
import JobReport, { JOB_REPORTS, isJobReport } from './JobReports.jsx';
import AccountantReports, { ACCOUNTANT_REPORTS, isAccountantReport } from './AccountantReports.jsx';
import GraphReport, { GRAPH_REPORTS, isGraphReport } from './Graphs.jsx';
import {
  AgingReport,
  BalanceSheetBudgetVsActualReport,
  BudgetVsActualReport,
  CashFlowReport,
  GeneralLedgerReport,
  InventoryValuationDetailReport,
  InventoryValuationReport,
  MissingChecksReport,
  SalesTaxLiabilityReport,
  StockStatusReport,
  SummaryReport,
  TrialBalanceReport,
  Vendors1099Report,
} from './MoreReports.jsx';
import { ItemPriceListReport } from './PriceLevels.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, todayIso } from '../lib/dates.js';
import { useOpenFromList, useWindowContent, WindowManagerContext } from '../lib/windows.js';

const iso = (y, m, d) => `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
const lastDay = (y, m) => new Date(Date.UTC(y, m, 0)).getUTCDate();
const MONTH_LABEL = new Intl.DateTimeFormat('en-US', { month: 'short', year: 'numeric', timeZone: 'UTC' });

/** The range split into calendar months or quarters (the first and last clipped to the range). */
export function splitPeriods(start, end, mode) {
  const columns = [];
  let [y, m] = start.split('-').map(Number);
  while (iso(y, m, 1) <= end) {
    const label = mode === 'quarters' ? `Q${Math.floor((m - 1) / 3) + 1} ${y}` : MONTH_LABEL.format(new Date(Date.UTC(y, m - 1, 1)));
    const from = iso(y, m, 1) < start ? start : iso(y, m, 1);
    const to = iso(y, m, lastDay(y, m)) > end ? end : iso(y, m, lastDay(y, m));
    const last = columns[columns.length - 1];
    if (last && last.label === label) last.endDate = to;
    else columns.push({ label, startDate: from, endDate: to });
    m += 1;
    if (m > 12) {
      m = 1;
      y += 1;
    }
  }
  return columns;
}

export const shiftYear = (date) => {
  const [y, m, d] = date.split('-').map(Number);
  return iso(y - 1, m, Math.min(d, lastDay(y - 1, m)));
};

/** The comparison period: the same length immediately before, or the same dates a year earlier. */
export function priorRange(start, end, mode) {
  if (mode === 'year') return [shiftYear(start), shiftYear(end)];
  const days = Math.round((Date.parse(end) - Date.parse(start)) / 86_400_000);
  const to = addDays(start, -1);
  return [addDays(to, -days), to];
}

const RANGE_LABEL = new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' });
const shortRange = (from, to) => `${RANGE_LABEL.format(new Date(`${from}T00:00:00Z`))} – ${RANGE_LABEL.format(new Date(`${to}T00:00:00Z`))}`;

function ProfitAndLoss({ byClass = false, initialCompare = 'none', title = null, navigate, initial = null }) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [period, setPeriod] = useState(initial?.startDate ? 'custom' : 'fiscalYearToDate');
  const [[start, end], setRange] = useState(() => (initial?.startDate ? [initial.startDate, initial.endDate] : resolvePeriod('fiscalYearToDate', null)));
  const [notice, setNotice] = useState(null);
  const classes = useClasses();
  // '' = all lines, 'none' = lines without a class, otherwise a class id (with its subclasses).
  const [classChoice, setClassChoice] = useState('');
  const classFilter = classChoice === '' ? null : classChoice === 'none' ? { classIds: [], unclassified: true } : { classIds: [Number(classChoice)], unclassified: false };
  const [basis, setBasis] = useState(initial?.basis ?? 'ACCRUAL');
  const [split, setSplit] = useState('total'); // total | months | quarters
  const [compare, setCompare] = useState(initialCompare); // none | period | year
  const periodColumns = () => {
    if (split !== 'total') return splitPeriods(start, end, split);
    if (compare === 'none') return null;
    const [ps, pe] = priorRange(start, end, compare);
    return [
      { label: shortRange(start, end), startDate: start, endDate: end },
      { label: shortRange(ps, pe), startDate: ps, endDate: pe },
    ];
  };
  const multi = !byClass && (split !== 'total' || compare !== 'none');
  const { data, error, reload } = useLedgerQuery(() => {
    if (byClass) return api.getProfitAndLossByClass(start, end);
    if (!multi) return api.getProfitAndLoss(start, end, classFilter, basis);
    const columns = periodColumns();
    if (columns.length > 36) return Promise.reject(new Error('RPT_BAD_COLUMNS: choose a shorter range or quarters (at most 36 columns)'));
    return api.getProfitAndLossPeriods({ columns, total: split !== 'total', compare: split === 'total' && compare !== 'none', basis, classFilter });
  });

  // Once the fiscal calendar loads (or changes in Settings), re-resolve the selected preset.
  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolvePeriod(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (start && end && start <= end) reload();
  }, [start, end, classChoice, basis, split, compare, reload]);
  const classSubtitle = classChoice === '' ? '' : ` · Class: ${classChoice === 'none' ? 'Unclassified' : `${classLabel(classes, Number(classChoice))} (with subclasses)`}`;
  const company = useCompany();
  const subtitle = data
    ? (data.startDate === data.endDate ? longDate(data.startDate) : `${longDate(start)} – ${longDate(end)}`) + (byClass ? '' : classSubtitle)
    : '';

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
        {!byClass && (
          <>
            <label>
              <span className="qb-label">Basis</span>
              <select className="qb-input w-[100px]" value={basis} onChange={(e) => setBasis(e.target.value)}>
                <option value="ACCRUAL">Accrual</option>
                <option value="CASH">Cash</option>
              </select>
            </label>
            <label>
              <span className="qb-label">Columns</span>
              <select className="qb-input w-[120px]" value={split} onChange={(e) => setSplit(e.target.value)}>
                <option value="total">Total only</option>
                <option value="months">Months</option>
                <option value="quarters">Quarters</option>
              </select>
            </label>
            <label>
              <span className="qb-label">Compare with</span>
              <select className="qb-input w-[150px]" value={split === 'total' ? compare : 'none'} onChange={(e) => setCompare(e.target.value)} disabled={split !== 'total'}>
                <option value="none">Nothing</option>
                <option value="period">Previous period</option>
                <option value="year">Previous year</option>
              </select>
            </label>
          </>
        )}
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
        {data && !byClass && !multi && (
          <div className="ml-auto flex gap-4 self-center text-sm">
            <span className="text-qb-ink-muted">Revenue <span className="num font-semibold text-qb-ink">{formatCents(data.totalRevenueCents)}</span></span>
            <span className="text-qb-ink-muted">Expenses <span className="num font-semibold text-qb-ink">{formatCents(data.totalExpensesCents)}</span></span>
            <span className="text-qb-ink-muted">Net income <span className={`num font-semibold ${data.netIncomeCents < 0 ? 'text-qb-negative' : 'text-qb-green-active'}`}>{data.netIncomeDisplay}</span></span>
          </div>
        )}
        <div className={data && !byClass && !multi ? '' : 'ml-auto'}>
          <ExportMenu
            onNotice={setNotice}
            onCsv={() => {
              if (byClass) return saveCsv('profit_and_loss_by_class', { startDate: start, endDate: end }, `Profit-and-Loss-by-Class_${start}_to_${end}.csv`, setNotice);
              if (!multi && basis === 'ACCRUAL') return saveCsv('profit_and_loss', { startDate: start, endDate: end, classFilter }, `Profit-and-Loss_${start}_to_${end}.csv`, setNotice);
              // Cash basis and multi-column layouts export what is on screen.
              const columns = data.columns ?? [{ label: 'Total' }];
              const rows = [
                [company.name],
                [title ?? 'Profit and Loss'],
                [subtitle],
                [`${basis === 'CASH' ? 'Cash' : 'Accrual'} basis`],
                [],
                ['Account', ...columns.map((c) => c.label)],
                ...data.lines.map((l) => [
                  `${'  '.repeat(l.depth)}${l.label}`,
                  ...(data.columns ? l.amountsCents : [l.amountCents]).map((a, i) => (columns[i]?.kind === 'percent' ? (a === null ? '' : (a / 100).toFixed(1)) : plainAmount(a))),
                ]),
              ];
              return saveText(toCsv(rows), `Profit-and-Loss_${start}_to_${end}.csv`, setNotice);
            }}
            onXlsx={() => {
              if (byClass) return saveReportXlsx('profit_and_loss_by_class', { startDate: start, endDate: end }, `Profit-and-Loss-by-Class_${start}_to_${end}.xlsx`, setNotice);
              if (!multi && basis === 'ACCRUAL') return saveReportXlsx('profit_and_loss', { startDate: start, endDate: end, classFilter }, `Profit-and-Loss_${start}_to_${end}.xlsx`, setNotice);
              const sheet = statementSheet({
                company: company.name,
                title: title ?? 'Profit and Loss',
                subtitle,
                notes: [`${basis === 'CASH' ? 'Cash' : 'Accrual'} basis`],
                lines: data.lines,
                columns: data.columns ?? null,
              });
              return saveXlsx(sheet, `Profit-and-Loss_${start}_to_${end}.xlsx`, setNotice);
            }}
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
          title={title ?? (byClass ? 'Profit and Loss by Class' : 'Profit and Loss')}
          subtitle={subtitle}
          lines={data.lines}
          columns={byClass || multi ? data.columns : null}
          onAccount={(line) =>
            navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: data.startDate, endDate: data.endDate, includeSubaccounts: line.kind === 'subtotal' })
          }
          footer={
            byClass
              ? 'Accrual basis · posted transactions only · lines without a class are in Unclassified'
              : `${basis === 'CASH' ? 'Cash basis: invoices and bills count as they are paid' : 'Accrual basis'} · posted transactions only`
          }
        />
      )}
    </>
  );
}

function BalanceSheet({ navigate, initial = null }) {
  const [asOf, setAsOf] = useState(initial?.asOf ?? todayIso());
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
          <ExportMenu
            onNotice={setNotice}
            onCsv={() => saveCsv('balance_sheet', { asOf }, `Balance-Sheet_${asOf}.csv`, setNotice)}
            onXlsx={() => saveReportXlsx('balance_sheet', { asOf }, `Balance-Sheet_${asOf}.xlsx`, setNotice)}
          />
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

/** Balance Sheet as of a date beside the same date a year earlier, with $ and % change. */
function BalanceSheetComparison({ navigate }) {
  const [asOf, setAsOf] = useState(todayIso());
  const [notice, setNotice] = useState(null);
  const company = useCompany();
  const prior = asOf ? shiftYear(asOf) : null;
  const { data, error, reload } = useLedgerQuery(() =>
    asOf ? api.getBalanceSheetColumns([{ label: RANGE_LABEL.format(new Date(`${asOf}T00:00:00Z`)), asOf }, { label: RANGE_LABEL.format(new Date(`${prior}T00:00:00Z`)), asOf: prior }], true) : Promise.resolve(null),
  );
  useEffect(() => {
    reload();
  }, [asOf, reload]);
  const title = 'Balance Sheet Prev Year Comparison';
  const subtitle = asOf ? `As of ${longDate(asOf)} and ${longDate(prior)}` : '';
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">As of</span>
          <input type="date" className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
        <div className="ml-auto">
          <ExportMenu
            onNotice={setNotice}
            onCsv={() =>
              saveText(
                toCsv([
                  [company.name],
                  [title],
                  [subtitle],
                  [],
                  ['Account', ...data.columns.map((c) => c.label)],
                  ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, ...l.amountsCents.map((a, i) => (data.columns[i]?.kind === 'percent' ? (a === null ? '' : (a / 100).toFixed(1)) : plainAmount(a)))]),
                ]),
                `Balance-Sheet-Prev-Year-Comparison_${asOf}.csv`,
                setNotice,
              )
            }
            onXlsx={() => saveXlsx(statementSheet({ company: company.name, title, subtitle, notes: ['Accrual basis'], lines: data.lines, columns: data.columns }), `Balance-Sheet-Prev-Year-Comparison_${asOf}.xlsx`, setNotice)}
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
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: null, endDate: asOf, includeSubaccounts: line.kind === 'subtotal' })}
          footer="Accrual basis · each column closes earlier fiscal years' profit into Retained Earnings · % Change is against the earlier date"
        />
      )}
    </>
  );
}

/** The report list, grouped as in classic desktop accounting software. */
const REPORT_GROUPS = [
  {
    title: 'Business overview',
    reports: [
      ['pl', 'Profit and Loss'],
      ['plc', 'Profit and Loss by Class'],
      ['pl-py', 'Profit and Loss Prev Year Comparison'],
      ['bs', 'Balance Sheet'],
      ['bs-py', 'Balance Sheet Prev Year Comparison'],
      ['cf', 'Statement of Cash Flows'],
    ],
  },
  {
    title: 'Budgets',
    reports: [
      ['bva', 'Profit and Loss Budget vs. Actual'],
      ['bs-bva', 'Balance Sheet Budget vs. Actual'],
    ],
  },
  {
    title: 'Who owes you',
    reports: [
      ['ar', 'A/R Aging Summary'],
    ],
  },
  {
    title: 'Sales and customers',
    reports: [
      ['sales-customer', 'Sales by Customer Summary'],
      ['sales-product', 'Sales by Product/Service Summary'],
      ['sales-rep', 'Sales by Rep Summary'],
      ['item-price-list', 'Item Price List'],
    ],
  },
  { title: 'Jobs', reports: JOB_REPORTS },
  {
    title: 'What you owe',
    reports: [
      ['ap', 'A/P Aging Summary'],
      ['sales-tax', 'Sales Tax Liability'],
    ],
  },
  {
    title: 'Expenses and vendors',
    reports: [
      ['expenses-vendor', 'Expenses by Vendor Summary'],
      ['1099', '1099 Summary'],
    ],
  },
  {
    title: 'Inventory',
    reports: [
      ['inv-val', 'Inventory Valuation Summary'],
      ['inv-detail', 'Inventory Valuation Detail'],
      ['stock-status', 'Inventory Stock Status by Item'],
    ],
  },
  {
    title: 'Banking',
    reports: [['missing-checks', 'Missing Checks']],
  },
  { title: 'Graphs', reports: GRAPH_REPORTS },
  {
    title: 'Accountant & Taxes',
    reports: [
      ['tb', 'Trial Balance'],
      ['gl', 'General Ledger'],
      ...ACCOUNTANT_REPORTS,
    ],
  },
];

export default function Reports({ navigate, intent }) {
  const [report, setReportId] = useState(() => intent?.data?.report ?? 'pl');
  // Where a graph's drill-down opens a report: its dates (or budget, or as-of date) to start on.
  const [initial, setInitial] = useState(null);
  const setReport = (id) => {
    setReportId(id);
    setInitial(null);
  };
  const openReport = (id, params) => {
    setReportId(id);
    setInitial({ ...params, at: Date.now() });
  };
  const custom = useCustomReports();
  useEffect(() => {
    if (intent?.data?.report) setReport(intent.data.report);
  }, [intent]);
  const groups = withCustomReports(REPORT_GROUPS, custom);
  const reportName = groups.flatMap((g) => g.reports).find(([id]) => id === report)?.[1] ?? null;
  // The window is named after its report, and reopens on it with the desktop.
  useWindowContent(reportName ? { name: reportName, action: 'open', data: { report } } : null);
  // A report chosen in the list opens in a window of its own (or brings forward the window that
  // already shows it), as reports do in QuickBooks; the Report Center stays where it is.
  const openFromList = useOpenFromList();
  const { open: openChild } = useContext(WindowManagerContext);
  const chooseReport = (id) => id !== report && openFromList('reports', 'open', { report: id });
  const drill = initial ? `-${initial.at}` : '';
  return (
    <div className="flex h-full gap-2 p-2">
      <nav className="qb-panel no-print w-[230px] flex-none overflow-y-auto scrollbar-thin py-1" aria-label="Reports">
        {groups.map((group) => (
          <div key={group.title} className="mb-1">
            <div className="px-2 py-0.5 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">{group.title}</div>
            {group.reports.map(([id, label]) => (
              <button
                key={id}
                type="button"
                aria-current={report === id ? 'page' : undefined}
                onClick={() => chooseReport(id)}
                className={`block w-full px-3 py-0.5 text-left text-sm ${report === id ? 'bg-qb-green-tint font-semibold text-qb-green-active' : 'hover:bg-qb-row-hover'}`}
              >
                {label}
              </button>
            ))}
          </div>
        ))}
        <div className="border-t border-qb-border px-2 pt-1.5">
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row w-full justify-center text-xs"
            title="Keep this report open in a window of its own"
            onClick={() => openChild('reports', 'open', { report }, { forceNew: true })}
          >
            Open report in new window
          </button>
        </div>
      </nav>
      <div className="flex min-w-0 flex-1 flex-col gap-2 overflow-auto scrollbar-thin">
        {report === 'pl' && <ProfitAndLoss key={`pl${drill}`} navigate={navigate} initial={initial} />}
        {report === 'plc' && <ProfitAndLoss key="plc" byClass navigate={navigate} />}
        {report === 'bs' && <BalanceSheet key={`bs${drill}`} navigate={navigate} initial={initial} />}
        {report === 'cf' && <CashFlowReport navigate={navigate} />}
        {report === 'ar' && <AgingReport key={`ar${drill}`} payable={false} initial={initial} />}
        {report === 'ap' && <AgingReport key={`ap${drill}`} payable initial={initial} />}
        {report === 'sales-customer' && <SummaryReport key={`sc${drill}`} by="customer" initial={initial} />}
        {report === 'sales-product' && <SummaryReport key={`sp${drill}`} by="product" initial={initial} />}
        {report === 'sales-rep' && <SummaryReport key={`sr${drill}`} by="rep" initial={initial} />}
        {report === 'expenses-vendor' && <SummaryReport key="ev" by="vendor" />}
        {report === 'tb' && <TrialBalanceReport navigate={navigate} />}
        {report === 'sales-tax' && <SalesTaxLiabilityReport />}
        {report === 'gl' && <GeneralLedgerReport navigate={navigate} />}
        {report === 'pl-py' && <ProfitAndLoss key="pl-py" initialCompare="year" title="Profit and Loss Prev Year Comparison" navigate={navigate} />}
        {report === 'bs-py' && <BalanceSheetComparison navigate={navigate} />}
        {report === 'bva' && <BudgetVsActualReport key={`bva${drill}`} navigate={navigate} initial={initial} />}
        {report === 'bs-bva' && <BalanceSheetBudgetVsActualReport navigate={navigate} />}
        {report === 'inv-val' && <InventoryValuationReport />}
        {report === 'inv-detail' && <InventoryValuationDetailReport navigate={navigate} />}
        {report === 'stock-status' && <StockStatusReport />}
        {report === 'item-price-list' && <ItemPriceListReport />}
        {report === 'missing-checks' && <MissingChecksReport navigate={navigate} />}
        {report === '1099' && <Vendors1099Report />}
        {isJobReport(report) && <JobReport key={report} report={report} navigate={navigate} />}
        {isGraphReport(report) && <GraphReport key={report} report={report} navigate={navigate} onOpenReport={openReport} />}
        {isAccountantReport(report) && <AccountantReports key={report} report={report} navigate={navigate} />}
        {isCustomReport(report) && <CustomReport key={report} report={report} custom={custom} navigate={navigate} onOpen={setReport} />}
      </div>
    </div>
  );
}
