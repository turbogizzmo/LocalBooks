import { useCallback, useEffect, useMemo, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { FolderOpen, Plus, Search } from 'lucide-react';
import Notice from './Notice.jsx';
import { api, COMPANY_FILE_CHANGED, errorText } from '../lib/api.js';
import { ACCOUNT_TYPE_NAMES } from '../lib/accountTypes.js';

/** Sent by File › New Company… ("new") and File › Open or Restore Company… ("open"); see file_menu.rs. */
const FILE_MENU_REQUESTED = 'file-menu-requested';
/** QuickBooks Desktop company files and backups: opening one starts a new company and migrates it. */
export const QUICKBOOKS_EXTENSIONS = ['qbw', 'qbb', 'qbm'];
/** QuickBooks names its files in capitals (`Company.QBW`), and some file pickers match extensions case by case. */
export const QUICKBOOKS_FILTER_EXTENSIONS = QUICKBOOKS_EXTENSIONS.flatMap((ext) => [ext, ext.toUpperCase()]);
const COMPANY_FILTER = {
  name: 'LocalBooks or QuickBooks company file or backup',
  extensions: ['localbooks', 'db', ...QUICKBOOKS_FILTER_EXTENSIONS],
};
/** The QuickBooks file to read once the new company it is migrating into has opened (the window reloads in between). */
const PENDING_MIGRATION_KEY = 'localbooks.pendingMigration';

export function isQuickBooksFile(path) {
  return QUICKBOOKS_EXTENSIONS.includes(path.split('.').pop()?.toLowerCase());
}

/** Whether a QuickBooks file is waiting for the Desktop Migration Wizard in this window. */
export function hasPendingMigration() {
  try {
    return !!sessionStorage.getItem(PENDING_MIGRATION_KEY);
  } catch {
    return false;
  }
}

/** The QuickBooks file waiting for the Desktop Migration Wizard, if any; it is handed over only once. */
export function takePendingMigration() {
  try {
    const path = sessionStorage.getItem(PENDING_MIGRATION_KEY);
    sessionStorage.removeItem(PENDING_MIGRATION_KEY);
    return path;
  } catch {
    return null;
  }
}

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

function fileStem(path) {
  const name = path.split(/[\\/]/).pop() ?? '';
  return name.replace(/\.[^.]+$/, '');
}

/** A company name as a file name: "Acme & Sons, Ltd." → "Acme Sons Ltd". */
function safeFileName(name) {
  return name.replace(/[^\w\- ]+/g, ' ').replace(/\s+/g, ' ').trim() || 'New Company';
}

/**
 * Open or Restore Company: a `.localbooks` file (or a `localbooks.db` kept in a folder) opens in
 * place; any other `.db` is a LocalBooks backup, which is restored to a new company file so the
 * backup itself stays as it was. A QuickBooks `.qbw`/`.qbb`/`.qbm` file starts a new company file
 * and opens the Desktop Migration Wizard on it; the QuickBooks file is only read. Resolves once the
 * company is open (the window then reloads), or when the user cancels.
 */
export async function openOrRestoreCompany() {
  const path = await open({ multiple: false, directory: false, title: 'Open or restore a company', filters: [COMPANY_FILTER] });
  if (!path) return;
  const name = path.split(/[\\/]/).pop().toLowerCase();
  if (name.endsWith('.localbooks') || name === 'localbooks.db') {
    await api.openCompanyFile(path);
    return;
  }
  if (isQuickBooksFile(path)) {
    await migrateQuickBooksFile(path);
    return;
  }
  const destination = await save({
    title: 'Restore the backup as a new company file',
    defaultPath: `${fileStem(path)}.localbooks`,
    filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }],
  });
  if (destination) await api.restoreBackupAs(path, destination);
}

/**
 * Starts a new company file named after a QuickBooks file and opens the Desktop Migration Wizard on
 * it once the new company has opened. Resolves when the user cancels, or as the window reloads.
 */
export async function migrateQuickBooksFile(path) {
  const companyName = fileStem(path);
  const destination = await save({
    title: 'Save the new company file to migrate the QuickBooks books into',
    defaultPath: `${safeFileName(companyName)}.localbooks`,
    filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }],
  });
  if (!destination) return;
  try {
    sessionStorage.setItem(PENDING_MIGRATION_KEY, path);
  } catch {
    // Without storage the new company still opens; the file is then chosen in the wizard.
  }
  try {
    await api.createCompanyFile(destination, companyName);
  } catch (err) {
    takePendingMigration();
    throw err;
  }
}

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const SETUP_TABS = [
  ['company', 'Company information'],
  ['accounts', 'Industry and accounts'],
  ['expert', 'Expert settings'],
];
const TYPE_ORDER = Object.keys(ACCOUNT_TYPE_NAMES);

/** The industry accounts QuickBooks checks by default: its core accounts. */
const defaultSelection = (chart) => new Set(chart.filter((a) => a.group === 'core').map((a) => a.number));

/**
 * New Company, modeled on QuickBooks' New Company setup: company and tax information, then an
 * industry and its suggested chart of accounts (core accounts checked, recommended ones offered),
 * then expert settings. "Skip" creates the company with the standard chart instead. Asks where to
 * save the file last, and opens it.
 */
export function NewCompanyDialog({ onClose }) {
  const [tab, setTab] = useState('company');
  const [options, setOptions] = useState(null);
  const [info, setInfo] = useState({ name: '', legalName: '', address: '', ein: '', entityType: 'sole_proprietorship', fiscalYearStart: '01-01' });
  const [industry, setIndustry] = useState('');
  const [chart, setChart] = useState([]);
  const [selected, setSelected] = useState(() => new Set());
  const [bankName, setBankName] = useState('Checking');
  const [salesTax, setSalesTax] = useState(false);
  const [inventory, setInventory] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  useEffect(() => {
    api.getCompanySetupOptions().then(setOptions, (err) => setError(errorText(err)));
  }, []);

  // The chart follows the industry and tax form; a new industry starts from its suggested accounts.
  useEffect(() => {
    if (!industry) {
      setChart([]);
      return;
    }
    let live = true;
    api.getStarterChart(industry, info.entityType).then(
      (rows) => live && setChart(rows),
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [industry, info.entityType]);

  const chooseIndustry = async (name) => {
    setIndustry(name);
    setSalesTax(!!options?.industries.find((i) => i.name === name)?.salesTax);
    setSelected(name ? defaultSelection(await api.getStarterChart(name, info.entityType)) : new Set());
  };

  const set = (key) => (e) => setInfo((v) => ({ ...v, [key]: e.target.value }));

  /** Checking a subaccount checks its parent; unchecking a parent unchecks its subaccounts. */
  const toggle = (account) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(account.number)) {
        next.delete(account.number);
        chart.filter((a) => a.parentNumber === account.number).forEach((a) => next.delete(a.number));
      } else {
        next.add(account.number);
        if (account.parentNumber) next.add(account.parentNumber);
      }
      return next;
    });

  const groups = useMemo(() => {
    const byType = new Map();
    for (const a of chart) {
      if (!byType.has(a.accountType)) byType.set(a.accountType, []);
      byType.get(a.accountType).push(a);
    }
    return TYPE_ORDER.filter((t) => byType.has(t)).map((t) => [t, byType.get(t)]);
  }, [chart]);
  const required = chart.filter((a) => a.group === 'required').length;

  const create = async (withSetup) => {
    if (!info.name.trim()) {
      setTab('company');
      return;
    }
    setError(null);
    const path = await save({
      title: 'Save the new company file',
      defaultPath: `${safeFileName(info.name)}.localbooks`,
      filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }],
    });
    if (!path) return;
    const setup = withSetup
      ? {
          industry,
          entityType: info.entityType,
          accounts: [...selected],
          bankAccountName: bankName,
          legalName: info.legalName,
          address: info.address,
          ein: info.ein,
          fiscalYearStart: info.fiscalYearStart,
          salesTax,
          inventory,
        }
      : null;
    setBusy(true);
    try {
      await api.createCompanyFile(path, info.name.trim(), setup);
      // The window reloads on the new company.
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const tabIndex = SETUP_TABS.findIndex(([id]) => id === tab);
  const canFinish = !!info.name.trim() && !!industry;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="New company">
      <form
        className="qb-panel flex h-[min(640px,90vh)] w-[720px] flex-col p-3 shadow-panel"
        onSubmit={(e) => {
          e.preventDefault();
          if (tabIndex < SETUP_TABS.length - 1) setTab(SETUP_TABS[tabIndex + 1][0]);
          else if (canFinish) create(true);
        }}
      >
        <h2 className="mb-1 shrink-0 text-md font-semibold">Create a new company</h2>
        <p className="mb-2 shrink-0 text-sm text-qb-ink-muted">
          Answer a few questions and LocalBooks suggests a chart of accounts for your type of business, like QuickBooks does. The company you have open now is not changed.
        </p>
        <div className="mb-2 flex shrink-0 self-start overflow-hidden rounded-xs border border-qb-border-strong text-sm" role="tablist" aria-label="Setup steps">
          {SETUP_TABS.map(([id, label], i) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              onClick={() => setTab(id)}
              disabled={busy}
              className={`px-2.5 py-0.5 font-semibold ${tab === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
            >
              {i + 1}. {label}
            </button>
          ))}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto">
          {tab === 'company' && (
            <div className="grid grid-cols-2 gap-2">
              <label className="col-span-2 block">
                <span className="qb-label">Company name</span>
                <input className="qb-input w-full" value={info.name} onChange={set('name')} autoFocus disabled={busy} />
              </label>
              <label className="block">
                <span className="qb-label">Legal name</span>
                <input className="qb-input w-full" value={info.legalName} onChange={set('legalName')} disabled={busy} placeholder="If different" />
              </label>
              <label className="block">
                <span className="qb-label">Federal ID (EIN)</span>
                <input className="qb-input w-full" value={info.ein} onChange={set('ein')} disabled={busy} placeholder="XX-XXXXXXX" />
              </label>
              <label className="col-span-2 block">
                <span className="qb-label">Address</span>
                <textarea className="qb-input w-full" rows={2} value={info.address} onChange={set('address')} disabled={busy} />
              </label>
              <label className="block">
                <span className="qb-label">Income tax form used</span>
                <select className="qb-input w-full" value={info.entityType} onChange={set('entityType')} disabled={busy}>
                  {(options?.entityTypes ?? []).map((t) => (
                    <option key={t.key} value={t.key}>{t.label}</option>
                  ))}
                </select>
              </label>
              <label className="block">
                <span className="qb-label">First month in fiscal year</span>
                <select className="qb-input w-full" value={info.fiscalYearStart} onChange={set('fiscalYearStart')} disabled={busy}>
                  {MONTHS.map((m, i) => (
                    <option key={m} value={`${String(i + 1).padStart(2, '0')}-01`}>{m}</option>
                  ))}
                </select>
              </label>
            </div>
          )}

          {tab === 'accounts' && (
            <div>
              <div className="mb-2 flex items-end gap-2">
                <label className="block flex-1">
                  <span className="qb-label">Industry</span>
                  <select className="qb-input w-full" value={industry} onChange={(e) => chooseIndustry(e.target.value)} disabled={busy} aria-label="Industry">
                    <option value="">Choose the type of business closest to yours…</option>
                    {(options?.industries ?? []).map((i) => (
                      <option key={i.name} value={i.name}>{i.name}</option>
                    ))}
                  </select>
                </label>
                <label className="block w-[200px]">
                  <span className="qb-label">Bank account name</span>
                  <input className="qb-input w-full" value={bankName} onChange={(e) => setBankName(e.target.value)} disabled={busy} />
                </label>
              </div>
              {industry ? (
                <>
                  <div className="mb-1 flex items-center justify-between text-sm text-qb-ink-muted">
                    <span>
                      We've preselected the accounts most businesses like yours need. Uncheck any you don't want or check more. {selected.size + required} accounts will be created.
                    </span>
                    <button type="button" className="qb-btn qb-btn-secondary shrink-0" onClick={() => setSelected(defaultSelection(chart))} disabled={busy}>
                      Default
                    </button>
                  </div>
                  <div className="rounded-xs border border-qb-border">
                    {groups.map(([type, accounts]) => (
                      <fieldset key={type} className="border-b border-qb-border last:border-b-0">
                        <legend className="sr-only">{ACCOUNT_TYPE_NAMES[type]}</legend>
                        <div className="bg-qb-row-hover px-2 py-0.5 text-xs font-semibold uppercase text-qb-ink-muted">{ACCOUNT_TYPE_NAMES[type]}</div>
                        {accounts.map((a) => {
                          const fixed = a.group === 'required';
                          return (
                            <label key={a.number} className={`flex items-baseline gap-2 px-2 py-0.5 text-sm hover:bg-qb-row-hover ${a.parentNumber ? 'pl-7' : ''}`} title={a.description}>
                              <input type="checkbox" checked={fixed || selected.has(a.number)} disabled={fixed || busy} onChange={() => toggle(a)} />
                              <span className="w-[44px] shrink-0 tabular-nums text-qb-ink-muted">{a.number}</span>
                              <span className="font-medium">{a.number === '10100' ? bankName.trim() || 'Checking' : a.name}</span>
                              <span className="truncate text-xs text-qb-ink-muted">{fixed ? 'Always created' : a.description}</span>
                            </label>
                          );
                        })}
                      </fieldset>
                    ))}
                  </div>
                </>
              ) : (
                <p className="text-sm text-qb-ink-muted">
                  Choose an industry to see its suggested accounts. If none fits, choose General Product-based Business or General Service-based Business at the end of the list.
                </p>
              )}
            </div>
          )}

          {tab === 'expert' && (
            <div className="space-y-2 text-sm">
              <p className="text-qb-ink-muted">These start from what's usual for your industry. You can change them later in Settings.</p>
              <label className="flex items-start gap-2">
                <input type="checkbox" checked={salesTax} onChange={(e) => setSalesTax(e.target.checked)} disabled={busy} />
                <span>
                  <span className="font-medium">Customers are charged sales tax</span>
                  <span className="block text-qb-ink-muted">Turns on sales tax and adds a Sales Tax Payable account. Add your tax rates afterwards in Sales Tax.</span>
                </span>
              </label>
              <label className="flex items-start gap-2">
                <input type="checkbox" checked={inventory} onChange={(e) => setInventory(e.target.checked)} disabled={busy} />
                <span>
                  <span className="font-medium">Inventory and purchase orders are used</span>
                  <span className="block text-qb-ink-muted">Adds the Inventory Asset and Cost of Goods Sold accounts for tracking products on hand.</span>
                </span>
              </label>
            </div>
          )}
        </div>

        {error && <div className="mt-2 shrink-0 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex shrink-0 items-center gap-2">
          <button
            type="button"
            className="qb-btn qb-btn-secondary"
            onClick={() => create(false)}
            disabled={busy || !info.name.trim()}
            title="Create the company with LocalBooks' standard chart of accounts and set it up yourself"
          >
            Skip — use the standard chart
          </button>
          <span className="flex-1" />
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          {tabIndex > 0 && (
            <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setTab(SETUP_TABS[tabIndex - 1][0])} disabled={busy}>
              Back
            </button>
          )}
          {tabIndex < SETUP_TABS.length - 1 ? (
            <button type="submit" className="qb-btn qb-btn-primary" disabled={busy}>
              Next
            </button>
          ) : (
            <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !canFinish} title={canFinish ? undefined : 'Enter a company name and choose an industry first'}>
              {busy ? 'Creating…' : 'Save As…'}
            </button>
          )}
        </div>
      </form>
    </div>
  );
}

/**
 * New Company and Open or Restore, for the File menu and the buttons that offer them. Returns the
 * actions, the New Company dialog to render (or null), and the last error.
 */
export function useCompanyFileActions() {
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState(null);

  const newCompany = useCallback(() => {
    setError(null);
    setCreating(true);
  }, []);
  const openOrRestore = useCallback(async () => {
    setError(null);
    try {
      await openOrRestoreCompany();
    } catch (err) {
      setError(`This file can't be opened: ${errorText(err)}`);
    }
  }, []);

  // The File menu asks the focused window.
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    getCurrentWebviewWindow()
      .listen(FILE_MENU_REQUESTED, (event) => (event.payload === 'new' ? newCompany() : openOrRestore()))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [newCompany, openOrRestore]);

  const dialog = creating ? <NewCompanyDialog onClose={() => setCreating(false)} /> : null;
  return { newCompany, openOrRestore, dialog, error, clearError: () => setError(null) };
}

/** Shown when no company file is open: at first launch after Close Company, or after File › Close Company. */
export default function NoCompanyOpen() {
  const [screen, setScreen] = useState(null);
  const [loadError, setLoadError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [openError, setOpenError] = useState(null);
  const [finding, setFinding] = useState(false);
  const actions = useCompanyFileActions();

  useEffect(() => {
    api.getStartScreen().then(
      (data) => {
        setScreen(data);
        setSelected(data.recent.find((r) => !r.missing)?.path ?? null);
      },
      (err) => setLoadError(errorText(err)),
    );
  }, []);

  // A company was opened (here, from the File menu, or by double-clicking a file): show it.
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    listen(COMPANY_FILE_CHANGED, () => window.location.reload())
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const recent = screen?.recent ?? [];
  const current = recent.find((r) => r.path === selected) ?? null;

  const openRecent = async (row) => {
    if (!row || row.missing || busy) return;
    setBusy(true);
    setOpenError(null);
    try {
      await api.openCompanyFile(row.path);
    } catch (err) {
      setOpenError(`Could not open ${row.companyName}: ${errorText(err)}`);
      setBusy(false);
    }
  };

  const forget = async (row) => {
    try {
      const next = await api.forgetRecentCompany(row.path);
      setScreen((s) => ({ ...s, recent: next }));
      if (selected === row.path) setSelected(next.find((r) => !r.missing)?.path ?? null);
    } catch (err) {
      setOpenError(errorText(err));
    }
  };

  const error = openError ?? actions.error ?? loadError;
  const tile = 'flex h-[84px] w-[150px] flex-col items-center justify-center gap-1 rounded-xs border border-qb-border-strong bg-white px-2 text-center text-sm font-semibold leading-tight shadow-sm hover:bg-qb-row-hover disabled:opacity-50';

  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <section className="qb-panel flex w-full max-w-[860px] flex-col" aria-labelledby="no-company-title">
        <h1 id="no-company-title" className="px-3 pt-3 text-[28px] font-light leading-tight">
          No Company Open
        </h1>
        {screen?.warning && <Notice kind="info" text={screen.warning} />}
        {error && <Notice kind="error" text={error} onDismiss={() => (setOpenError(null), actions.clearError(), setLoadError(null))} />}
        <div className="px-3 pb-1 pt-1 text-sm font-semibold">Select a company that you’ve previously opened and click Open</div>
        <div className="flex gap-3 px-3">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <div className="h-[200px] overflow-auto border border-qb-border scrollbar-thin">
              <table className="qb-grid">
                <colgroup>
                  <col />
                  <col className="w-[150px]" />
                  <col className="w-[90px]" />
                  {editing && <col className="w-[80px]" />}
                </colgroup>
                <thead>
                  <tr>
                    <th className="qb-grid-head">Company name</th>
                    <th className="qb-grid-head">Last modified</th>
                    <th className="qb-grid-head text-right">File size</th>
                    {editing && <th className="qb-grid-head" aria-label="Remove from list" />}
                  </tr>
                </thead>
                <tbody>
                  {screen && recent.length === 0 && (
                    <tr>
                      <td colSpan={editing ? 4 : 3} className="qb-grid-cell text-qb-ink-muted">
                        No company files opened yet. Create a new company, open an existing one, or find a company file.
                      </td>
                    </tr>
                  )}
                  {recent.map((row) => (
                    <tr
                      key={row.path}
                      className={`qb-grid-row cursor-default ${row.path === selected ? 'bg-qb-green-tint' : ''} ${row.missing ? 'text-qb-ink-muted' : ''}`}
                      aria-selected={row.path === selected}
                      onClick={() => setSelected(row.path)}
                      onDoubleClick={() => openRecent(row)}
                      title={row.path}
                    >
                      <td className="qb-grid-cell truncate">
                        {row.companyName}
                        {row.missing && <span className="ml-1.5 text-xs">(file not found)</span>}
                      </td>
                      <td className="qb-grid-cell">{row.lastModified ?? '—'}</td>
                      <td className="qb-grid-cell num">{formatBytes(row.fileSize)}</td>
                      {editing && (
                        <td className="qb-grid-cell cell-tight">
                          <button
                            type="button"
                            className="qb-btn qb-btn-secondary h-[20px] w-full"
                            onClick={(e) => {
                              e.stopPropagation();
                              forget(row);
                            }}
                            aria-label={`Remove ${row.companyName} from the list`}
                          >
                            Remove
                          </button>
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="min-h-[18px] break-all text-xs text-qb-ink-muted">
              <span className="font-semibold uppercase tracking-wide">Location:</span> {current?.location ?? ''}
            </div>
            {editing && (
              <div className="text-xs text-qb-ink-muted">Removing a company takes it off this list only. The company file itself is not deleted.</div>
            )}
          </div>
          <div className="flex w-[120px] flex-none flex-col items-center gap-2">
            <button type="button" className="qb-btn qb-btn-primary h-row-lg w-full" onClick={() => openRecent(current)} disabled={!current || current.missing || busy}>
              {busy ? 'Opening…' : 'Open'}
            </button>
            <button type="button" className="text-sm text-qb-link hover:underline disabled:opacity-50" onClick={() => setEditing((e) => !e)} disabled={recent.length === 0 && !editing}>
              {editing ? 'Done Editing' : 'Edit List'}
            </button>
          </div>
        </div>
        <div className="flex flex-wrap gap-2 px-3 pb-3 pt-2">
          <button type="button" className={tile} onClick={actions.newCompany} disabled={busy}>
            <Plus className="h-6 w-6 text-qb-green-active" aria-hidden="true" />
            Create a new company
          </button>
          <button type="button" className={tile} onClick={actions.openOrRestore} disabled={busy}>
            <FolderOpen className="h-6 w-6 text-qb-green-active" aria-hidden="true" />
            Open or restore an existing company
          </button>
          <button type="button" className={tile} onClick={() => setFinding(true)} disabled={busy}>
            <Search className="h-6 w-6 text-qb-green-active" aria-hidden="true" />
            Find a company file
          </button>
        </div>
      </section>
      {actions.dialog}
      {finding && <FindCompanyDialog onClose={() => setFinding(false)} />}
    </div>
  );
}

/** Find a Company File: searches this computer for company files and opens the one picked. */
function FindCompanyDialog({ onClose }) {
  const [found, setFound] = useState(null);
  const [error, setError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.findCompanyFiles().then(
      (files) => {
        setFound(files);
        setSelected(files[0]?.path ?? null);
      },
      (err) => setError(errorText(err)),
    );
  }, []);

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  const openFile = async (path) => {
    if (!path || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.openCompanyFile(path);
      // The window reloads on the company opened.
    } catch (err) {
      setError(`This file can't be opened: ${errorText(err)}`);
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Find a company file">
      <div className="qb-panel flex w-[720px] flex-col p-3 shadow-panel">
        <h2 className="mb-1 text-md font-semibold">Find a company file</h2>
        <p className="mb-2 text-sm text-qb-ink-muted">
          {found === null && !error
            ? 'Searching your home folder, synced folders and connected drives…'
            : `Found ${found?.length ?? 0} company file${found?.length === 1 ? '' : 's'} on this computer.`}
        </p>
        <div className="h-[260px] overflow-auto border border-qb-border scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[220px]" />
              <col />
              <col className="w-[130px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">File</th>
                <th className="qb-grid-head">Location</th>
                <th className="qb-grid-head">Last modified</th>
              </tr>
            </thead>
            <tbody>
              {found?.length === 0 && (
                <tr>
                  <td colSpan={3} className="qb-grid-cell text-qb-ink-muted">
                    No company files found. Use Open or restore an existing company to browse for one.
                  </td>
                </tr>
              )}
              {(found ?? []).map((f) => (
                <tr
                  key={f.path}
                  className={`qb-grid-row cursor-default ${f.path === selected ? 'bg-qb-green-tint' : ''}`}
                  aria-selected={f.path === selected}
                  onClick={() => setSelected(f.path)}
                  onDoubleClick={() => openFile(f.path)}
                  title={f.path}
                >
                  <td className="qb-grid-cell truncate">{f.fileName}</td>
                  <td className="qb-grid-cell truncate text-xs">{f.location}</td>
                  <td className="qb-grid-cell">{f.lastModified ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={() => openFile(selected)} disabled={!selected || busy}>
            {busy ? 'Opening…' : 'Open'}
          </button>
        </div>
      </div>
    </div>
  );
}
