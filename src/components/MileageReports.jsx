import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { ExportMenu, PERIODS, longDate, resolvePeriod } from './ReportParts.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ReportTable, reportExports } from './CustomReport.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { todayIso } from '../lib/dates.js';

/**
 * The mileage reports (`mileage.rs`) in the Reports menu's Jobs group: miles and deductible amount
 * (miles × the mileage rate in effect on each trip's date) by vehicle and by customer/job.
 */

/** Reports menu entries: [menu id, title]. */
export const MILEAGE_REPORTS = [
  ['mileage-vehicle', 'Mileage by Vehicle Summary'],
  ['mileage-vehicle-detail', 'Mileage by Vehicle Detail'],
  ['mileage-job', 'Mileage by Job Summary'],
  ['mileage-job-detail', 'Mileage by Job Detail'],
];

const KINDS = {
  'mileage-vehicle': 'vehicleSummary',
  'mileage-vehicle-detail': 'vehicleDetail',
  'mileage-job': 'jobSummary',
  'mileage-job-detail': 'jobDetail',
};

const TITLES = {
  vehicleSummary: 'Mileage by Vehicle Summary',
  vehicleDetail: 'Mileage by Vehicle Detail',
  jobSummary: 'Mileage by Job Summary',
  jobDetail: 'Mileage by Job Detail',
};

const RANGE_PERIODS = [...PERIODS.filter((p) => p.id !== 'custom'), { id: 'all', label: 'All dates' }, { id: 'custom', label: 'Custom' }];

/** Whether a Reports menu id is one of this screen's. */
export const isMileageReport = (id) => id in KINDS;

const subtitleFor = (start, end) => (start ? (start === end ? longDate(start) : `${longDate(start)} – ${longDate(end)}`) : `All dates through ${longDate(end)}`);

export default function MileageReport({ report, navigate }) {
  const kind = KINDS[report];
  const company = useCompany();
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const resolve = (p, f) => (p === 'all' ? [null, todayIso()] : resolvePeriod(p, f));
  const [period, setPeriod] = useState('fiscalYearToDate');
  const [range, setRange] = useState(() => resolve('fiscalYearToDate', null));
  useEffect(() => {
    if (fiscal && period !== 'custom') setRange(resolve(period, fiscal));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps
  const [start, end] = range;
  const vehicles = useLedgerQuery(api.listVehicles).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const byVehicle = kind === 'vehicleSummary' || kind === 'vehicleDetail';
  const [filter, setFilter] = useState('');
  // A summary row opens its detail report for that vehicle or customer/job.
  const [drill, setDrill] = useState(null);
  const [notice, setNotice] = useState(null);

  const view = drill ?? { kind, title: TITLES[kind], filter };
  const params = { startDate: start, endDate: end };
  if (drill) Object.assign(params, { vehicleId: drill.vehicleId, customerId: drill.customerId, noJob: drill.noJob });
  else if (filter === 'none') params.noJob = true;
  else if (filter !== '') params[byVehicle ? 'vehicleId' : 'customerId'] = Number(filter);
  const valid = Boolean(end) && (!start || start <= end);
  const runKey = JSON.stringify([view.kind, params]);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getMileageReport(view.kind, params) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [runKey, reload]); // eslint-disable-line react-hooks/exhaustive-deps

  const open = (row) => {
    if (!row.drill) return;
    setDrill({ kind: row.drill.report, title: `${TITLES[row.drill.report]}: ${row.cells[0]}`, vehicleId: row.drill.vehicleId, customerId: row.drill.customerId, noJob: row.drill.noJob });
  };
  const choosePeriod = (p) => {
    setPeriod(p);
    if (p !== 'custom') setRange(resolve(p, fiscal));
  };
  const subtitle = data ? subtitleFor(data.startDate, data.endDate) : '';
  const filename = view.title.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'Report';
  const exports = data && reportExports({ company: company.name, title: view.title, subtitle, report: data, filename, onNotice: setNotice });
  const summary = view.kind === 'vehicleSummary' || view.kind === 'jobSummary';
  const footer = `Deductible amount is miles × the mileage rate in effect on each trip’s date${summary ? ' · click a row to see its trips' : ''}`;

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Report period</span>
          <select className="qb-input w-[160px]" value={period} onChange={(e) => choosePeriod(e.target.value)}>
            {RANGE_PERIODS.map((p) => (
              <option key={p.id} value={p.id}>{p.label}</option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">From</span>
          <input type="date" className="qb-input w-[132px]" value={start ?? ''} onChange={(e) => { setPeriod('custom'); setRange([e.target.value || null, end]); }} />
        </label>
        <label>
          <span className="qb-label">To</span>
          <input type="date" className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value]); }} aria-invalid={Boolean(start && end && end < start)} />
        </label>
        <label>
          <span className="qb-label">{byVehicle ? 'Vehicle' : 'Customer:Job'}</span>
          <select className="qb-input w-[220px]" value={filter} onChange={(e) => { setFilter(e.target.value); setDrill(null); }}>
            <option value="">{byVehicle ? 'All vehicles' : 'All customers and jobs'}</option>
            {(byVehicle ? vehicles : customers).map((c) => (
              <option key={c.id} value={c.id}>{c.name}</option>
            ))}
            {!byVehicle && <option value="none">No customer/job</option>}
          </select>
        </label>
        {drill && (
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setDrill(null)}>
            ‹ Back
          </button>
        )}
        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => navigate?.('employees/mileage')}>
          Enter mileage
        </button>
        <div className="ml-auto">{exports && <ExportMenu onNotice={setNotice} onCsv={exports.onCsv} onXlsx={exports.onXlsx} />}</div>
      </div>
      {!valid && <Notice kind="error" text="The end date is before the start date." />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && data.tripsWithoutRate > 0 && (
        <Notice kind="info" text={`${data.tripsWithoutRate} trip${data.tripsWithoutRate === 1 ? ' is' : 's are'} dated before your first mileage rate and ${data.tripsWithoutRate === 1 ? 'has' : 'have'} no deductible amount. Add the rate under Employees › Mileage › Mileage rates.`} />
      )}
      {data && <ReportTable company={company} title={view.title} subtitle={subtitle} report={data} onRow={open} footer={footer} empty="No trips match this report’s dates and filters." />}
    </>
  );
}
