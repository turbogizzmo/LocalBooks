import { useCallback, useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { FolderOpen, Plus, Search } from 'lucide-react';
import Notice from './Notice.jsx';
import { api, COMPANY_FILE_CHANGED, errorText } from '../lib/api.js';

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

/** New Company: asks for the company's name, then where to save its file, and opens it. */
export function NewCompanyDialog({ onClose }) {
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && !busy && onClose();
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  const create = async (e) => {
    e.preventDefault();
    if (!name.trim()) return;
    setError(null);
    const path = await save({
      title: 'Save the new company file',
      defaultPath: `${safeFileName(name)}.localbooks`,
      filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }],
    });
    if (!path) return;
    setBusy(true);
    try {
      await api.createCompanyFile(path, name.trim());
      // The window reloads on the new company.
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="New company">
      <form className="qb-panel w-[440px] p-3 shadow-panel" onSubmit={create}>
        <h2 className="mb-1 text-md font-semibold">Create a new company</h2>
        <p className="mb-2 text-sm text-qb-ink-muted">
          A new, empty company file with the standard chart of accounts. You choose where to save it next; the company you have open now is not changed.
        </p>
        <label className="block">
          <span className="qb-label">Company name</span>
          <input className="qb-input w-full" value={name} onChange={(e) => setName(e.target.value)} autoFocus disabled={busy} />
        </label>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary" disabled={busy || !name.trim()}>
            {busy ? 'Creating…' : 'Save As…'}
          </button>
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
