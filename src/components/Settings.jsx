import { useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { formatDate, todayIso } from '../lib/dates.js';
import { UpdatesPanel } from './Updates.jsx';
import FormDesigner from './FormDesigner.jsx';
import EmailSettings from './EmailSettings.jsx';
import FileRepair from './FileRepair.jsx';
import CondenseData, { openArchive } from './CondenseData.jsx';
import { isQuickBooksFile, migrateQuickBooksFile, NewCompanyDialog, openOrRestoreCompany, QUICKBOOKS_FILTER_EXTENSIONS } from './CompanyFiles.jsx';
import { UsersSettings } from './Users.jsx';
import QbwImporter from './QbwImporter.jsx';
import OffsiteBackup from './OffsiteBackup.jsx';
import { canOpen, findNav } from './Sidebar.jsx';
import PreferencesWindow, { Group } from './PreferencesWindow.jsx';
import Diagnostics from './Diagnostics.jsx';
import { savedCopyText } from '../lib/plainCopy.js';

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
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

function FileAndData({ companyName, isAdmin, navigate }) {
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

  // Every file attached to a transaction, in one zip file (a folder per transaction).
  const saveAllAttachments = async () => {
    setBusy(true);
    try {
      const result = await api.saveAllAttachments();
      if (result) setNotice({ kind: 'success', text: savedCopyText(result[0], `${result[1]} attachment${result[1] === 1 ? '' : 's'}`) });
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
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'LocalBooks or QuickBooks backup', extensions: ['localbooks', 'db', ...QUICKBOOKS_FILTER_EXTENSIONS] }],
    });
    if (!path) return;
    // A QuickBooks backup can't replace these books: it becomes a new company through the migration wizard.
    if (!isQuickBooksFile(path)) return prepareRestore(path);
    try {
      await migrateQuickBooksFile(path);
    } catch (err) {
      setNotice({ kind: 'error', text: `This file can't be restored: ${errorText(err)}` });
    }
  };

  // Opens another company file in place for this session (the same as double-clicking it).
  const openCompanyFile = async (e) => {
    setBusy(true);
    setNotice(null);
    try {
      await openOrRestoreCompany(e?.altKey === true);
    } catch (err) {
      setNotice({ kind: 'error', text: `This file can't be opened: ${errorText(err)}` });
    } finally {
      setBusy(false);
    }
  };

  const [creatingCompany, setCreatingCompany] = useState(false);
  // Closes the books and every other window; this window shows the No Company Open screen.
  const closeCompany = async () => {
    setBusy(true);
    setNotice(null);
    try {
      await api.closeCompanyFile();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
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
    PRE_CONDENSE: 'Before condense',
    ARCHIVE: 'Archive (read-only)',
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
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={() => setCreatingCompany(true)} disabled={busy}>
            New Company…
          </button>
          {isAdmin && navigate && (
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={() => navigate('settings/import')} disabled={busy}>
              Desktop Migration Wizard…
            </button>
          )}
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={saveAllAttachments} disabled={busy} title="Every file attached to a transaction, in one zip file with a folder per transaction">
            Save All Attachments…
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={closeCompany} disabled={busy}>
            Close Company
          </button>
          <span className="text-xs text-qb-ink-muted">
            Backups are taken live: keep working while they run. Restores are checked for integrity, schema version, broken links and
            unbalanced entries before anything is replaced. Backups are saved as .localbooks files, which open in LocalBooks with a
            double-click; an opened file is used in place for this session.
          </span>
        </div>
        {pending && <RestoreConfirm info={pending.info} busy={busy} onConfirm={restore} onCancel={() => setPending(null)} />}
        {creatingCompany && <NewCompanyDialog onClose={() => setCreatingCompany(false)} />}
      </Group>
      <FileRepair onRebuilt={backups.reload} />
      <CondenseData isAdmin={isAdmin} onDone={backups.reload} />
      <CompanyFileLocation />
      <OffsiteBackup isAdmin={isAdmin} onRestored={backups.reload} />
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
                  {b.kind === 'ARCHIVE' ? (
                    <button
                      type="button"
                      className="qb-btn qb-btn-secondary h-[20px] w-full"
                      onClick={() => openArchive(b.path).catch((err) => setNotice({ kind: 'error', text: `The archive can't be opened: ${errorText(err)}` }))}
                      disabled={busy}
                    >
                      Open
                    </button>
                  ) : (
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full" onClick={() => prepareRestore(b.path)} disabled={busy}>
                      Restore
                    </button>
                  )}
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
                onClick={async () => {
                  if (await askConfirm('Issue a new access token? Assistants using the current token will stop working until you update them.', { confirmLabel: 'Issue New Token' })) {
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

/** Settings' tabs as `[id, label]`, from its sidebar pages (`settings/<id>`) the user may open. */
const tabsFor = (isAdmin) =>
  findNav('settings').item.children.filter((page) => canOpen(page.id, isAdmin)).map((page) => [page.id.slice('settings/'.length), page.label]);

/**
 * Settings, on the tab its page names (`tab`, from `settings/<tab>`). Each tab is its own page in the
 * sidebar; the tabs here go to those pages.
 */
export default function Settings({ accounts, companyName, intent, isAdmin = true, navigate, tab }) {
  const tabs = tabsFor(isAdmin);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="flex max-w-full flex-none gap-px self-start overflow-x-auto rounded-xs border border-qb-border-strong text-sm scrollbar-thin" role="tablist" aria-label="Settings">
        {tabs.map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => navigate(`settings/${id}`)}
            className={`flex-none whitespace-nowrap px-3 py-0.5 font-semibold ${tab === id ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
          >
            {label}
          </button>
        ))}
      </div>
      {!isAdmin && <Notice kind="info" text="Company-wide settings are managed by an Admin." />}
      {/* My Preferences are everyone's own: without the Preferences page, they sit with your sign-in. */}
      {(tab === 'preferences' || (!isAdmin && tab === 'users')) && <PreferencesWindow isAdmin={isAdmin} intent={intent} navigate={navigate} />}
      {tab === 'forms' && <FormDesigner />}
      {tab === 'email' && <EmailSettings />}
      {tab === 'data' && <FileAndData companyName={companyName} isAdmin={isAdmin} navigate={navigate} />}
      {tab === 'import' && <QbwImporter accounts={accounts} />}
      {tab === 'ai' && <LocalAiAccess />}
      {tab === 'users' && <UsersSettings />}
      {tab === 'updates' && <UpdatesPanel />}
      {tab === 'diagnostics' && <Diagnostics />}
    </div>
  );
}
