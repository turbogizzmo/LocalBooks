import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { ExportMenu, PERIODS, Statement, longDate, plainAmount, resolvePeriod, saveText, saveXlsx, statementSheet, toCsv } from './ReportParts.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ReportTable, reportExports } from './CustomReport.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { todayIso } from '../lib/dates.js';

/**
 * The job costing reports (`job_reports.rs`) in the Reports menu's Jobs group: profitability and
 * estimates against actuals by customer/job and by product/service, estimates by job, progress
 * invoicing, and Profit and Loss by Job. Rows open the report beneath them, down to transactions.
 */

/** Reports menu entries: [menu id, title]. */
export const JOB_REPORTS = [
  ['job-profit', 'Job Profitability Summary'],
  ['job-profit-detail', 'Job Profitability Detail'],
  ['job-est', 'Job Estimates vs. Actuals Summary'],
  ['job-est-detail', 'Job Estimates vs. Actuals Detail'],
  ['item-profit', 'Item Profitability'],
  ['item-est', 'Item Estimates vs. Actuals'],
  ['pl-job', 'Profit and Loss by Job'],
  ['est-by-job', 'Estimates by Job'],
  ['progress', 'Progress Invoices vs. Estimates'],
];

const KINDS = {
  'job-profit': 'jobProfitSummary',
  'job-profit-detail': 'jobProfitDetail',
  'job-est': 'jobEstimatesSummary',
  'job-est-detail': 'jobEstimatesDetail',
  'item-profit': 'itemProfit',
  'item-est': 'itemEstimates',
  'est-by-job': 'estimatesByJob',
  progress: 'progressInvoices',
};

const TITLES = {
  jobProfitSummary: 'Job Profitability Summary',
  jobProfitDetail: 'Job Profitability Detail',
  jobTransactions: 'Job Transactions',
  jobEstimatesSummary: 'Job Estimates vs. Actuals Summary',
  jobEstimatesDetail: 'Job Estimates vs. Actuals Detail',
  itemProfit: 'Item Profitability',
  itemEstimates: 'Item Estimates vs. Actuals',
  estimatesByJob: 'Estimates by Job',
  progressInvoices: 'Progress Invoices vs. Estimates',
};

const FOOTERS = {
  estimatesByJob: 'click an estimate to open Estimates',
  progressInvoices: 'Invoiced is the invoice an estimate became · click a row to open its invoice',
};
const ACTUALS = 'Accrual basis · revenue is income less credit memos; cost is expense and cost of goods sold charged to the customer or job · time isn’t costed';
const ESTIMATES = 'Estimates carry no cost, so only revenue is compared · rejected estimates are left out';

const RANGE_PERIODS = [...PERIODS.filter((p) => p.id !== 'custom'), { id: 'all', label: 'All dates' }, { id: 'custom', label: 'Custom' }];

/** Whether a Reports menu id is one of this screen's. */
export const isJobReport = (id) => id === 'pl-job' || id in KINDS;

function useRange(initial) {
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [period, setPeriod] = useState(initial);
  const resolve = (p, f) => (p === 'all' ? [null, todayIso()] : resolvePeriod(p, f));
  const [range, setRange] = useState(() => resolve(initial, null));
  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolve(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps
  const choose = (p) => {
    setPeriod(p);
    if (p !== 'custom') setRange(resolve(p, fiscal));
  };
  return { period, choose, range, setRange: (r) => { setPeriod('custom'); setRange(r); }, periods: initial === 'all' ? RANGE_PERIODS : PERIODS };
}

function RangeControls({ range: r }) {
  const [start, end] = r.range;
  return (
    <>
      <label>
        <span className="qb-label">Report period</span>
        <select className="qb-input w-[160px]" value={r.period} onChange={(e) => r.choose(e.target.value)}>
          {r.periods.map((p) => (
            <option key={p.id} value={p.id}>{p.label}</option>
          ))}
        </select>
      </label>
      <label>
        <span className="qb-label">From</span>
        <input type="date" className="qb-input w-[132px]" value={start ?? ''} onChange={(e) => r.setRange([e.target.value || null, end])} />
      </label>
      <label>
        <span className="qb-label">To</span>
        <input type="date" className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => r.setRange([start, e.target.value])} aria-invalid={Boolean(start && end && end < start)} />
      </label>
    </>
  );
}

const subtitleFor = (start, end) => (start ? (start === end ? longDate(start) : `${longDate(start)} – ${longDate(end)}`) : `All dates through ${longDate(end)}`);

export default function JobReport({ report, navigate }) {
  return report === 'pl-job' ? <ProfitAndLossByJob navigate={navigate} /> : <JobReportScreen key={report} kind={KINDS[report]} navigate={navigate} />;
}

function JobReportScreen({ kind, navigate }) {
  const company = useCompany();
  const range = useRange('all');
  const [start, end] = range.range;
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  // '' = every customer/job, 'none' = lines with no customer/job, otherwise a customer with its jobs.
  const [job, setJob] = useState('');
  const [by, setBy] = useState('item');
  // Drill-downs: each opens the report beneath the row, with the same dates.
  const [stack, setStack] = useState([]);
  const [notice, setNotice] = useState(null);
  const top = { kind, title: TITLES[kind], filter: job === '' ? {} : job === 'none' ? { noJob: true } : { jobIds: [Number(job)], withSubjobs: true } };
  const view = stack[stack.length - 1] ?? top;
  const valid = Boolean(end) && (!start || start <= end);
  const params = { startDate: start, endDate: end, filter: view.filter, by };
  const runKey = JSON.stringify([view.kind, params]);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getJobReport(view.kind, params) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [runKey, reload]); // eslint-disable-line react-hooks/exhaustive-deps

  const open = (row) => {
    if (row.journalEntryId) return navigate?.('accounting/journal', 'open-entry', { entryId: row.journalEntryId });
    if (row.drill) return setStack((s) => [...s, { kind: row.drill.report, title: `${TITLES[row.drill.report]}: ${String(row.cells[0]).replace(/^Total /, '')}`, filter: row.drill.filter }]);
    if (row.estimateId) return navigate?.('sales/estimates', 'open', { estimateId: row.estimateId });
    return undefined;
  };
  const subtitle = data ? subtitleFor(data.startDate, data.endDate) : '';
  const filename = view.title.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'Report';
  const exports = data && reportExports({ company: company.name, title: view.title, subtitle, report: data, filename, onNotice: setNotice });
  const estimates = ['jobEstimatesSummary', 'jobEstimatesDetail', 'itemEstimates'].includes(view.kind);
  const footer = FOOTERS[view.kind] ?? `${estimates ? ESTIMATES : ACTUALS} · click a row to see what is behind it`;

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <RangeControls range={range} />
        <label>
          <span className="qb-label">Customer:Job</span>
          <select className="qb-input w-[220px]" value={job} onChange={(e) => { setJob(e.target.value); setStack([]); }}>
            <option value="">All customers and jobs</option>
            {customers.map((c) => (
              <option key={c.id} value={c.id}>
                {'  '.repeat((c.name.match(/:/g) ?? []).length)}
                {c.name.split(':').pop()}
              </option>
            ))}
            <option value="none">No customer/job</option>
          </select>
        </label>
        {view.kind === 'jobProfitDetail' && (
          <label>
            <span className="qb-label">Break down by</span>
            <select className="qb-input w-[150px]" value={by} onChange={(e) => setBy(e.target.value)}>
              <option value="item">Product/service</option>
              <option value="account">Account</option>
            </select>
          </label>
        )}
        {stack.length > 0 && (
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setStack((s) => s.slice(0, -1))}>
            ‹ Back
          </button>
        )}
        <div className="ml-auto">{exports && <ExportMenu onNotice={setNotice} onCsv={exports.onCsv} onXlsx={exports.onXlsx} />}</div>
      </div>
      {!valid && <Notice kind="error" text="The end date is before the start date." />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && <ReportTable company={company} title={view.title} subtitle={subtitle} report={data} onRow={open} footer={footer} />}
    </>
  );
}

/** Profit and Loss with a column per customer and job, like Profit and Loss by Class. */
function ProfitAndLossByJob({ navigate }) {
  const company = useCompany();
  const range = useRange('fiscalYearToDate');
  const [start, end] = range.range;
  const [notice, setNotice] = useState(null);
  const valid = Boolean(start && end && start <= end);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getProfitAndLossByJob(start, end) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [start, end, reload]); // eslint-disable-line react-hooks/exhaustive-deps
  const title = 'Profit and Loss by Job';
  const subtitle = valid ? subtitleFor(start, end) : '';
  const name = `Profit-and-Loss-by-Job_${start}_to_${end}`;
  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <RangeControls range={range} />
        <div className="ml-auto">
          {data && (
            <ExportMenu
              onNotice={setNotice}
              onCsv={() =>
                saveText(
                  toCsv([[company.name], [title], [subtitle], ['Accrual basis'], [], ['Account', ...data.columns.map((c) => c.label)], ...data.lines.map((l) => [`${'  '.repeat(l.depth)}${l.label}`, ...l.amountsCents.map(plainAmount)])]),
                  `${name}.csv`,
                  setNotice,
                )
              }
              onXlsx={() => saveXlsx(statementSheet({ company: company.name, title, subtitle, notes: ['Accrual basis'], lines: data.lines, columns: data.columns }), `${name}.xlsx`, setNotice)}
            />
          )}
        </div>
      </div>
      {!valid && <Notice kind="error" text="Choose a start date on or before the end date." />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && data.columns.every((c) => c.isTotal || c.kind === 'unclassified') && <Notice kind="info" text="No income or expenses in this period belong to a customer or job." />}
      {data && (
        <Statement
          title={title}
          subtitle={subtitle}
          lines={data.lines}
          columns={data.columns}
          onAccount={(line) => navigate?.('accounting/register', 'open', { accountId: line.accountId, startDate: start, endDate: end, includeSubaccounts: line.kind === 'subtotal' })}
          footer="Accrual basis · sales belong to their customer; bills, expenses and vendor credits to the customer or job on each line · the rest is in No customer/job"
        />
      )}
    </>
  );
}
