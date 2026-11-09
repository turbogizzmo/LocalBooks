import { useState } from 'react';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { longDate } from './ReportParts.jsx';
import { RANGE_PERIODS, ReportTable, datesFor, engineOnly, reportSheet } from './CustomReport.jsx';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { todayIso } from '../lib/dates.js';
import { savedCopyText } from '../lib/plainCopy.js';

/**
 * Process Multiple Reports: pick memorized reports and run them together, each with its own saved
 * dates or one report period for all, then show them one after another, print them in one go (each
 * on a new page) or save them as one Excel workbook with a worksheet per report.
 */

const OWN_DATES = 'own';

/** What a memorized report runs with: its title, whether it is as of a date, and the engine settings with dates. */
export function memorizedRun(saved, fiscal, period = OWN_DATES) {
  const s = saved.settings ?? {};
  const asOf = (s.dateMode ?? 'range') === 'asOf';
  let start = null;
  let end;
  if (period === OWN_DATES) {
    if (asOf) end = s.endDate ?? todayIso();
    else [start, end] = datesFor(s.period ?? 'custom', fiscal, s);
  } else {
    const [from, to] = datesFor(period, fiscal, null);
    end = to;
    start = asOf ? null : from;
  }
  return { id: saved.id, title: s.title ?? saved.name, asOf, settings: { ...engineOnly(s), startDate: start, endDate: end } };
}

function subtitleOf(run, report) {
  if (run.asOf) return `As of ${longDate(report.endDate)}`;
  if (!report.startDate) return `All dates through ${longDate(report.endDate)}`;
  return report.startDate === report.endDate ? longDate(report.startDate) : `${longDate(report.startDate)} – ${longDate(report.endDate)}`;
}

export default function ProcessMultipleReports({ custom, navigate }) {
  const company = useCompany();
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [chosen, setChosen] = useState(() => new Set());
  const [period, setPeriod] = useState(OWN_DATES);
  const [results, setResults] = useState(null); // [{ run, report, subtitle }]
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const saved = custom.saved;
  const picked = saved.filter((s) => chosen.has(s.id));

  const runAll = async () => {
    const out = [];
    for (const s of picked) {
      const run = memorizedRun(s, fiscal, period);
      const report = await api.getCustomReport(run.settings);
      out.push({ run, report, subtitle: subtitleOf(run, report) });
    }
    return out;
  };
  const act = (then) => async () => {
    setBusy(true);
    setNotice(null);
    try {
      const out = await runAll();
      setResults(out);
      await then?.(out);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  // Printing waits a frame so the reports are in the print area first.
  const print = () => new Promise((resolve) => requestAnimationFrame(() => resolve(printDocument('Reports'))));
  const exportXlsx = async (out) => {
    const sheets = out.map(({ run, report, subtitle }) => reportSheet({ company: company.name, title: run.title, subtitle, report }));
    const path = await api.saveXlsxBook(sheets, 'Reports.xlsx');
    if (path) setNotice({ kind: 'success', text: savedCopyText(path) });
  };

  if (saved.length === 0) {
    return (
      <div className="qb-panel p-3 text-sm text-qb-ink-muted">
        No memorized reports yet. Open a report, change it with Customize… if you like, and choose Save as… to memorize it; then pick it here to run it with others.
      </div>
    );
  }
  const all = chosen.size === saved.length;
  return (
    <>
      <section className="qb-panel no-print flex-none px-2 py-1.5" aria-label="Process multiple reports">
        <div className="mb-1 flex flex-wrap items-end gap-2">
          <h3 className="mr-2 self-center text-sm font-semibold">Process Multiple Reports</h3>
          <label>
            <span className="qb-label">Report period</span>
            <select className="qb-input w-[200px]" value={period} onChange={(e) => setPeriod(e.target.value)}>
              <option value={OWN_DATES}>Each report’s own dates</option>
              {RANGE_PERIODS.filter((p) => p.id !== 'custom' && p.id !== 'all').map((p) => (
                <option key={p.id} value={p.id}>{p.label}</option>
              ))}
            </select>
          </label>
          <div className="ml-auto flex gap-1">
            <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || picked.length === 0} onClick={act(null)}>Display</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || picked.length === 0} onClick={act(print)}>Print</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy || picked.length === 0} onClick={act(exportXlsx)}>Export to Excel</button>
          </div>
        </div>
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-qb-border-strong text-left text-xs uppercase tracking-wide text-qb-ink-muted">
              <th className="w-6 px-1">
                <input type="checkbox" className="accent-qb-green" aria-label="Choose every report" checked={all} onChange={() => setChosen(all ? new Set() : new Set(saved.map((s) => s.id)))} />
              </th>
              <th className="px-1">Memorized report</th>
              <th className="px-1">Dates</th>
            </tr>
          </thead>
          <tbody>
            {saved.map((s) => {
              const run = memorizedRun(s, fiscal, period);
              const dates = run.asOf ? `As of ${longDate(run.settings.endDate)}` : run.settings.startDate ? `${longDate(run.settings.startDate)} – ${longDate(run.settings.endDate)}` : `All dates through ${longDate(run.settings.endDate)}`;
              return (
                <tr key={s.id} className="hover:bg-qb-row-hover">
                  <td className="px-1">
                    <input
                      type="checkbox"
                      className="accent-qb-green"
                      aria-label={`Choose ${s.name}`}
                      checked={chosen.has(s.id)}
                      onChange={(e) =>
                        setChosen((c) => {
                          const next = new Set(c);
                          if (e.target.checked) next.add(s.id);
                          else next.delete(s.id);
                          return next;
                        })
                      }
                    />
                  </td>
                  <td className="px-1">{s.name}</td>
                  <td className="px-1 text-qb-ink-muted">{dates}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {busy && <div className="qb-panel p-2 text-sm text-qb-ink-muted">Running {picked.length} reports…</div>}
      {results?.map(({ run, report, subtitle }, i) => (
        <ReportTable
          key={`${run.id}-${i}`}
          company={company}
          title={run.title}
          subtitle={subtitle}
          report={report}
          onRow={(r) => r.journalEntryId && navigate?.('accounting/journal', 'open-entry', { entryId: r.journalEntryId })}
          pageBreak={i > 0}
          footer="memorized report · click a line to open its transaction"
        />
      ))}
    </>
  );
}
