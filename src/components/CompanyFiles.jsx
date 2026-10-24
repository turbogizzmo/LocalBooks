import { useCallback, useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import Notice from './Notice.jsx';
import { api, COMPANY_FILE_CHANGED, errorText } from '../lib/api.js';

/** Sent by File › New Company… ("new") and File › Open or Restore Company… ("open"); see file_menu.rs. */
const FILE_MENU_REQUESTED = 'file-menu-requested';
const COMPANY_FILTER = { name: 'LocalBooks company file or backup', extensions: ['localbooks', 'db'] };

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
 * backup itself stays as it was. Resolves once the company is open (the window then reloads), or
 * when the user cancels.
 */
export async function openOrRestoreCompany() {
  const path = await open({ multiple: false, directory: false, title: 'Open or restore a company', filters: [COMPANY_FILTER] });
  if (!path) return;
  const name = path.split(/[\\/]/).pop().toLowerCase();
  if (name.endsWith('.localbooks') || name === 'localbooks.db') {
    await api.openCompanyFile(path);
    return;
  }
  const destination = await save({
    title: 'Restore the backup as a new company file',
    defaultPath: `${fileStem(path)}.localbooks`,
    filters: [{ name: 'LocalBooks company file', extensions: ['localbooks'] }],
  });
  if (destination) await api.restoreBackupAs(path, destination);
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

  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <section className="qb-panel flex w-full max-w-[820px] flex-col" aria-labelledby="no-company-title">
        <h1 id="no-company-title" className="border-b border-qb-border px-3 py-2 text-lg font-semibold">
          No Company Open
        </h1>
        {screen?.warning && <Notice kind="info" text={screen.warning} />}
        {error && <Notice kind="error" text={error} onDismiss={() => (setOpenError(null), actions.clearError(), setLoadError(null))} />}
        <div className="flex gap-3 px-3 py-2">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <div className="text-sm font-semibold">Select a company that you’ve previously opened and click Open</div>
            <div className="max-h-[320px] min-h-[180px] overflow-auto border border-qb-border scrollbar-thin">
              <table className="qb-grid">
                <colgroup>
                  <col />
                  <col className="w-[140px]" />
                  <col className="w-[80px]" />
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
                        No company files opened yet. Create a new company or open an existing one.
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
              {current && (
                <>
                  <span className="font-semibold">Location:</span> {current.path}
                </>
              )}
            </div>
            {editing && (
              <div className="text-xs text-qb-ink-muted">Removing a company takes it off this list only. The company file itself is not deleted.</div>
            )}
          </div>
          <div className="flex w-[260px] flex-none flex-col gap-1.5 pt-5">
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg justify-start px-3 text-left" onClick={actions.newCompany} disabled={busy}>
              Create a new company
            </button>
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg justify-start px-3 text-left" onClick={actions.openOrRestore} disabled={busy}>
              Open or restore an existing company
            </button>
          </div>
        </div>
        <div className="flex items-center gap-2 border-t border-qb-border px-3 py-2">
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" onClick={() => openRecent(current)} disabled={!current || current.missing || busy}>
            {busy ? 'Opening…' : 'Open'}
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={() => setEditing((e) => !e)} disabled={recent.length === 0 && !editing}>
            {editing ? 'Done Editing' : 'Edit List'}
          </button>
        </div>
      </section>
      {actions.dialog}
    </div>
  );
}
