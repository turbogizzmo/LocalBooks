import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';

const EXTENSIONS = ['csv', 'txt', 'xlsx', 'xls', 'xlsm', 'ods'];
const HOW = { chosen: 'chosen here', saved: 'as last time', matched: 'same name', none: '' };

/**
 * Settings › Import & Migration › Payroll (`payroll_import.rs`): a payroll service's journal
 * export becomes journal entries. Choose the file, match its columns and its account names, look
 * over every entry, then post them all at once. Nothing posts while a row has a problem.
 */
export default function PayrollImport({ busy = false }) {
  const [expanded, setExpanded] = useState(false);
  const [path, setPath] = useState(null);
  const [preview, setPreview] = useState(null);
  const [choices, setChoices] = useState({ mapping: null, dateOrder: null, accounts: {} });
  const [error, setError] = useState(null);
  const [done, setDone] = useState(null);
  const [running, setWorking] = useState(false);
  const working = busy || running;
  const { data: accounts } = useLedgerQuery(api.listAccounts);
  const { data: history, reload: reloadHistory } = useLedgerQuery(api.listPayrollImports);

  const run = async (file, next) => {
    setWorking(true);
    setError(null);
    try {
      setPreview(await api.previewPayrollImport(file, next));
      setChoices(next);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setWorking(false);
    }
  };

  const choose = async () => {
    const picked = await open({ multiple: false, directory: false, filters: [{ name: 'Payroll journal (.csv, .xlsx)', extensions: EXTENSIONS.flatMap((e) => [e, e.toUpperCase()]) }] });
    if (typeof picked !== 'string') return;
    setPath(picked);
    setDone(null);
    run(picked, { mapping: null, dateOrder: null, accounts: {} });
  };

  const setColumn = (id, value) => {
    const mapping = { ...(choices.mapping ?? preview.mapping) };
    if (value === '') delete mapping[id];
    else mapping[id] = Number(value);
    run(path, { ...choices, mapping });
  };
  const setAccount = (name, id) => {
    const next = { ...choices.accounts };
    if (id == null) delete next[name];
    else next[name] = id;
    run(path, { ...choices, mapping: choices.mapping ?? preview.mapping, accounts: next });
  };

  const post = async () => {
    setWorking(true);
    setError(null);
    try {
      const posted = await api.postPayrollImport(path, { ...choices, mapping: choices.mapping ?? preview.mapping });
      setDone(`Posted ${posted.journalEntryIds.length} payroll entr${posted.journalEntryIds.length === 1 ? 'y' : 'ies'}, ${formatCents(posted.totalCents)} in debits. Void an entry in the journal to undo it.`);
      setPreview(null);
      setPath(null);
      reloadHistory();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setWorking(false);
    }
  };

  const mapping = preview ? (choices.mapping ?? preview.mapping) : {};
  const ready = preview && preview.problems.length === 0 && preview.entries.length > 0 && !preview.alreadyImported;
  const names = new Map((accounts ?? []).map((a) => [a.id, a.fullName]));
  const accountName = (id) => names.get(id) ?? '';

  return (
    <div className="border-t border-qb-border px-3 py-2">
      <button type="button" className="flex w-full items-center gap-2 text-left text-sm font-semibold" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
        <span aria-hidden="true">{expanded ? '▾' : '▸'}</span>
        Payroll: a payroll service’s journal export
      </button>
      {expanded && (
        <div className="mt-2 flex flex-col gap-2">
          <p className="text-xs text-qb-ink-muted">
            Export each payroll from your payroll service as a journal (a .csv or Excel file with a date, an account, and a debit and credit or one amount). Tillhouse groups the rows
            into one entry per payroll, matches the service’s account names to yours (and remembers them for next time), and posts only when every entry balances.
          </p>
          <div className="flex items-center gap-2">
            <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4 text-md" disabled={working} onClick={choose}>
              Choose a payroll export…
            </button>
          </div>
          {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
          {done && <Notice kind="success" text={done} onDismiss={() => setDone(null)} />}
          {preview && (
            <>
              <p className="text-sm">
                <span className="font-semibold">{preview.fileName}</span>
                <span className="text-qb-ink-muted">
                  {' '}
                  · {preview.entries.length} entr{preview.entries.length === 1 ? 'y' : 'ies'}, {formatCents(preview.totalCents)} in debits
                </span>
              </p>
              {preview.alreadyImported && <Notice kind="error" text={`This file was already imported on ${preview.alreadyImported.slice(0, 10)}. Void those entries first to import it again.`} />}
              <table className="qb-grid" aria-label="Match columns">
                <thead>
                  <tr>
                    <th className="qb-grid-head w-[160px]">Field</th>
                    <th className="qb-grid-head">Column in the file</th>
                  </tr>
                </thead>
                <tbody>
                  {preview.fields.map((f) => (
                    <tr key={f.id}>
                      <td className="qb-grid-cell">
                        {f.label}
                        {f.required && <span className="text-qb-ink-muted"> (required)</span>}
                      </td>
                      <td className="qb-grid-cell">
                        <select className="qb-input h-row w-full" aria-label={`Column for ${f.label}`} value={mapping[f.id] ?? ''} disabled={working} onChange={(e) => setColumn(f.id, e.target.value)}>
                          <option value="">Not in the file</option>
                          {preview.headers.map((h, i) => (
                            <option key={i} value={i}>{h || `Column ${i + 1}`}</option>
                          ))}
                        </select>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <label className="flex items-center gap-2 text-xs">
                <span className="font-semibold text-qb-ink-muted">Dates like 03/04/2026 are</span>
                <select className="qb-input h-row" value={choices.dateOrder ?? ''} disabled={working} onChange={(e) => run(path, { ...choices, mapping, dateOrder: e.target.value || null })}>
                  <option value="">Worked out from the file</option>
                  <option value="MDY">Month first (March 4)</option>
                  <option value="DMY">Day first (3 April)</option>
                </select>
              </label>
              {preview.sourceAccounts.length > 0 && (
                <table className="qb-grid" aria-label="Match accounts">
                  <thead>
                    <tr>
                      <th className="qb-grid-head">Account in the file</th>
                      <th className="qb-grid-head w-[320px]">Tillhouse account</th>
                      <th className="qb-grid-head w-[120px]">Matched</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preview.sourceAccounts.map((s) => (
                      <tr key={s.name}>
                        <td className="qb-grid-cell">{s.name}</td>
                        <td className="qb-grid-cell">
                          <AccountSelect accounts={accounts ?? []} value={s.accountId} placeholder="Choose an account…" className="qb-input h-row w-full" aria-label={`Tillhouse account for ${s.name}`} disabled={working} onChange={(id) => setAccount(s.name, id)} />
                        </td>
                        <td className="qb-grid-cell text-qb-ink-muted">{HOW[s.how]}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {preview.problems.length > 0 && (
                <div role="alert" className="border border-qb-border">
                  <p className="border-b border-qb-border bg-qb-canvas px-2 py-1 text-xs font-semibold">
                    {preview.problems.length} problem{preview.problems.length === 1 ? '' : 's'} to fix in the file or the matching above. Nothing has been posted.
                  </p>
                  <ul className="max-h-48 overflow-auto text-sm scrollbar-thin">
                    {preview.problems.map((p, i) => (
                      <li key={i} className="flex gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
                        <span className="w-16 flex-none tabular-nums text-qb-ink-muted">{p.row ? `Row ${p.row}` : 'Matching'}</span>
                        <span>{p.message}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {preview.entries.length > 0 && (
                <div className="max-h-80 overflow-auto border border-qb-border scrollbar-thin">
                  <table className="qb-grid" aria-label="Entries to post">
                    <thead>
                      <tr>
                        <th className="qb-grid-head w-[110px]">Date</th>
                        <th className="qb-grid-head">Account</th>
                        <th className="qb-grid-head">Memo</th>
                        <th className="qb-grid-head w-[120px] text-right">Debit</th>
                        <th className="qb-grid-head w-[120px] text-right">Credit</th>
                      </tr>
                    </thead>
                    {preview.entries.map((e) => (
                      <tbody key={e.key}>
                        <tr className="bg-qb-canvas">
                          <td className="qb-grid-cell font-semibold" colSpan={3}>{e.description}</td>
                          <td className={`qb-grid-cell text-right tabular-nums font-semibold ${e.debitCents === e.creditCents ? '' : 'text-qb-negative'}`}>{formatCents(e.debitCents)}</td>
                          <td className={`qb-grid-cell text-right tabular-nums font-semibold ${e.debitCents === e.creditCents ? '' : 'text-qb-negative'}`}>{formatCents(e.creditCents)}</td>
                        </tr>
                        {e.lines.map((l) => (
                          <tr key={l.row} className="qb-grid-row">
                            <td className="qb-grid-cell tabular-nums text-qb-ink-muted">{e.date}</td>
                            <td className="qb-grid-cell">{l.accountId ? accountName(l.accountId) : <span className="text-qb-negative">{l.sourceAccount} (not matched)</span>}</td>
                            <td className="qb-grid-cell text-qb-ink-muted">{l.memo ?? ''}</td>
                            <td className="qb-grid-cell text-right tabular-nums">{l.debitCents ? formatCents(l.debitCents) : ''}</td>
                            <td className="qb-grid-cell text-right tabular-nums">{l.creditCents ? formatCents(l.creditCents) : ''}</td>
                          </tr>
                        ))}
                      </tbody>
                    ))}
                  </table>
                </div>
              )}
              <div className="flex items-center gap-2">
                <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={!ready || working} onClick={post}>
                  Post {preview.entries.length} entr{preview.entries.length === 1 ? 'y' : 'ies'}
                </button>
              </div>
            </>
          )}
          {(history ?? []).length > 0 && (
            <table className="qb-grid" aria-label="Past payroll imports">
              <thead>
                <tr>
                  <th className="qb-grid-head">Imported</th>
                  <th className="qb-grid-head">File</th>
                  <th className="qb-grid-head w-[90px] text-right">Entries</th>
                  <th className="qb-grid-head w-[130px] text-right">Debits</th>
                </tr>
              </thead>
              <tbody>
                {history.map((h) => (
                  <tr key={h.id} className="qb-grid-row">
                    <td className="qb-grid-cell tabular-nums">{h.importedAt.slice(0, 10)}</td>
                    <td className="qb-grid-cell">{h.fileName}</td>
                    <td className="qb-grid-cell text-right tabular-nums">{h.entryCount}{h.voided ? ` (${h.voided} voided)` : ''}</td>
                    <td className="qb-grid-cell text-right tabular-nums">{formatCents(h.totalCents)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}
