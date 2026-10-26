import { useEffect, useMemo, useState } from 'react';
import Notice from './Notice.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { ExportMenu, PERIODS, longDate, plainAmount, resolvePeriod, saveText, saveXlsx, toCsv } from './ReportParts.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatQuantity, parseMoneyToCents } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/**
 * The custom report builder (`custom_reports.rs`): the engine's presets in the Reports menu (P&L
 * Detail, Open Invoices, Check Detail, ...), each with a Customize panel for filters, columns and
 * grouping, Save as… (memorized reports), drill-down and the usual exports.
 */

const COLUMNS = [
  ['type', 'Type'], ['date', 'Date'], ['num', 'Num'], ['name', 'Name'], ['memo', 'Memo'], ['account', 'Account'], ['split', 'Split'],
  ['class', 'Class'], ['item', 'Item'], ['qty', 'Qty'], ['rate', 'Rate'], ['debit', 'Debit'], ['credit', 'Credit'], ['amount', 'Amount'],
  ['balance', 'Balance'], ['openBalance', 'Open Balance'], ['dueDate', 'Due Date'], ['aging', 'Aging'], ['rep', 'Rep'],
];
const GROUPS = [
  ['account', 'Account'], ['name', 'Name'], ['item', 'Product/Service'], ['class', 'Class'], ['type', 'Transaction type'], ['rep', 'Rep'],
  ['transaction', 'Transaction'], ['month', 'Month'], ['quarter', 'Quarter'], ['year', 'Year'], ['aging', 'Aging period'],
];
const CLASSIFICATIONS = [['ASSET', 'Assets'], ['LIABILITY', 'Liabilities'], ['EQUITY', 'Equity'], ['INCOME', 'Income'], ['EXPENSE', 'Expenses']];
const RANGE_PERIODS = [
  ...PERIODS.filter((p) => p.id !== 'custom'),
  { id: 'lastYear', label: 'Last calendar year' },
  { id: 'all', label: 'All dates' },
  { id: 'custom', label: 'Custom' },
];
/** Columns a summary row's detail opens with when the summary has none of its own. */
const DRILL_COLUMNS = ['type', 'date', 'num', 'name', 'memo', 'account', 'amount', 'balance'];
/** Settings that belong to the screen, not the engine. */
const SCREEN_FIELDS = ['title', 'dateMode', 'period', 'startDate', 'endDate'];

const engineOnly = (settings) => Object.fromEntries(Object.entries(settings ?? {}).filter(([k]) => !SCREEN_FIELDS.includes(k)));

/** The presets and saved reports, for the Reports menu and the report screen. */
export function useCustomReports() {
  const presets = useLedgerQuery(api.listReportPresets).data ?? [];
  const saved = useLedgerQuery(api.listSavedReports);
  return { presets, saved: saved.data ?? [], reloadSaved: saved.reload };
}

/** The report list with each preset added to its group, and the saved reports as a group of their own. */
export function withCustomReports(groups, { presets, saved }) {
  const out = groups.map((g) => ({ ...g, reports: [...g.reports, ...presets.filter((p) => p.group === g.title).map((p) => [`preset:${p.id}`, p.title])] }));
  for (const p of presets) {
    if (!out.some((g) => g.title === p.group)) out.push({ title: p.group, reports: [[`preset:${p.id}`, p.title]] });
  }
  if (saved.length > 0) out.push({ title: 'Memorized reports', reports: saved.map((s) => [`saved:${s.id}`, s.name]) });
  return out;
}

/** Whether a Reports menu id is one of this screen's. */
export const isCustomReport = (id) => id.startsWith('preset:') || id.startsWith('saved:');

function datesFor(period, fiscal, saved) {
  if (period === 'custom') return [saved?.startDate ?? todayIso(), saved?.endDate ?? todayIso()];
  if (period === 'all') return [null, todayIso()];
  return resolvePeriod(period, fiscal);
}

export default function CustomReport({ report, custom, navigate, onOpen }) {
  const source = useMemo(() => {
    if (report.startsWith('preset:')) {
      const p = custom.presets.find((x) => x.id === report.slice('preset:'.length));
      return p && { preset: p, title: p.title, dateMode: p.dateMode, period: p.period, settings: p.settings };
    }
    const s = custom.saved.find((x) => `saved:${x.id}` === report);
    const preset = s && custom.presets.find((x) => x.id === s.preset);
    return s && { saved: s, preset, title: s.settings.title ?? s.name, dateMode: s.settings.dateMode ?? 'range', period: s.settings.period ?? 'custom', settings: s.settings };
  }, [report, custom.presets, custom.saved]);
  if (!source) return <div className="qb-panel p-2 text-sm text-qb-ink-muted">Loading report…</div>;
  return <Builder source={source} custom={custom} navigate={navigate} onOpen={onOpen} />;
}

function Builder({ source, custom, navigate, onOpen }) {
  const company = useCompany();
  const { data: fiscal } = useLedgerQuery(api.getFiscalPeriods);
  const [settings, setSettings] = useState(() => engineOnly(source.settings));
  const [title, setTitle] = useState(source.title);
  const [period, setPeriod] = useState(source.dateMode === 'asOf' ? 'today' : source.period);
  const [[start, end], setRange] = useState(() => (source.dateMode === 'asOf' ? [null, source.settings.endDate ?? todayIso()] : datesFor(source.period, null, source.settings)));
  const asOf = source.dateMode === 'asOf';
  const [customizing, setCustomizing] = useState(false);
  const [saving, setSaving] = useState(null); // { name, exists }
  const [back, setBack] = useState(null); // the summary a drill-down came from
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (fiscal && !asOf && period !== 'custom') setRange(datesFor(period, fiscal, null));
  }, [fiscal]); // eslint-disable-line react-hooks/exhaustive-deps

  const valid = Boolean(end) && (!start || start <= end);
  const run = { ...settings, startDate: asOf ? null : start, endDate: end };
  const runKey = JSON.stringify(run);
  const { data, error, reload } = useLedgerQuery(() => (valid ? api.getCustomReport(run) : Promise.resolve(null)));
  useEffect(() => {
    reload();
  }, [runKey, reload]); // eslint-disable-line react-hooks/exhaustive-deps

  const subtitle = !data ? '' : asOf ? `As of ${longDate(data.endDate)}` : data.startDate ? (data.startDate === data.endDate ? longDate(data.startDate) : `${longDate(data.startDate)} – ${longDate(data.endDate)}`) : `All dates through ${longDate(data.endDate)}`;
  const filename = title.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'Report';
  const update = (patch) => setSettings((s) => ({ ...s, ...patch }));
  const updateFilters = (patch) => setSettings((s) => ({ ...s, filters: { ...s.filters, ...patch } }));

  const drill = (row) => {
    if (row.journalEntryId) return navigate?.('accounting/journal', 'open-entry', { entryId: row.journalEntryId });
    if (row.kind === 'section' && row.accountId) return navigate?.('accounting/register', 'open', { accountId: row.accountId, startDate: start, endDate: end });
    if (!row.drill) return undefined;
    const d = row.drill;
    const filters = { ...settings.filters };
    for (const key of ['accountIds', 'names', 'itemIds', 'classIds', 'repIds', 'transactionTypes']) if (d[key].length) filters[key] = d[key];
    setBack({ settings, title, range: [start, end], period });
    setSettings({ ...settings, mode: 'detail', columns: settings.columns.length ? settings.columns : DRILL_COLUMNS, groupBy: settings.groupBy, columnBy: null, filters });
    setTitle(`${title}: ${row.cells[0]}`);
    if (d.startDate) {
      setPeriod('custom');
      setRange([d.startDate, d.endDate]);
    }
    return undefined;
  };

  const saveAs = async (replace) => {
    try {
      const saved = await api.saveCustomReport(saving.name, source.preset?.id ?? null, { ...settings, title, dateMode: source.dateMode, period, startDate: asOf ? null : start, endDate: asOf ? null : end }, replace);
      setSaving(null);
      await custom.reloadSaved();
      // Opens the saved report, selected under Memorized reports in the list.
      onOpen?.(`saved:${saved.id}`);
    } catch (err) {
      const text = typeof err === 'string' ? err : String(err?.message ?? err);
      if (text.startsWith('RPT_SAVE_EXISTS')) setSaving((s) => ({ ...s, exists: true }));
      else setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const remove = async () => {
    try {
      await api.deleteSavedReport(source.saved.id);
      await custom.reloadSaved();
      onOpen?.(source.preset ? `preset:${source.preset.id}` : 'pl');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const exports = data && reportExports({ company: company.name, title, subtitle, report: data, filename, onNotice: setNotice });

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {asOf ? (
          <label>
            <span className="qb-label">As of</span>
            <input type="date" className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => setRange([null, e.target.value])} />
          </label>
        ) : (
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
              <input type="date" className="qb-input w-[132px]" value={end ?? ''} onChange={(e) => { setPeriod('custom'); setRange([start, e.target.value]); }} aria-invalid={!valid} />
            </label>
          </>
        )}
        <button type="button" className={`qb-btn h-row ${customizing ? 'qb-btn-primary' : 'qb-btn-secondary'}`} aria-expanded={customizing} onClick={() => setCustomizing((c) => !c)}>
          Customize…
        </button>
        <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setSaving({ name: source.saved?.name ?? title, exists: false })}>
          Save as…
        </button>
        {back && (
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row"
            onClick={() => {
              setSettings(back.settings);
              setTitle(back.title);
              setPeriod(back.period);
              setRange(back.range);
              setBack(null);
            }}
          >
            ‹ Back to summary
          </button>
        )}
        {source.saved && <DeleteSaved onDelete={remove} />}
        <div className="ml-auto">{exports && <ExportMenu onNotice={setNotice} onCsv={exports.onCsv} onXlsx={exports.onXlsx} />}</div>
      </div>
      {saving && (
        <form
          className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1"
          onSubmit={(e) => {
            e.preventDefault();
            saveAs(false);
          }}
        >
          <label>
            <span className="qb-label">Save this report as</span>
            <input className="qb-input w-[260px]" autoFocus maxLength={100} value={saving.name} onChange={(e) => setSaving({ name: e.target.value, exists: false })} />
          </label>
          {saving.exists ? (
            <>
              <span className="self-center text-sm">A saved report has this name.</span>
              <button type="button" className="qb-btn qb-btn-primary h-row" onClick={() => saveAs(true)}>Replace it</button>
            </>
          ) : (
            <button type="submit" className="qb-btn qb-btn-primary h-row" disabled={!saving.name.trim()}>Save</button>
          )}
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setSaving(null)}>Cancel</button>
          <span className="self-center text-xs text-qb-ink-muted">Saves the filters, columns, grouping and report period, not the numbers.</span>
        </form>
      )}
      {customizing && (
        <CustomizePanel
          settings={settings}
          title={title}
          onTitle={setTitle}
          update={update}
          updateFilters={updateFilters}
          onReset={source.preset ? () => { setSettings(engineOnly(source.preset.settings)); setTitle(source.preset.title); } : null}
          onClose={() => setCustomizing(false)}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {data && <ReportTable company={company} title={title} subtitle={subtitle} report={data} onRow={drill} />}
    </>
  );
}

function DeleteSaved({ onDelete }) {
  const [sure, setSure] = useState(false);
  return sure ? (
    <span className="flex items-end gap-1">
      <button type="button" className="qb-btn qb-btn-primary h-row" onClick={onDelete}>Delete saved report</button>
      <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setSure(false)}>Keep it</button>
    </span>
  ) : (
    <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setSure(true)}>Delete…</button>
  );
}

// ------------------------------------------------------------------------------------------------
// The report
// ------------------------------------------------------------------------------------------------

const RIGHT = new Set(['money', 'quantity', 'number']);

function screenCell(kind, v) {
  if (v === null || v === undefined) return '';
  if (kind === 'money') return formatCents(v);
  if (kind === 'quantity') return formatQuantity(v);
  if (kind === 'date') return formatDate(v);
  return String(v);
}

/** CSV and Excel from the engine's typed cells: money as cents, quantities as numbers, dates as dates. */
export function reportExports({ company, title, subtitle, report, filename, onNotice }) {
  const csvCell = (kind, v) => {
    if (v === null || v === undefined) return '';
    if (kind === 'money') return plainAmount(v);
    if (kind === 'quantity') return String(v / 10_000);
    return String(v);
  };
  const header = report.columns.map((c) => c.label);
  return {
    onCsv: () =>
      saveText(
        toCsv([
          [company],
          [title],
          [subtitle],
          [],
          header,
          ...report.rows.map((r) => r.cells.map((v, i) => (i === 0 && r.depth && v ? '  '.repeat(r.depth) : '') + csvCell(report.columns[i].kind, v))),
        ]),
        `${filename}.csv`,
        onNotice,
      ),
    onXlsx: () =>
      saveXlsx(
        {
          company,
          title,
          subtitle,
          notes: [],
          columns: report.columns.map((c) => ({ label: c.label, kind: c.kind === 'quantity' ? 'number' : c.kind })),
          rows: report.rows.map((r) => ({
            kind: r.kind,
            depth: Math.min(r.depth, 15),
            cells: r.cells.map((v, i) => (v !== null && report.columns[i].kind === 'quantity' ? v / 10_000 : v)),
          })),
        },
        `${filename}.xlsx`,
        onNotice,
      ),
  };
}

function ReportTable({ company, title, subtitle, report, onRow }) {
  const { columns, rows } = report;
  const rowClass = (kind) =>
    kind === 'section' ? 'font-semibold bg-qb-canvas' : kind === 'subtotal' ? 'font-semibold border-t border-qb-ink' : kind === 'total' ? 'font-semibold border-t-2 border-double border-qb-ink' : '';
  const clickable = (r) => Boolean(r.journalEntryId || r.drill || (r.kind === 'section' && r.accountId));
  const hint = (r) => (r.journalEntryId ? 'Open this transaction' : r.drill ? 'Show these transactions' : r.accountId ? 'Open the register for this account' : undefined);
  const cells = (r, cellClass) =>
    r.kind === 'section' ? (
      <td colSpan={columns.length} className={cellClass(0)}>{r.cells[0]}</td>
    ) : (
      r.cells.map((v, i) => (
        <td key={i} className={cellClass(i)} style={i === 0 && r.depth ? { paddingLeft: `${6 + r.depth * 12}px` } : undefined}>
          {screenCell(columns[i].kind, v)}
        </td>
      ))
    );
  return (
    <>
      <PrintPortal>
        <article className="doc">
          <DocHeader company={company} title={title} subtitle={subtitle} />
          <table className="doc-table">
            <thead>
              <tr>
                {columns.map((c, i) => (
                  <th key={i} className={RIGHT.has(c.kind) ? 'doc-num' : ''}>{c.label}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i} className={r.kind === 'section' ? 'doc-line-section' : r.kind === 'detail' ? '' : 'doc-line-total'}>
                  {cells(r, (c) => (RIGHT.has(columns[c].kind) ? 'doc-num' : ''))}
                </tr>
              ))}
            </tbody>
          </table>
        </article>
      </PrintPortal>
      <article className="mx-auto w-full max-w-[1400px] overflow-x-auto bg-white px-6 py-4 shadow-panel">
        <header className="mb-3 border-b-[3px] border-double border-qb-ink pb-2">
          <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
          <h2 className="text-xl font-semibold">{title}</h2>
          <div className="text-sm text-qb-ink-muted">{subtitle}</div>
        </header>
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-qb-border-strong">
              {columns.map((c, i) => (
                <th key={i} className={`whitespace-nowrap px-1.5 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted ${RIGHT.has(c.kind) ? 'text-right' : 'text-left'}`}>{c.label}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {report.lineCount === 0 && (
              <tr>
                <td colSpan={columns.length} className="px-1.5 py-1 text-qb-ink-muted">No transactions match this report’s dates and filters.</td>
              </tr>
            )}
            {rows.map((r, i) => (
              <tr key={i} className={`${rowClass(r.kind)} ${clickable(r) ? 'cursor-pointer hover:bg-qb-row-hover' : ''}`} onClick={clickable(r) ? () => onRow(r) : undefined} title={hint(r)}>
                {cells(r, (c) => `px-1.5 py-0.5 ${RIGHT.has(columns[c].kind) ? 'num whitespace-nowrap text-right' : ''} ${c === 0 && r.kind !== 'detail' ? 'whitespace-nowrap' : ''}`)}
              </tr>
            ))}
          </tbody>
        </table>
        <footer className="mt-3 border-t border-qb-border pt-1 text-xs text-qb-ink-muted">
          {report.lineCount} {report.lineCount === 1 ? 'line' : 'lines'} · click a line to open its transaction · Customize… changes the filters, columns and grouping
        </footer>
      </article>
    </>
  );
}

// ------------------------------------------------------------------------------------------------
// Customize
// ------------------------------------------------------------------------------------------------

/** A checkbox list with a search box; `value` holds the chosen options' values. */
function MultiPick({ label, options, value, onChange, keyOf = String }) {
  const [q, setQ] = useState('');
  const chosen = new Set(value.map(keyOf));
  const shown = options.filter((o) => chosen.has(keyOf(o.value)) || o.label.toLowerCase().includes(q.toLowerCase()));
  return (
    <fieldset className="min-w-0">
      <legend className="qb-label">
        {label}
        {value.length > 0 && (
          <>
            {' '}({value.length}){' '}
            <button type="button" className="text-qb-info hover:underline" onClick={() => onChange([])}>clear</button>
          </>
        )}
      </legend>
      {options.length > 8 && <input className="qb-input mb-0.5 w-full" placeholder="Search…" aria-label={`Search ${label}`} value={q} onChange={(e) => setQ(e.target.value)} />}
      <div className="h-[92px] overflow-y-auto rounded-xs border border-qb-border bg-white px-1 scrollbar-thin">
        {shown.length === 0 && <div className="text-xs text-qb-ink-muted">None</div>}
        {shown.map((o) => (
          <label key={keyOf(o.value)} className="flex items-center gap-1 text-sm">
            <input
              type="checkbox"
              className="accent-qb-green"
              checked={chosen.has(keyOf(o.value))}
              onChange={(e) => onChange(e.target.checked ? [...value, o.value] : value.filter((v) => keyOf(v) !== keyOf(o.value)))}
            />
            <span className="truncate">{o.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

function MoneyBound({ label, cents, onChange }) {
  const [text, setText] = useState(cents === null || cents === undefined ? '' : centsToInput(cents));
  const parsed = text.trim() === '' ? null : parseMoneyToCents(text);
  return (
    <label>
      <span className="qb-label">{label}</span>
      <input
        className="qb-input w-[100px] text-right"
        value={text}
        aria-invalid={text.trim() !== '' && parsed === null}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => (text.trim() === '' || parsed !== null) && onChange(parsed === null ? null : Math.abs(parsed))}
      />
    </label>
  );
}

const nameKey = (n) => `${n.kind}:${n.id}`;

function CustomizePanel({ settings, title, onTitle, update, updateFilters, onReset, onClose }) {
  const f = settings.filters ?? {};
  const accounts = useLedgerQuery(api.listAccounts).data ?? [];
  const types = useLedgerQuery(api.listAccountTypes).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const vendors = useLedgerQuery(api.listVendors).data ?? [];
  const classes = useLedgerQuery(api.listClasses).data ?? [];
  const products = useLedgerQuery(api.listProducts).data ?? [];
  const reps = useLedgerQuery(api.listSalesReps).data ?? [];
  const detail = settings.mode === 'detail';
  const names = [
    ...(f.nameKind === 'vendor' ? [] : customers.map((c) => ({ value: { kind: 'customer', id: c.id }, label: `${c.name} (customer)` }))),
    ...(f.nameKind === 'customer' ? [] : vendors.map((v) => ({ value: { kind: 'vendor', id: v.id }, label: `${v.name} (vendor)` }))),
  ];
  const checkbox = (label, checked, onChange) => (
    <label className="flex items-center gap-1 text-sm">
      <input type="checkbox" className="accent-qb-green" checked={Boolean(checked)} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
  const select = (label, value, options, onChange, width = 'w-[170px]') => (
    <label>
      <span className="qb-label">{label}</span>
      <select className={`qb-input ${width}`} value={value ?? ''} onChange={(e) => onChange(e.target.value || null)}>
        {options.map(([v, l]) => (
          <option key={v} value={v}>{l}</option>
        ))}
      </select>
    </label>
  );
  const toggleColumn = (id, on) => update({ columns: on ? [...settings.columns, id] : settings.columns.filter((c) => c !== id) });
  const move = (i, by) => {
    const cols = [...settings.columns];
    const j = i + by;
    if (j < 0 || j >= cols.length) return;
    [cols[i], cols[j]] = [cols[j], cols[i]];
    update({ columns: cols });
  };
  const label = Object.fromEntries(COLUMNS);

  return (
    <section className="qb-panel no-print flex-none px-2 py-1.5" aria-label="Customize report">
      <div className="mb-1 flex items-center gap-2">
        <h3 className="text-sm font-semibold">Customize report</h3>
        <label className="flex items-center gap-1">
          <span className="qb-label mb-0">Title</span>
          <input className="qb-input w-[280px]" value={title} onChange={(e) => onTitle(e.target.value)} />
        </label>
        <div className="ml-auto flex gap-1">
          {onReset && <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={onReset}>Reset to preset</button>}
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={onClose}>Done</button>
        </div>
      </div>
      <div className="grid grid-cols-1 gap-x-4 gap-y-1.5 md:grid-cols-[260px_1fr]">
        <div className="flex flex-col gap-1.5">
          <div className="text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Display</div>
          <div className="flex gap-3">
            {[['detail', 'Detail (one row per line)'], ['summary', 'Summary (one row per group)']].map(([v, l]) => (
              <label key={v} className="flex items-center gap-1 text-sm">
                <input type="radio" name="report-mode" className="accent-qb-green" checked={settings.mode === v} onChange={() => update({ mode: v, columns: v === 'detail' && settings.columns.length === 0 ? DRILL_COLUMNS : settings.columns, groupBy: settings.groupBy ?? (v === 'summary' ? 'account' : null) })} />
                {l}
              </label>
            ))}
          </div>
          {select(detail ? 'Subtotal by' : 'Rows', settings.groupBy, [...(detail ? [['', 'None']] : []), ...GROUPS], (v) => update({ groupBy: v }))}
          {!detail && select('Columns over time', settings.columnBy, [['', 'Total only'], ['month', 'Month'], ['quarter', 'Quarter'], ['year', 'Year']], (v) => update({ columnBy: v }))}
          {select('Amount', settings.sign, [['natural', 'Normal balance side'], ['debit', 'Debits minus credits'], ['credit', 'Credits minus debits']], (v) => update({ sign: v ?? 'natural' }))}
          {detail && settings.groupBy === 'account' && checkbox('Beginning balance for each account', settings.openingBalances, (v) => update({ openingBalances: v }))}
          {detail && checkbox('One row per transaction', settings.perTransaction, (v) => update({ perTransaction: v }))}
          {!detail && checkbox('Show quantity', settings.showQty, (v) => update({ showQty: v }))}
          {!detail && checkbox('Show rows that total zero', settings.showZeroRows, (v) => update({ showZeroRows: v }))}
          {detail && (
            <fieldset>
              <legend className="qb-label">Columns, in order</legend>
              <div className="grid grid-cols-2 gap-x-2">
                {settings.columns.map((id, i) => (
                  <div key={id} className="col-span-2 flex items-center gap-1 text-sm">
                    <input type="checkbox" className="accent-qb-green" checked onChange={() => toggleColumn(id, false)} aria-label={`Hide ${label[id]}`} />
                    <span className="flex-1">{label[id]}</span>
                    <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-ink" aria-label={`Move ${label[id]} left`} onClick={() => move(i, -1)}>↑</button>
                    <button type="button" className="px-1 text-qb-ink-muted hover:text-qb-ink" aria-label={`Move ${label[id]} right`} onClick={() => move(i, 1)}>↓</button>
                  </div>
                ))}
                {COLUMNS.filter(([id]) => !settings.columns.includes(id)).map(([id, l]) => (
                  <label key={id} className="flex items-center gap-1 text-sm text-qb-ink-muted">
                    <input type="checkbox" className="accent-qb-green" checked={false} onChange={() => toggleColumn(id, true)} />
                    {l}
                  </label>
                ))}
              </div>
            </fieldset>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-1.5">
          <div className="text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Filters</div>
          <div className="flex flex-wrap items-end gap-2">
            {select('Lines', settings.source, [['ledger', 'Transaction lines (every account)'], ['items', 'Product & service lines of sales and purchases']], (v) => update({ source: v ?? 'ledger' }), 'w-[300px]')}
            {select('Names', f.nameKind, [['', 'Any name'], ['customer', 'Customers only'], ['vendor', 'Vendors only']], (v) => updateFilters({ nameKind: v }), 'w-[140px]')}
            {select('Paid status', f.paidStatus, [['', 'Any'], ['open', 'Open (balance left)'], ['paid', 'Paid in full']], (v) => updateFilters({ paidStatus: v }), 'w-[150px]')}
            {select('Cleared', f.cleared, [['', 'Any'], ['cleared', 'Cleared'], ['uncleared', 'Not cleared'], ['reconciled', 'Reconciled']], (v) => updateFilters({ cleared: v }), 'w-[120px]')}
            <MoneyBound label="Amount from" cents={f.amountMinCents} onChange={(v) => updateFilters({ amountMinCents: v })} />
            <MoneyBound label="to" cents={f.amountMaxCents} onChange={(v) => updateFilters({ amountMaxCents: v })} />
            <label>
              <span className="qb-label">Memo contains</span>
              <input className="qb-input w-[160px]" value={f.memo ?? ''} onChange={(e) => updateFilters({ memo: e.target.value || null })} />
            </label>
          </div>
          <div className="flex flex-wrap gap-3">
            {checkbox('1099 vendors only', f.vendors1099, (v) => updateFilters({ vendors1099: v }))}
            {checkbox('Lines with a product or service only', f.itemsOnly, (v) => updateFilters({ itemsOnly: v }))}
            {checkbox('Leave out voided transactions', f.excludeVoided, (v) => updateFilters({ excludeVoided: v }))}
            {checkbox('Lines without a class', f.unclassified, (v) => updateFilters({ unclassified: v }))}
          </div>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-4">
            <MultiPick label="Accounts (with subaccounts)" options={accounts.map((a) => ({ value: a.id, label: `${a.accountNumber ? `${a.accountNumber} ` : ''}${a.fullName}` }))} value={f.accountIds ?? []} onChange={(v) => updateFilters({ accountIds: v })} />
            <MultiPick label="Account types" options={[...CLASSIFICATIONS.map(([c, l]) => ({ value: `class:${c}`, label: `All ${l.toLowerCase()}` })), ...types.map((t) => ({ value: t.code, label: t.displayName }))]} value={[...(f.classifications ?? []).map((c) => `class:${c}`), ...(f.accountTypes ?? [])]} onChange={(v) => updateFilters({ classifications: v.filter((x) => x.startsWith('class:')).map((x) => x.slice(6)), accountTypes: v.filter((x) => !x.startsWith('class:')) })} />
            <MultiPick label="Transaction types" options={Object.entries(SOURCE_LABEL).map(([code, l]) => ({ value: code, label: l }))} value={f.transactionTypes ?? []} onChange={(v) => updateFilters({ transactionTypes: v })} />
            <MultiPick label="Customers and vendors" options={names} keyOf={(v) => (typeof v === 'object' ? nameKey(v) : String(v))} value={f.names ?? []} onChange={(v) => updateFilters({ names: v })} />
            <MultiPick label="Classes (with subclasses)" options={classes.map((c) => ({ value: c.id, label: c.fullName }))} value={f.classIds ?? []} onChange={(v) => updateFilters({ classIds: v })} />
            <MultiPick label="Products and services" options={products.map((p) => ({ value: p.id, label: p.name }))} value={f.itemIds ?? []} onChange={(v) => updateFilters({ itemIds: v })} />
            <MultiPick label="Reps" options={reps.map((r) => ({ value: r.id, label: r.name ? `${r.initials} (${r.name})` : r.initials }))} value={f.repIds ?? []} onChange={(v) => updateFilters({ repIds: v })} />
          </div>
        </div>
      </div>
    </section>
  );
}
