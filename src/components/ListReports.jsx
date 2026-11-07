import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate } from './ReportParts.jsx';
import { RANGE_PERIODS, ReportTable, datesFor, reportExports } from './CustomReport.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { todayIso } from '../lib/dates.js';
import DateBox from './DateBox.jsx';

/**
 * Reports outside the report builder (`list_reports.rs`): the phone and contact lists, item and
 * account listings, Balance Sheet Summary, P&L YTD Comparison, Average Days to Pay, Unpaid Bills by
 * Job, Open Purchase Orders Detail, the time reports, the Physical Inventory Worksheet and the Audit
 * Trail. Each comes back in the builder's shape, so it shows, prints and exports the same way.
 */

/** The catalog, for the Reports menu. */
export function useListReports() {
  return useLedgerQuery(api.listListReports).data ?? [];
}

/** The report list with each list report added to its group (new groups at the end). */
export function withListReports(groups, catalog) {
  const out = groups.map((g) => ({ ...g, reports: [...g.reports] }));
  for (const r of catalog) {
    const entry = [`list:${r.id}`, r.title];
    const group = out.find((g) => g.title === r.group);
    if (group) group.reports.push(entry);
    else out.push({ title: r.group, reports: [entry] });
  }
  return out;
}

export const isListReport = (id) => id.startsWith('list:');

export default function ListReport({ report, catalog, navigate }) {
  const info = catalog.find((r) => `list:${r.id}` === report);
  if (!info) return <div className="qb-panel p-2 text-sm text-qb-ink-muted">Loading report…</div>;
  return <Shown key={info.id} info={info} navigate={navigate} />;
}

function Shown({ info, navigate }) {
  const company = useCompany();
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [notice, setNotice] = useState(null);
  const [period, setPeriod] = useState(info.period || 'custom');
  const [[start, end], setRange] = useState(() => (info.dates === 'range' ? datesFor(info.period, null, null) : [null, todayIso()]));
  useEffect(() => {
    if (fiscal && info.dates === 'range' && period !== 'custom') setRange(datesFor(period, fiscal, null));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps

  const valid = info.dates === 'none' || (Boolean(end) && (!start || start <= end));
  const key = `${info.id}|${start}|${end}`;
  const { data, error, reload } = useLedgerQuery(() =>
    valid ? api.getListReport(info.id, info.dates === 'range' ? start : null, info.dates === 'none' ? null : end) : Promise.resolve(null),
  );
  useEffect(() => {
    reload();
  }, [key, reload]);

  const subtitle = !data
    ? ''
    : info.dates === 'none'
      ? longDate(data.endDate)
      : info.dates === 'asOf'
        ? `As of ${longDate(data.endDate)}`
        : data.startDate === data.endDate
          ? longDate(data.startDate)
          : `${longDate(data.startDate)} – ${longDate(data.endDate)}`;
  const filename = info.title.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'Report';
  const exports = data && reportExports({ company: company.name, title: info.title, subtitle, report: data, filename, onNotice: setNotice });
  const open = (row) => {
    if (row.journalEntryId) return navigate?.('accounting/journal', 'open-entry', { entryId: row.journalEntryId });
    if (row.accountId) return navigate?.('accounting/register', 'open', { accountId: row.accountId, startDate: info.dates === 'range' ? start : null, endDate: end });
    return undefined;
  };

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {info.dates === 'asOf' && (
          <label>
            <span className="qb-label">As of</span>
            <DateBox className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => setRange([null, e.target.value])} />
          </label>
        )}
        {info.dates === 'range' && (
          <>
            <label>
              <span className="qb-label">Report period</span>
              <select
                className="qb-input w-[160px]"
                value={period}
                onChange={(e) => {
                  setPeriod(e.target.value);
                  if (e.target.value !== 'custom') setRange(datesFor(e.target.value, fiscal, null));
                }}
              >
                {RANGE_PERIODS.filter((p) => p.id !== 'all').map((p) => (
                  <option key={p.id} value={p.id}>{p.label}</option>
                ))}
              </select>
            </label>
            <label>
              <span className="qb-label">From</span>
              <DateBox className="qb-input w-[132px]" value={start ?? ''} onChange={(e) => { setPeriod('custom'); setRange([e.target.value || null, end]); }} />
            </label>
            <label>
              <span className="qb-label">To</span>
              <DateBox className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value]); }} aria-invalid={!valid} />
            </label>
          </>
        )}
        <div className="ml-auto">{exports && <ExportMenu onNotice={setNotice} onCsv={exports.onCsv} onXlsx={exports.onXlsx} />}</div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && (
        <ReportTable
          company={company}
          title={info.title}
          subtitle={subtitle}
          report={data}
          onRow={open}
          empty="Nothing to show for these dates."
          footer={[...(data.notes ?? []), data.rows.some((r) => r.journalEntryId || r.accountId) ? 'click a line to open it' : null].filter(Boolean).join(' · ')}
        />
      )}
    </>
  );
}
