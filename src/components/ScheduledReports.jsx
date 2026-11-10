// Reports › Scheduled Reports (report_schedules.rs): chosen reports run daily to quarterly and are
// emailed through the company's email server, or saved to a folder, as PDF or Excel. They run only
// while LocalBooks is open; a run missed while it was closed happens once when it opens.
import { useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import Notice from './Notice.jsx';
import RowMenu from './RowMenu.jsx';
import DateBox from './DateBox.jsx';
import { Dialog } from './InventoryDialog.jsx';
import { useCustomReports } from './CustomReport.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { formatDate, todayIso } from '../lib/dates.js';

const STATEMENTS = [
  ['profit_and_loss', 'Profit and Loss'],
  ['profit_and_loss_by_class', 'Profit and Loss by Class'],
  ['balance_sheet', 'Balance Sheet'],
];

export const FREQUENCIES = [
  ['DAILY', 'Every day'],
  ['WEEKLY', 'Every week'],
  ['MONTHLY', 'Every month'],
  ['QUARTERLY', 'Every quarter'],
];

export const PERIODS = [
  ['YESTERDAY', 'Yesterday'],
  ['LAST_WEEK', 'Last week'],
  ['LAST_MONTH', 'Last month'],
  ['MONTH_TO_DATE', 'Month to date'],
  ['LAST_QUARTER', 'Last quarter'],
  ['QUARTER_TO_DATE', 'Quarter to date'],
  ['LAST_YEAR', 'Last year'],
  ['YEAR_TO_DATE', 'Year to date'],
];

/** A report's key in the picker: "statement:balance_sheet", "preset:open-invoices", "saved:3". */
const keyOf = (r) => (r.kind === 'statement' ? `statement:${r.report}` : r.kind === 'preset' ? `preset:${r.id}` : `saved:${r.id}`);
function fromKey(key, period) {
  const [kind, id] = [key.slice(0, key.indexOf(':')), key.slice(key.indexOf(':') + 1)];
  if (kind === 'statement') return { kind, report: id, period };
  if (kind === 'saved') return { kind, id: Number(id), period };
  return { kind, id, period };
}

function useReportChoices() {
  const { presets, saved } = useCustomReports();
  return [
    ...STATEMENTS.map(([id, name]) => [`statement:${id}`, name]),
    ...presets.map((p) => [`preset:${p.id}`, p.title]),
    ...saved.map((s) => [`saved:${s.id}`, `${s.name} (memorized)`]),
  ];
}

function ScheduleDialog({ schedule, onClose, onSaved }) {
  const choices = useReportChoices();
  const [draft, setDraft] = useState(
    schedule ?? { name: '', reports: [{ kind: 'statement', report: 'profit_and_loss', period: 'LAST_MONTH' }], frequency: 'MONTHLY', startDate: todayIso(), endDate: null, destination: 'EMAIL', recipients: '', folder: '', format: 'PDF', isActive: true },
  );
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const set = (key) => (value) => setDraft((d) => ({ ...d, [key]: value }));
  const setReport = (i, r) => setDraft((d) => ({ ...d, reports: d.reports.map((x, j) => (j === i ? r : x)) }));

  const chooseFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === 'string') set('folder')(picked);
  };
  const save = async (e) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const saved = await api.saveReportSchedule({ ...draft, endDate: draft.endDate || null, pdfPassword: draft.pdfPassword || null }, schedule?.id ?? null);
      onSaved(`“${saved.name}” saved. ${saved.nextRun ? `It next runs on ${formatDate(saved.nextRun)}.` : 'It has no more runs.'}`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={schedule ? `Edit schedule · ${schedule.name}` : 'New scheduled reports'} onClose={onClose}>
      <form onSubmit={save} className="flex min-h-0 flex-col gap-2 overflow-auto p-3 text-sm">
        <label>
          <span className="qb-label">Name</span>
          <input className="qb-input w-full" value={draft.name} onChange={(e) => set('name')(e.target.value)} maxLength={80} placeholder="Month-end reports for my accountant" />
        </label>
        <fieldset className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
          <legend className="px-1 text-xs font-semibold text-qb-ink-muted">Reports</legend>
          {draft.reports.map((r, i) => (
            <div key={i} className="flex items-center gap-1">
              <select className="qb-input min-w-0 flex-1" value={keyOf(r)} onChange={(e) => setReport(i, fromKey(e.target.value, r.period))} aria-label={`Report ${i + 1}`}>
                {choices.map(([key, name]) => (
                  <option key={key} value={key}>
                    {name}
                  </option>
                ))}
              </select>
              <select className="qb-input w-[140px]" value={r.period} onChange={(e) => setReport(i, { ...r, period: e.target.value })} aria-label={`Report ${i + 1} dates`} title="The dates it covers, counted from the day it runs (a one-date report is as of the last day)">
                {PERIODS.map(([id, label]) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
              <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-negative" disabled={draft.reports.length === 1} onClick={() => setDraft((d) => ({ ...d, reports: d.reports.filter((_, j) => j !== i) }))} aria-label={`Remove report ${i + 1}`}>
                ×
              </button>
            </div>
          ))}
          <button type="button" className="self-start text-qb-link hover:underline" onClick={() => setDraft((d) => ({ ...d, reports: [...d.reports, { kind: 'statement', report: 'balance_sheet', period: 'LAST_MONTH' }] }))}>
            Add a report
          </button>
        </fieldset>
        <div className="flex flex-wrap items-end gap-2">
          <label>
            <span className="qb-label">How often</span>
            <select className="qb-input w-[140px]" value={draft.frequency} onChange={(e) => set('frequency')(e.target.value)}>
              {FREQUENCIES.map(([id, label]) => (
                <option key={id} value={id}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">Starting</span>
            <DateBox className="qb-input w-[132px]" value={draft.startDate} onChange={(e) => set('startDate')(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">Ending (optional)</span>
            <DateBox className="qb-input w-[132px]" value={draft.endDate ?? ''} onChange={(e) => set('endDate')(e.target.value || null)} />
          </label>
        </div>
        <fieldset className="flex flex-col gap-1 rounded-xs border border-qb-border p-2">
          <legend className="px-1 text-xs font-semibold text-qb-ink-muted">Where they go</legend>
          <div className="flex gap-3" role="radiogroup" aria-label="Where they go">
            <label className="flex items-center gap-1">
              <input type="radio" checked={draft.destination === 'EMAIL'} onChange={() => set('destination')('EMAIL')} />
              Email them
            </label>
            <label className="flex items-center gap-1">
              <input type="radio" checked={draft.destination === 'FOLDER'} onChange={() => set('destination')('FOLDER')} />
              Save them in a folder
            </label>
          </div>
          {draft.destination === 'EMAIL' ? (
            <label>
              <span className="qb-label">To</span>
              <input className="qb-input w-full" value={draft.recipients} onChange={(e) => set('recipients')(e.target.value)} placeholder="accountant@example.com, partner@example.com" />
              <span className="text-2xs text-qb-ink-faint">Sent through the email server in Settings › Email, which must be turned on.</span>
            </label>
          ) : (
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate rounded-xs border border-qb-border bg-qb-canvas px-1.5 py-0.5" title={draft.folder}>
                {draft.folder || 'No folder chosen'}
              </span>
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={chooseFolder}>
                Choose folder…
              </button>
            </div>
          )}
          <label className="flex items-center gap-2">
            <span className="qb-label">As</span>
            <select className="qb-input w-[160px]" value={draft.format} onChange={(e) => setDraft((d) => ({ ...d, format: e.target.value, ...(e.target.value === 'PDF' ? {} : { protectPdf: false, pdfPassword: null }) }))}>
              <option value="PDF">PDF</option>
              <option value="XLSX">Excel spreadsheet</option>
            </select>
          </label>
          {draft.format === 'PDF' && (
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={!!draft.protectPdf} onChange={(e) => set('protectPdf')(e.target.checked)} />
                Password-protect the PDFs
              </label>
              {draft.protectPdf && (
                <input
                  type="password"
                  className="qb-input w-[200px]"
                  aria-label="PDF password"
                  placeholder={schedule?.protectPdf ? 'Keep the saved password' : 'Password to open them'}
                  autoComplete="new-password"
                  maxLength={127}
                  value={draft.pdfPassword ?? ''}
                  onChange={(e) => set('pdfPassword')(e.target.value)}
                />
              )}
              {draft.protectPdf && <p className="w-full text-2xs text-qb-ink-faint">The password is kept in this computer’s credential store, not in the company file. Tell recipients the password another way.</p>}
            </div>
          )}
        </fieldset>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={draft.isActive} onChange={(e) => set('isActive')(e.target.checked)} />
          On (untick to pause it)
        </label>
        <p className="text-2xs text-qb-ink-faint">Schedules run only while LocalBooks is open. A run missed while it was closed happens once, soon after LocalBooks opens.</p>
        {error && <Notice kind="error" text={error} />}
        <div className="flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || !draft.name.trim()}>
            Save schedule
          </button>
        </div>
      </form>
    </Dialog>
  );
}

export default function ScheduledReports() {
  const schedules = useLedgerQuery(api.listReportSchedules);
  const choices = useReportChoices();
  const [editing, setEditing] = useState(null); // { schedule } (null = new)
  const [notice, setNotice] = useState(null);
  const nameOf = (r) => choices.find(([key]) => key === keyOf(r))?.[1] ?? 'A report that no longer exists';

  const runNow = async (s) => {
    try {
      setNotice({ kind: 'success', text: await api.runReportScheduleNow(s.id) });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const remove = async (s) => {
    if (!(await askConfirm(`Delete the schedule “${s.name}”? Reports already sent or saved stay where they are.`, { confirmLabel: 'Delete', danger: true }))) return;
    try {
      await api.deleteReportSchedule(s.id);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const list = schedules.data ?? [];
  return (
    <section className="qb-panel flex min-h-0 flex-col" aria-label="Scheduled Reports">
      {editing && (
        <ScheduleDialog
          schedule={editing.schedule}
          onClose={() => setEditing(null)}
          onSaved={(text) => {
            setEditing(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Scheduled Reports</h2>
        <span className="text-xs text-qb-ink-muted">Runs only while LocalBooks is open; a missed run happens when it next opens.</span>
        <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setEditing({ schedule: null })}>
          New schedule
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {schedules.error && <Notice kind="error" text={schedules.error} />}
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head">Schedule</th>
            <th className="qb-grid-head">Reports</th>
            <th className="qb-grid-head w-[120px]">How often</th>
            <th className="qb-grid-head w-[110px]">Next run</th>
            <th className="qb-grid-head">Last run</th>
            <th className="qb-grid-head w-[30px]" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {schedules.data && list.length === 0 && (
            <tr>
              <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">No scheduled reports yet. Click New schedule to send reports to your accountant every month, say.</td>
            </tr>
          )}
          {list.map((s) => (
            <tr key={s.id} className={`qb-grid-row ${s.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setEditing({ schedule: s })}>
              <td className="qb-grid-cell font-semibold">
                {s.name}
                <span className="ml-1 text-2xs font-normal text-qb-ink-muted">{s.destination === 'EMAIL' ? `email ${s.format === 'PDF' ? 'PDF' : 'Excel'}` : `folder · ${s.format === 'PDF' ? 'PDF' : 'Excel'}`}</span>
              </td>
              <td className="qb-grid-cell text-qb-ink-muted" title={s.reports.map(nameOf).join(', ')}>
                {s.reports.map(nameOf).join(', ')}
              </td>
              <td className="qb-grid-cell">{FREQUENCIES.find(([id]) => id === s.frequency)?.[1]}</td>
              <td className="qb-grid-cell">{!s.isActive ? 'Paused' : s.nextRun ? formatDate(s.nextRun) : 'Ended'}</td>
              <td className="qb-grid-cell text-xs text-qb-ink-muted" title={s.lastResult ?? ''}>
                {s.lastResult ?? '—'}
              </td>
              <td className="qb-grid-cell cell-tight text-center">
                <RowMenu
                  label={`Actions for ${s.name}`}
                  items={[
                    { label: 'Edit…', onSelect: () => setEditing({ schedule: s }) },
                    { label: 'Run now', hint: 'For today, without changing the next run', onSelect: () => runNow(s) },
                    { label: 'Delete…', onSelect: () => remove(s) },
                  ]}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
