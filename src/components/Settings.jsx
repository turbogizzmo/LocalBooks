import { useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatDate, todayIso } from '../lib/dates.js';
import { UpdatesPanel } from './Updates.jsx';
import FormDesigner from './FormDesigner.jsx';
import EmailSettings from './EmailSettings.jsx';
import FileDentist from './FileDentist.jsx';

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const CURRENCY_PRESETS = ['$', '€', '£', '¥', 'CA$', 'A$', '₹', 'CHF', 'R$'];

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** One preference field that commits to the database as soon as the user leaves it or picks a value. */
function PrefField({ label, prefKey, prefs, hint, children }) {
  const [value, setValue] = useState(prefs[prefKey] ?? '');
  const [state, setState] = useState({ status: 'idle' });

  useEffect(() => setValue(prefs[prefKey] ?? ''), [prefs, prefKey]);

  const commit = async (next = value) => {
    if (next === (prefs[prefKey] ?? '')) return;
    setState({ status: 'saving' });
    try {
      const stored = await api.setPreference(prefKey, next);
      setValue(stored);
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };

  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)_140px] items-center gap-2 border-b border-qb-border px-2 py-0.5 last:border-b-0">
      <label htmlFor={`pref-${prefKey}`} className="text-sm font-semibold text-qb-ink-muted">
        {label}
      </label>
      <div>
        {children({ id: `pref-${prefKey}`, value, setValue, commit, invalid: state.status === 'error' })}
        {hint && <div className="text-2xs text-qb-ink-faint">{hint}</div>}
      </div>
      <div className="truncate text-xs" title={state.message}>
        {state.status === 'saving' && <span className="text-qb-ink-muted">Saving…</span>}
        {state.status === 'saved' && <span className="font-semibold text-qb-green-active">✓ Saved</span>}
        {state.status === 'error' && <span className="text-qb-negative">{state.message}</span>}
      </div>
    </div>
  );
}

const textInput = (extra = {}) =>
  function TextInput({ id, value, setValue, commit, invalid }) {
    return (
      <input
        id={id}
        className="qb-input w-full"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => commit()}
        onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
        aria-invalid={invalid}
        {...extra}
      />
    );
  };

function Group({ title, id, children }) {
  return (
    <section className="qb-panel" id={id}>
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">{title}</h2>
      {children}
    </section>
  );
}

const SET_AT = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** Closing date: entries on or before it cannot be posted or voided; changes need the passcode. */
function ClosingDate() {
  const { data, error, reload } = useLedgerQuery(api.getClosingStatus);
  const [date, setDate] = useState('');
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [state, setState] = useState(null);

  useEffect(() => setDate(data?.lockDate ?? ''), [data?.lockDate]);

  const save = async (lockDate) => {
    setState({ busy: true });
    try {
      await api.setClosingDate({ lockDate: lockDate || null, currentPasscode: current || null, newPasscode: next === '' ? null : next });
      setCurrent('');
      setNext('');
      setState({ kind: 'success', text: lockDate ? `Books closed through ${formatDate(lockDate)}.` : 'All periods reopened.' });
      reload();
    } catch (err) {
      setState({ kind: 'error', text: errorText(err) });
    }
  };

  if (error) return <Notice kind="error" text={error} />;
  if (!data) return null;
  return (
    <Group title="Closing date" id="closing-date">
      <div className="flex flex-wrap items-end gap-3 px-2 py-1.5 text-sm">
        <div className="min-w-[200px]">
          <div className="text-2xs uppercase tracking-wide text-qb-ink-faint">In force</div>
          <div className="font-semibold">
            {data.lockDate ? `Closed through ${formatDate(data.lockDate)}` : 'No closing date: every period is open'}
            {data.passcodeProtected && <span className="ml-1 text-xs font-normal text-qb-ink-muted">· passcode protected</span>}
          </div>
        </div>
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">Closing date</span>
          <input type="date" className="qb-input" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {data.passcodeProtected && (
          <label className="flex flex-col gap-0.5 text-xs">
            <span className="font-semibold text-qb-ink-muted">Current passcode</span>
            <input type="password" className="qb-input w-[150px]" value={current} onChange={(e) => setCurrent(e.target.value)} autoComplete="off" />
          </label>
        )}
        <label className="flex flex-col gap-0.5 text-xs">
          <span className="font-semibold text-qb-ink-muted">{data.passcodeProtected ? 'New passcode (optional)' : 'Passcode (optional)'}</span>
          <input type="password" className="qb-input w-[150px]" value={next} onChange={(e) => setNext(e.target.value)} autoComplete="new-password" />
        </label>
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={state?.busy || !date} onClick={() => save(date)}>
          Close books
        </button>
        {data.lockDate && (
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" disabled={state?.busy} onClick={() => save('')}>
            Reopen all periods
          </button>
        )}
      </div>
      <p className="border-t border-qb-border px-2 py-1 text-xs text-qb-ink-muted">
        Entries dated on or before the closing date cannot be posted or voided, from any screen. Changing or removing the date needs
        the passcode. Every change is kept in the history below.
      </p>
      {state?.text && (
        <div className="px-2 pb-1">
          <Notice kind={state.kind} text={state.text} onDismiss={() => setState(null)} />
        </div>
      )}
      {data.history.length > 0 && (
        <ul className="max-h-[96px] overflow-auto border-t border-qb-border px-2 py-1 text-xs scrollbar-thin">
          {data.history.map((h) => (
            <li key={h.id} className="flex gap-2">
              <span className="w-[150px] text-qb-ink-muted">{SET_AT.format(new Date(h.setAt))}</span>
              <span>{h.lockDate ? `Closed through ${formatDate(h.lockDate)}` : 'Reopened all periods'}</span>
              {h.passcodeProtected && <span className="text-qb-ink-muted">· passcode</span>}
            </li>
          ))}
        </ul>
      )}
    </Group>
  );
}

function TaxCalendar() {
  const { data, error } = useLedgerQuery(api.getTaxCalendar);
  const today = todayIso();
  const next = (data ?? []).find((d) => d.dueDate >= today);
  return (
    <Group title="Tax calendar">
      {error && <Notice kind="error" text={error} />}
      <table className="qb-grid">
        <colgroup>
          <col className="w-[108px]" />
          <col className="w-[130px]" />
          <col />
          <col className="w-[84px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head">Due</th>
            <th className="qb-grid-head">Form</th>
            <th className="qb-grid-head">Filing</th>
            <th className="qb-grid-head">Status</th>
          </tr>
        </thead>
        <tbody>
          {(data ?? []).map((d) => (
            <tr key={`${d.dueDate}-${d.label}`} className={`qb-grid-row ${d.isPast ? 'text-qb-ink-faint' : ''}`}>
              <td className="qb-grid-cell font-semibold" title={d.statutoryDate !== d.dueDate ? `Statutory date ${formatDate(d.statutoryDate)} falls on a weekend` : undefined}>
                {formatDate(d.dueDate)}
                {d.statutoryDate !== d.dueDate && <span className="ml-0.5 text-qb-ink-faint">*</span>}
              </td>
              <td className="qb-grid-cell">{d.form}</td>
              <td className="qb-grid-cell" title={d.label}>{d.label}</td>
              <td className="qb-grid-cell">
                {d.isPast ? 'Past' : next && d.dueDate === next.dueDate && d.label === next.label ? <span className="font-semibold text-qb-warning">● Next</span> : 'Upcoming'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="px-2 py-0.5 text-2xs text-qb-ink-muted">
        Federal due dates for the configured entity type and fiscal year. * Weekend dates move to the next Monday; federal holidays,
        disaster relief and extensions are not applied, and state deadlines are not included. Confirm dates with the IRS or your tax advisor.
      </p>
    </Group>
  );
}

function CompanyLogoField() {
  const { data: logo, reload } = useLedgerQuery(api.getCompanyLogo);
  const [state, setState] = useState({ status: 'idle' });

  const upload = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'Logo image', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }] });
    if (!path) return;
    setState({ status: 'saving' });
    try {
      await api.setCompanyLogo(path);
      await reload();
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };
  const remove = async () => {
    try {
      await api.clearCompanyLogo();
      await reload();
      setState({ status: 'saved' });
    } catch (err) {
      setState({ status: 'error', message: errorText(err) });
    }
  };

  return (
    <div className="grid grid-cols-[170px_minmax(0,1fr)_140px] items-center gap-2 border-b border-qb-border px-2 py-1">
      <span className="text-sm font-semibold text-qb-ink-muted">Logo</span>
      <div className="flex items-center gap-2">
        <div className="grid h-[44px] w-[132px] place-items-center rounded-xs border border-dashed border-qb-border-strong bg-qb-canvas">
          {logo ? <img src={logo} alt="Company logo" className="max-h-[40px] max-w-[124px] object-contain" /> : <span className="text-2xs text-qb-ink-faint">No logo</span>}
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="flex gap-1">
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={upload}>
              {logo ? 'Replace…' : 'Upload logo…'}
            </button>
            {logo && (
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={remove}>
                Remove
              </button>
            )}
          </span>
          <span className="text-2xs text-qb-ink-faint">PNG, JPEG, GIF or WebP under 512 KB · printed on invoices, estimates and reports</span>
        </div>
      </div>
      <div className="truncate text-xs" title={state.message}>
        {state.status === 'saving' && <span className="text-qb-ink-muted">Saving…</span>}
        {state.status === 'saved' && <span className="font-semibold text-qb-green-active">✓ Saved</span>}
        {state.status === 'error' && <span className="text-qb-negative">{state.message}</span>}
      </div>
    </div>
  );
}

function PreferencesConsole() {
  const { data: prefs, error } = useLedgerQuery(api.getPreferences);
  const { data: forms } = useLedgerQuery(api.listTaxForms);
  if (error) return <Notice kind="error" text={error} />;
  if (!prefs) return <div className="px-2 py-1 text-sm text-qb-ink-muted">Loading…</div>;
  const field = (props) => <PrefField prefs={prefs} {...props} />;

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-start gap-2">
      <div className="flex flex-col gap-2">
        <Group title="Company">
          {field({ label: 'Company name', prefKey: 'company_name', hint: 'Shown on reports, in the sidebar and in the window title', children: textInput({ maxLength: 100 }) })}
          <CompanyLogoField />
          {field({ label: 'Legal name', prefKey: 'company_legal_name', children: textInput({ maxLength: 150 }) })}
          {field({ label: 'EIN', prefKey: 'company_ein', hint: 'Federal employer identification number', children: textInput({ placeholder: 'XX-XXXXXXX', maxLength: 10 }) })}
          {field({ label: 'Address', prefKey: 'company_address', children: textInput({ maxLength: 300 }) })}
          {field({ label: 'Email', prefKey: 'company_email', children: textInput({ type: 'email', maxLength: 150 }) })}
          {field({ label: 'Phone', prefKey: 'company_phone', children: textInput({ type: 'tel', maxLength: 40 }) })}
        </Group>
        <Group title="Accounting & tax">
          {field({
            label: 'First month of fiscal year',
            prefKey: 'fiscal_year_start',
            hint: 'Drives fiscal report periods and when Net Income closes into Retained Earnings',
            children: ({ id, value, commit, invalid }) => (
              <select id={id} className="qb-input w-[180px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
                {MONTHS.map((m, i) => (
                  <option key={m} value={`${String(i + 1).padStart(2, '0')}-01`}>{m}</option>
                ))}
              </select>
            ),
          })}
          {field({
            label: 'Tax form',
            prefKey: 'tax_form',
            hint: 'Sets the federal filing deadlines in the tax calendar',
            children: ({ id, value, commit, invalid }) => (
              <select id={id} className="qb-input w-full" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
                {(forms ?? []).map(([code, label]) => (
                  <option key={code} value={code}>{label}</option>
                ))}
              </select>
            ),
          })}
          {field({
            label: 'Home currency',
            prefKey: 'home_currency_code',
            hint: 'Every report and balance is in this currency; it locks once foreign-currency transactions exist',
            children: ({ id, value, setValue, commit, invalid }) => (
              <input
                id={id}
                className="qb-input w-[70px] uppercase"
                value={value}
                maxLength={3}
                onChange={(e) => setValue(e.target.value.toUpperCase())}
                onBlur={() => commit()}
                aria-invalid={invalid}
              />
            ),
          })}
          {field({
            label: 'Currency symbol',
            prefKey: 'currency_symbol',
            hint: 'Display only; amounts are stored in cents and never converted',
            children: ({ id, value, setValue, commit, invalid }) => (
              <span className="flex gap-1">
                <input id={id} className="qb-input w-[70px]" value={value} maxLength={4} onChange={(e) => setValue(e.target.value)} onBlur={() => commit()} aria-invalid={invalid} />
                {CURRENCY_PRESETS.map((c) => (
                  <button key={c} type="button" className={`qb-btn qb-btn-secondary h-input px-1.5 ${value === c ? 'border-qb-green text-qb-green-active' : ''}`} onClick={() => commit(c)}>
                    {c}
                  </button>
                ))}
              </span>
            ),
          })}
        </Group>
        <Group title="Class tracking" id="class-tracking">
          {field({
            label: 'Use class tracking',
            prefKey: 'class_tracking',
            hint: 'Adds a Class column to sales, purchase, deposit and journal entry forms (manage the list in Accounting › Classes)',
            children: ({ id, value, commit }) => (
              <label className="flex items-center gap-1.5 text-sm">
                <input id={id} type="checkbox" className="accent-qb-green" checked={value === 'true'} onChange={(e) => commit(e.target.checked ? 'true' : 'false')} />
                Track classes on transactions
              </label>
            ),
          })}
          {prefs.class_tracking === 'true' &&
            field({
              label: 'Assign classes',
              prefKey: 'class_assign',
              hint: 'One class per line, or one class for the whole transaction',
              children: ({ id, value, commit, invalid }) => (
                <select id={id} className="qb-input w-[220px]" value={value} onChange={(e) => commit(e.target.value)} aria-invalid={invalid}>
                  <option value="line">One to each line</option>
                  <option value="transaction">One to the entire transaction</option>
                </select>
              ),
            })}
          {prefs.class_tracking === 'true' &&
            field({
              label: 'Warn when no class',
              prefKey: 'class_prompt',
              hint: 'Asks before saving income or expense lines that have no class',
              children: ({ id, value, commit }) => (
                <label className="flex items-center gap-1.5 text-sm">
                  <input id={id} type="checkbox" className="accent-qb-green" checked={value === 'true'} onChange={(e) => commit(e.target.checked ? 'true' : 'false')} />
                  Warn me when a transaction is not assigned a class
                </label>
              ),
            })}
        </Group>
        <Group title="Automatic backups">
          {field({
            label: 'Daily backup',
            prefKey: 'auto_backup_enabled',
            hint: 'Taken when LocalBooks starts, at most once a day',
            children: ({ id, value, commit }) => (
              <label className="flex items-center gap-1.5 text-sm">
                <input id={id} type="checkbox" className="accent-qb-green" checked={value === 'true'} onChange={(e) => commit(e.target.checked ? 'true' : 'false')} />
                Back up the company file automatically
              </label>
            ),
          })}
          {field({ label: 'Keep', prefKey: 'auto_backup_keep', hint: 'Number of daily backups to keep (1–60)', children: textInput({ inputMode: 'numeric', className: 'qb-input num w-[70px]' }) })}
        </Group>
      </div>
      <ClosingDate />
      <TaxCalendar />
    </div>
  );
}

const STORAGE_BADGE = {
  LOCAL: { className: 'border-qb-green/50 bg-qb-green-tint text-qb-green-active', icon: '✓' },
  NETWORK: { className: 'border-qb-warning/60 bg-[#fff4e5] text-[#a35200]', icon: '⚠' },
  SYNCED: { className: 'border-qb-warning/60 bg-[#fff4e5] text-[#a35200]', icon: '⚠' },
};

function StorageBadge({ storage, verified }) {
  const style = STORAGE_BADGE[storage.kind];
  return (
    <span className={`inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-[1px] text-xs font-semibold ${style.className}`}>
      <span aria-hidden="true">{style.icon}</span>
      {storage.detail}
      {storage.kind === 'LOCAL' && verified ? ' · verified' : ''}
    </span>
  );
}

const SHARED_WARNING =
  'Shared folders work for one person at a time: LocalBooks locks the company file while it is open and uses a safer journal mode, but never open the same company file on two computers at once, and let a synced folder finish syncing before opening it elsewhere.';

function CompanyFileLocation() {
  const status = useLedgerQuery(api.getStorageStatus);
  const [path, setPath] = useState('');
  const [inspection, setInspection] = useState(null);
  const [mode, setMode] = useState('MOVE');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (status.data) setPath(status.data.directory);
  }, [status.data]);

  const inspect = async (dir) => {
    setNotice(null);
    setInspection(null);
    if (!dir.trim()) return;
    try {
      setInspection(await api.inspectDatabaseDirectory(dir.trim()));
      setMode('MOVE');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const browse = async () => {
    const dir = await open({ directory: true, multiple: false, defaultPath: status.data?.directory });
    if (!dir) return;
    setPath(dir);
    inspect(dir);
  };

  const apply = async () => {
    setBusy(true);
    try {
      const next = await api.updateDatabaseDirectory(inspection.directory, inspection.hasCompanyFile ? 'OPEN' : mode);
      setInspection(null);
      setNotice({ kind: 'success', text: `LocalBooks is now using ${next.path}. Reloading…` });
      setTimeout(() => window.location.reload(), 1200);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const current = status.data;
  const changed = current && inspection && inspection.directory !== current.directory;

  return (
    <Group title="Company file location">
      {current && (
        <div className="flex flex-wrap items-center gap-2 border-b border-qb-border px-2 py-1 text-xs">
          <StorageBadge storage={current.storage} verified />
          <span className="text-qb-ink-muted">
            {current.isDefault ? 'Default location' : 'Custom location'} · journal mode <span className="font-mono">{current.journalMode}</span>
          </span>
          <span className="min-w-0 flex-1 truncate font-mono text-qb-ink" title={current.path}>{current.path}</span>
        </div>
      )}
      {current?.storage.kind !== 'LOCAL' && current && <div className="border-b border-qb-border bg-[#fff8eb] px-2 py-0.5 text-xs">{SHARED_WARNING}</div>}
      <form
        className="flex items-center gap-2 px-2 py-1"
        onSubmit={(e) => {
          e.preventDefault();
          inspect(path);
        }}
      >
        <label htmlFor="db-dir" className="whitespace-nowrap text-sm font-semibold text-qb-ink-muted">
          Folder
        </label>
        <input id="db-dir" className="qb-input min-w-0 flex-1 font-mono text-sm" value={path} onChange={(e) => setPath(e.target.value)} placeholder="/Volumes/Accounting/LocalBooks" spellCheck={false} />
        <button type="submit" className="qb-btn qb-btn-secondary h-row-lg" disabled={busy}>
          Check
        </button>
        <button type="button" className="qb-btn qb-btn-primary h-row-lg" onClick={browse} disabled={busy}>
          Change Company File Location…
        </button>
      </form>
      {inspection && (
        <div className="border-t border-qb-border px-2 py-1 text-sm">
          <div className="mb-1 flex flex-wrap items-center gap-2">
            <StorageBadge storage={inspection.storage} verified={!inspection.problem} />
            <span className="font-mono text-xs text-qb-ink-muted">{inspection.targetPath}</span>
          </div>
          {inspection.problem ? (
            <div className="font-semibold text-qb-negative">{inspection.problem.replace(/^[A-Z_]+: /, '')}</div>
          ) : !changed ? (
            <div className="text-qb-ink-muted">LocalBooks is already using this folder.</div>
          ) : (
            <>
              {inspection.storage.kind !== 'LOCAL' && <div className="mb-1 text-xs text-[#a35200]">{SHARED_WARNING}</div>}
              {inspection.hasCompanyFile ? (
                <div>
                  Found “{inspection.companyFile.companyName ?? 'a company file'}” with {inspection.companyFile.journalEntries} posted entries
                  {inspection.companyFile.needsUpgrade ? ` (it will be upgraded to schema v${inspection.companyFile.appSchemaVersion})` : ''}. It passed integrity,
                  link and balance checks. Your current books stay where they are.
                </div>
              ) : (
                <fieldset className="flex flex-col gap-0.5">
                  <legend className="mb-0.5 text-qb-ink-muted">This folder has no company file. What should LocalBooks do?</legend>
                  <label className="flex items-center gap-1.5">
                    <input type="radio" name="relocate-mode" className="accent-qb-green" checked={mode === 'MOVE'} onChange={() => setMode('MOVE')} />
                    Copy my current books there and use the copy (the original stays as a backup)
                  </label>
                  <label className="flex items-center gap-1.5">
                    <input type="radio" name="relocate-mode" className="accent-qb-green" checked={mode === 'NEW'} onChange={() => setMode('NEW')} />
                    Start a new, empty company file there
                  </label>
                </fieldset>
              )}
              <div className="mt-1 flex gap-2">
                <button type="button" className="qb-btn qb-btn-primary" onClick={apply} disabled={busy}>
                  {busy ? 'Switching…' : inspection.hasCompanyFile ? 'Open this company file' : mode === 'MOVE' ? 'Move books here' : 'Create company file here'}
                </button>
                <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setInspection(null)} disabled={busy}>
                  Cancel
                </button>
              </div>
            </>
          )}
        </div>
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
    </Group>
  );
}

function RestoreConfirm({ info, onConfirm, onCancel, busy }) {
  return (
    <div className="border-t border-qb-border bg-[#fff8eb] px-2 py-1 text-sm">
      <div className="font-semibold">Restore “{info.companyName ?? 'Unnamed company'}”?</div>
      <div className="text-qb-ink-muted">
        {info.journalEntries} posted entr{info.journalEntries === 1 ? 'y' : 'ies'}
        {info.lastEntryDate ? `, latest ${formatDate(info.lastEntryDate)}` : ''} · {formatBytes(info.sizeBytes)} · schema v{info.schemaVersion}
        {info.needsUpgrade ? ` (will be upgraded to v${info.appSchemaVersion})` : ''}
      </div>
      <div className="mt-0.5">
        This replaces the books you have open now. A safety backup of your current books is saved first, so this can be undone by restoring that file.
      </div>
      <div className="mt-1 flex gap-2">
        <button type="button" className="qb-btn bg-qb-negative text-white hover:opacity-90" onClick={onConfirm} disabled={busy}>
          {busy ? 'Restoring…' : 'Replace current books'}
        </button>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
      </div>
    </div>
  );
}

function FileAndData({ companyName }) {
  const backups = useLedgerQuery(api.listBackups);
  const [notice, setNotice] = useState(null);
  const [pending, setPending] = useState(null);
  const [busy, setBusy] = useState(false);

  const safeName = (companyName || 'LocalBooks').replace(/[^\w\- ]+/g, '').trim().replace(/\s+/g, '-');

  const backupTo = async () => {
    try {
      const path = await save({
        defaultPath: `${safeName}-${todayIso()}.localbooks`,
        filters: [{ name: 'LocalBooks company file', extensions: ['localbooks', 'db'] }],
      });
      if (!path) return;
      setBusy(true);
      const file = await api.exportCompanyFile(path);
      setNotice({ kind: 'success', text: `Company file backed up to ${file.path} (${formatBytes(file.sizeBytes)}). It was verified after writing.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const backupNow = async () => {
    setBusy(true);
    try {
      const file = await api.backupNow();
      setNotice({ kind: 'success', text: `Snapshot ${file.fileName} saved (${formatBytes(file.sizeBytes)}).` });
      backups.reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const prepareRestore = async (path) => {
    setNotice(null);
    try {
      const info = await api.inspectCompanyFile(path);
      setPending({ path, info });
    } catch (err) {
      setPending(null);
      setNotice({ kind: 'error', text: `This file can't be restored: ${errorText(err)}` });
    }
  };

  const chooseRestore = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'LocalBooks company file', extensions: ['localbooks', 'db'] }] });
    if (path) prepareRestore(path);
  };

  // Opens another company file in place for this session (the same as double-clicking it).
  const openCompanyFile = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'LocalBooks company file', extensions: ['localbooks', 'db'] }] });
    if (!path) return;
    setBusy(true);
    setNotice(null);
    try {
      await api.openCompanyFile(path);
    } catch (err) {
      setNotice({ kind: 'error', text: `This file can't be opened: ${errorText(err)}` });
      setBusy(false);
    }
  };

  const restore = async () => {
    setBusy(true);
    try {
      const result = await api.importCompanyFile(pending.path);
      setPending(null);
      setNotice({
        kind: 'success',
        text: `Restored “${result.restored.companyName ?? 'company file'}”. Your previous books were saved as ${result.safetyBackup.fileName}. Reloading…`,
      });
      setTimeout(() => window.location.reload(), 1200);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const KIND = {
    AUTO: 'Automatic',
    MANUAL: 'Manual',
    PRE_RESTORE: 'Before restore',
    PRE_UPGRADE: 'Before upgrade',
    PRE_IMPORT: 'Before import',
    PRE_REBUILD: 'Before rebuild',
    EXPORT: 'Exported',
  };

  return (
    <div className="flex flex-col gap-2">
      <Group title="File & data controls">
        <div className="flex flex-wrap items-center gap-2 px-2 py-1.5">
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={backupTo} disabled={busy}>
            Backup Company File…
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={chooseRestore} disabled={busy}>
            Restore from Backup…
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={openCompanyFile} disabled={busy}>
            Open Company File…
          </button>
          <span className="text-xs text-qb-ink-muted">
            Backups are taken live: keep working while they run. Restores are checked for integrity, schema version, broken links and
            unbalanced entries before anything is replaced. Backups are saved as .localbooks files, which open in LocalBooks with a
            double-click; an opened file is used in place for this session.
          </span>
        </div>
        {pending && <RestoreConfirm info={pending.info} busy={busy} onConfirm={restore} onCancel={() => setPending(null)} />}
      </Group>
      <FileDentist onRebuilt={backups.reload} />
      <CompanyFileLocation />
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      <section className="qb-panel flex min-h-0 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Snapshots on this computer</h2>
          <span className="text-xs text-qb-ink-muted">Kept on this computer, even when the company file is on a shared drive</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={backupNow} disabled={busy}>
            Back up now
          </button>
        </div>
        <table className="qb-grid">
          <colgroup>
            <col className="w-[160px]" />
            <col className="w-[120px]" />
            <col />
            <col className="w-[90px]" />
            <col className="w-[90px]" />
          </colgroup>
          <thead>
            <tr>
              <th className="qb-grid-head">Taken</th>
              <th className="qb-grid-head">Kind</th>
              <th className="qb-grid-head">File</th>
              <th className="qb-grid-head text-right">Size</th>
              <th className="qb-grid-head" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {backups.data && backups.data.length === 0 && (
              <tr>
                <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">No snapshots yet.</td>
              </tr>
            )}
            {(backups.data ?? []).map((b) => (
              <tr key={b.path} className="qb-grid-row">
                <td className="qb-grid-cell">{b.modified.replace('T', ' ')}</td>
                <td className="qb-grid-cell">{KIND[b.kind] ?? b.kind}</td>
                <td className="qb-grid-cell font-mono text-xs" title={b.path}>{b.fileName}</td>
                <td className="qb-grid-cell num">{formatBytes(b.sizeBytes)}</td>
                <td className="qb-grid-cell cell-tight">
                  <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full" onClick={() => prepareRestore(b.path)} disabled={busy}>
                    Restore
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}

function CopyButton({ text, label = 'Copy' }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };
  return (
    <button type="button" className="qb-btn qb-btn-secondary h-row flex-none px-2 text-xs" onClick={copy}>
      {copied ? '✓ Copied' : label}
    </button>
  );
}

/** Turns the read-only local MCP server on or off and shows how to connect an AI assistant to it. */
function LocalAiAccess() {
  const [status, setStatus] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [reveal, setReveal] = useState(false);

  useEffect(() => {
    api.getMcpStatus().then(setStatus, (err) => setError(errorText(err)));
  }, []);

  const run = async (fn) => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await fn());
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  if (!status) return error ? <Notice kind="error" text={error} /> : null;
  const token = status.token ?? '';
  const shown = reveal ? token : '•'.repeat(Math.min(token.length, 24));
  const cli = (t) => `claude mcp add --transport http localbooks ${status.url} --header "Authorization: Bearer ${t}"`;
  const json = (t) =>
    JSON.stringify({ mcpServers: { localbooks: { type: 'http', url: status.url, headers: { Authorization: `Bearer ${t}` } } } }, null, 2);

  return (
    <div className="flex flex-col gap-2">
      <Group title="Local AI access (Model Context Protocol)">
        <div className="flex flex-wrap items-center gap-2 px-2 py-1.5">
          <button
            type="button"
            className={`qb-btn h-row-lg px-3 ${status.enabled ? 'qb-btn-secondary' : 'qb-btn-primary'}`}
            onClick={() => run(() => api.setMcpEnabled(!status.enabled))}
            disabled={busy}
          >
            {status.enabled ? 'Turn off' : 'Turn on'}
          </button>
          <span className="text-sm">
            {status.running ? (
              <span className="font-semibold text-qb-green-active">● Running at {status.url}</span>
            ) : status.enabled ? (
              <span className="font-semibold text-qb-negative">▲ Not running</span>
            ) : (
              <span className="text-qb-ink-muted">Off</span>
            )}
          </span>
        </div>
        <p className="border-t border-qb-border px-2 py-1 text-xs text-qb-ink-muted">
          Lets an AI assistant on this computer, such as Claude Code or Claude Desktop, read three summaries: the trial balance, a
          profit and loss summary, and overdue receivables aging. It cannot post, void or change anything. The server answers only on
          this computer (127.0.0.1), refuses requests from web pages, and requires the access token below. Your books never leave this
          computer through LocalBooks, but the assistant you connect may send what it reads to its AI provider.
        </p>
      </Group>
      {status.error && <Notice kind="error" text={status.error.replace(/^[A-Z_]+: /, '')} />}
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {status.enabled && token && (
        <Group title="Connect an assistant">
          <div className="grid grid-cols-[110px_minmax(0,1fr)_auto] items-center gap-x-2 gap-y-1 px-2 py-1.5 text-sm">
            <span className="font-semibold text-qb-ink-muted">Server URL</span>
            <code className="truncate font-mono text-xs">{status.url}</code>
            <CopyButton text={status.url} />
            <span className="font-semibold text-qb-ink-muted">Access token</span>
            <code className="truncate font-mono text-xs" aria-label="Access token">{shown}</code>
            <div className="flex gap-1">
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" onClick={() => setReveal((r) => !r)}>
                {reveal ? 'Hide' : 'Show'}
              </button>
              <CopyButton text={token} />
              <button
                type="button"
                className="qb-btn qb-btn-secondary h-row px-2 text-xs"
                disabled={busy}
                onClick={() => {
                  if (window.confirm('Issue a new access token? Assistants using the current token will stop working until you update them.')) {
                    run(api.regenerateMcpToken);
                  }
                }}
              >
                Regenerate
              </button>
            </div>
          </div>
          <div className="flex flex-col gap-1 border-t border-qb-border px-2 py-1.5">
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-qb-ink-muted">Claude Code (run in a terminal)</span>
              <span className="ml-auto" />
              <CopyButton text={cli(token)} label="Copy command" />
            </div>
            <pre className="overflow-x-auto bg-qb-canvas px-2 py-1 font-mono text-2xs scrollbar-thin">{cli(shown)}</pre>
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-qb-ink-muted">Other MCP clients (JSON configuration)</span>
              <span className="ml-auto" />
              <CopyButton text={json(token)} label="Copy JSON" />
            </div>
            <pre className="overflow-x-auto bg-qb-canvas px-2 py-1 font-mono text-2xs scrollbar-thin">{json(shown)}</pre>
          </div>
        </Group>
      )}
    </div>
  );
}

const SETTINGS_TABS = { preferences: 'preferences', closing: 'preferences', forms: 'forms', email: 'email', data: 'data', ai: 'ai', updates: 'updates' };

export default function Settings({ companyName, intent }) {
  const [tab, setTab] = useState(() => SETTINGS_TABS[intent?.action] ?? 'preferences');

  // Help shortcuts open a specific tab (and, for the closing date, scroll to it).
  useEffect(() => {
    const target = SETTINGS_TABS[intent?.action];
    if (!target) return;
    setTab(target);
    if (intent.action === 'closing') {
      const timer = setTimeout(() => document.getElementById('closing-date')?.scrollIntoView({ behavior: 'smooth', block: 'start' }), 50);
      return () => clearTimeout(timer);
    }
  }, [intent]);
  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="flex flex-none gap-px self-start overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist">
        {[
          ['preferences', 'Preferences Console'],
          ['forms', 'Custom Forms'],
          ['email', 'Email'],
          ['data', 'File & Data Controls'],
          ['ai', 'AI Access (MCP)'],
          ['updates', 'Updates'],
        ].map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`px-3 py-0.5 font-semibold ${tab === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === 'preferences' && <PreferencesConsole />}
      {tab === 'forms' && <FormDesigner />}
      {tab === 'email' && <EmailSettings />}
      {tab === 'data' && <FileAndData companyName={companyName} />}
      {tab === 'ai' && <LocalAiAccess />}
      {tab === 'updates' && <UpdatesPanel />}
    </div>
  );
}
