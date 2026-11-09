import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import Notice from './Notice.jsx';

/** What a sheet can hold: [kind, name, one record, several]. */
const GROUPS = [
  ['Lists', [['CUSTOMERS', 'Customers', 'customer', 'customers'], ['VENDORS', 'Vendors', 'vendor', 'vendors']]],
  [
    'Sales',
    [
      ['INVOICES', 'Invoices', 'invoice', 'invoices'],
      ['SALES_RECEIPTS', 'Sales receipts', 'sales receipt', 'sales receipts'],
      ['CREDIT_MEMOS', 'Credit memos', 'credit memo', 'credit memos'],
      ['DEPOSITS', 'Deposits', 'deposit', 'deposits'],
    ],
  ],
  [
    'Purchases',
    [
      ['BILLS', 'Bills', 'bill', 'bills'],
      ['BILL_CREDITS', 'Bill credits', 'bill credit', 'bill credits'],
      ['CHECKS', 'Checks', 'check', 'checks'],
      ['CARD_CHARGES', 'Credit card charges', 'card charge', 'card charges'],
      ['CARD_CREDITS', 'Credit card credits', 'card credit', 'card credits'],
    ],
  ],
  ['Accountant', [['JOURNAL_ENTRIES', 'Journal entries', 'journal entry', 'journal entries']]],
];
const KINDS = GROUPS.flatMap(([, kinds]) => kinds);
const LISTS = ['CUSTOMERS', 'VENDORS'];
const SHEET_EXTENSIONS = ['csv', 'txt', 'xlsx', 'xls', 'xlsm', 'ods'];

/** What a clean check found, in words. */
function readyText(kind, c) {
  const [, , one, many] = KINDS.find(([k]) => k === kind);
  const counted = `${c.records.toLocaleString()} ${c.records === 1 ? one : many}`;
  if (LISTS.includes(kind)) return `${counted}.`;
  return `${counted} from ${c.rows.toLocaleString()} rows, ${formatCents(c.totalCents)} in total${kind === 'JOURNAL_ENTRIES' ? ' of debits' : ''}.`;
}

/**
 * Settings › Import & Migration › From a spreadsheet: pick what the sheet holds, choose the file,
 * match its columns to fields, check every row, then stage it for the same review as any import.
 * `onStage(path, stage)` starts the read the wizard follows; `stage` returns the staged batch.
 */
export default function SheetImport({ busy, onStage }) {
  const [expanded, setExpanded] = useState(false);
  const [kind, setKind] = useState('CUSTOMERS');
  const [path, setPath] = useState(null);
  const [sheet, setSheet] = useState(null);
  const [mapping, setMapping] = useState({});
  const [dateOrder, setDateOrder] = useState('');
  const [check, setCheck] = useState(null);
  const [error, setError] = useState(null);
  const [working, setWorking] = useState(false);
  const disabled = busy || working;

  const attempt = async (fn) => {
    setWorking(true);
    setError(null);
    try {
      return await fn();
    } catch (err) {
      setError(errorText(err));
      return undefined;
    } finally {
      setWorking(false);
    }
  };

  const load = async (file, k) => {
    setCheck(null);
    const s = await attempt(() => api.readImportSheet(file, k));
    if (!s) return;
    setPath(file);
    setSheet(s);
    setMapping(s.mapping);
  };

  const choose = async () => {
    const picked = await open({ multiple: false, directory: false, filters: [{ name: 'Spreadsheet (.csv, .xlsx, .xls, .ods)', extensions: SHEET_EXTENSIONS.flatMap((e) => [e, e.toUpperCase()]) }] });
    if (typeof picked === 'string') load(picked, kind);
  };

  const changeKind = (k) => {
    setKind(k);
    if (path) load(path, k);
  };

  const setField = (id, value) => {
    setCheck(null);
    setMapping((m) => {
      const next = { ...m };
      if (value === '') delete next[id];
      else next[id] = Number(value);
      return next;
    });
  };

  const order = dateOrder || null;
  const runCheck = async () => {
    const c = await attempt(() => api.checkImportSheet(path, kind, mapping, order));
    if (c) setCheck(c);
  };

  const template = async () => {
    const saved = await attempt(() => api.saveImportTemplate(kind));
    if (saved) setError(null);
  };

  const clean = check && check.problems.length === 0 && check.records > 0;
  const firstRow = sheet?.preview[0]?.[1] ?? [];

  return (
    <div className="border-t border-qb-border px-3 py-2">
      <button type="button" className="flex w-full items-center gap-2 text-left text-sm font-semibold" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
        <span aria-hidden="true">{expanded ? '▾' : '▸'}</span>
        From a spreadsheet you laid out: customers, vendors, transactions or journal entries
      </button>
      {expanded && (
        <div className="mt-2 flex flex-col gap-2">
          <p className="text-xs text-qb-ink-muted">
            Choose what the sheet holds, then the .csv or Excel file. Match its columns to LocalBooks’ fields; LocalBooks checks every row and lists the ones that would fail, so
            you can fix the file before anything is staged. Nothing is added to your books until you post it in the review below. No sheet yet? Download the template: its
            columns match without any changes.
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-2 text-sm">
              <span className="font-semibold">The sheet holds</span>
              <select className="qb-input h-row" value={kind} disabled={disabled} onChange={(e) => changeKind(e.target.value)}>
                {GROUPS.map(([group, kinds]) => (
                  <optgroup key={group} label={group}>
                    {kinds.map(([k, label]) => (
                      <option key={k} value={k}>
                        {label}
                      </option>
                    ))}
                  </optgroup>
                ))}
              </select>
            </label>
            <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4 text-md" disabled={disabled} onClick={choose}>
              Choose a spreadsheet…
            </button>
            <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={disabled} onClick={template}>
              Download the {KINDS.find(([k]) => k === kind)[3]} template
            </button>
          </div>
          {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
          {sheet && (
            <>
              <p className="text-sm">
                <span className="font-semibold">{sheet.fileName}</span>
                <span className="text-qb-ink-muted">
                  {' '}
                  · {sheet.rowCount.toLocaleString()} row{sheet.rowCount === 1 ? '' : 's'} under the headings
                </span>
              </p>
              <table className="qb-grid" aria-label="Match columns to fields">
                <colgroup>
                  <col className="w-[200px]" />
                  <col className="w-[260px]" />
                  <col />
                </colgroup>
                <thead>
                  <tr>
                    <th className="qb-grid-head">Field</th>
                    <th className="qb-grid-head">Column in the sheet</th>
                    <th className="qb-grid-head">First row</th>
                  </tr>
                </thead>
                <tbody>
                  {sheet.fields.map((f) => (
                    <tr key={f.id}>
                      <td className="qb-grid-cell">
                        {f.label}
                        {f.required && <span className="text-qb-ink-muted"> (required)</span>}
                      </td>
                      <td className="qb-grid-cell">
                        <select className="qb-input h-row w-full" aria-label={`Column for ${f.label}`} value={mapping[f.id] ?? ''} disabled={disabled} onChange={(e) => setField(f.id, e.target.value)}>
                          <option value="">Not in the sheet</option>
                          {sheet.headers.map((h, i) => (
                            <option key={i} value={i}>
                              {h || `Column ${i + 1}`}
                            </option>
                          ))}
                        </select>
                      </td>
                      <td className="qb-grid-cell truncate text-qb-ink-muted">{mapping[f.id] == null ? '' : (firstRow[mapping[f.id]] ?? '')}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {!LISTS.includes(kind) && (
                <label className="flex items-center gap-2 text-xs">
                  <span className="font-semibold text-qb-ink-muted">Dates like 03/04/2026 are</span>
                  <select className="qb-input h-row" value={dateOrder} disabled={disabled} onChange={(e) => (setDateOrder(e.target.value), setCheck(null))}>
                    <option value="">Worked out from the sheet</option>
                    <option value="MDY">Month first (March 4)</option>
                    <option value="DMY">Day first (3 April)</option>
                  </select>
                </label>
              )}
              <div className="flex items-center gap-2">
                <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-4" disabled={disabled} onClick={runCheck}>
                  Check the rows
                </button>
                {clean && (
                  <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={disabled} onClick={() => onStage(path, () => api.stageImportSheet(path, kind, mapping, order))}>
                    Continue to the review
                  </button>
                )}
              </div>
              {check && clean && <Notice kind="success" text={`Every row reads. ${readyText(kind, check)}`} />}
              {check && !clean && check.problems.length === 0 && <Notice kind="info" text="The sheet has no rows to import." />}
              {check && check.problems.length > 0 && (
                <div role="alert" className="border border-qb-border">
                  <p className="border-b border-qb-border bg-qb-canvas px-2 py-1 text-xs font-semibold">
                    {check.problems.length.toLocaleString()} problem{check.problems.length === 1 ? '' : 's'} to fix in the file or the matching above, then check again. Nothing has been
                    staged.
                  </p>
                  <ul className="max-h-60 overflow-auto text-sm scrollbar-thin">
                    {check.problems.map((p, i) => (
                      <li key={i} className="flex gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
                        <span className="w-16 flex-none tabular-nums text-qb-ink-muted">{p.row ? `Row ${p.row}` : 'Matching'}</span>
                        <span>{p.message}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
