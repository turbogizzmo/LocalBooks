import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import Notice from './Notice.jsx';

function formatBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

const when = (t) => (t ? t.replace('T', ' ').slice(0, 16) : 'never');

/**
 * Settings › File & Data › Offsite backup (`offsite.rs`): an encrypted copy of the books, made on
 * this computer, in a folder the user picks (a synced cloud folder, a USB drive). Set up with a
 * backup password; copies are made daily while the company is open; any copy can be test-restored
 * (checked without changing the books) or restored.
 */
export default function OffsiteBackup({ isAdmin, onRestored }) {
  const { data: status, error: loadError, reload } = useLedgerQuery(api.getOffsiteStatus);
  const [form, setForm] = useState(null);
  const [ask, setAsk] = useState(null);
  const [password, setPassword] = useState('');
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const settings = status?.settings ?? null;
  const keepOk = form && Number.isInteger(Number(form.keep)) && Number(form.keep) >= 1 && Number(form.keep) <= 100 && String(form.keep).trim() !== '';

  const act = async (fn, ok) => {
    setBusy(true);
    setNotice(null);
    try {
      const result = await fn();
      if (ok) setNotice({ kind: 'success', text: ok(result) });
      reload();
      return result;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      reload();
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder === 'string') setForm((f) => ({ ...f, folder }));
  };

  const saveSetup = async () => {
    if (form.password !== form.again) {
      setNotice({ kind: 'error', text: 'The two passwords are not the same.' });
      return;
    }
    const done = await act(() => api.setUpOffsiteBackup(form.folder, form.password, Number(form.keep)));
    if (done) {
      setForm(null);
      await act(() => api.offsiteBackupNow(), (c) => `Offsite backup is on. Saved the first copy, ${c.fileName}, to the folder.`);
    }
  };

  const confirmCopy = async () => {
    const { kind, copy } = ask;
    const pw = password;
    setPassword('');
    setAsk(null);
    if (kind === 'test') {
      await act(() => api.testOffsiteRestore(copy.path, pw), (info) => `${copy.fileName} opens and checks out: ${info.companyName ?? 'the company'}, ${info.journalEntries.toLocaleString()} entries, the last dated ${info.lastEntryDate ?? '—'}. Your books were not changed.`);
    } else {
      const r = await act(() => api.restoreOffsiteBackup(copy.path, pw), () => `Restored ${copy.fileName}. The books it replaced were saved first as a snapshot.`);
      if (r) onRestored?.();
    }
  };

  return (
    <section className="qb-panel flex min-h-0 flex-col" aria-label="Offsite backup">
      <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Offsite backup</h2>
        <span className="text-xs text-qb-ink-muted">Encrypted on this computer before it is saved, so a cloud folder holds only scrambled data</span>
        {settings && (
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" disabled={busy || !status.hasKey} onClick={() => act(() => api.offsiteBackupNow(), (c) => `Saved ${c.fileName} to the folder.`)}>
            Back up offsite now
          </button>
        )}
        {isAdmin && (
          <button type="button" className={`qb-btn qb-btn-secondary h-row ${settings ? '' : 'ml-auto'}`} disabled={busy} onClick={() => setForm({ folder: settings?.folder ?? '', password: '', again: '', keep: settings?.keep ?? 10 })}>
            {settings ? 'Change…' : 'Set up…'}
          </button>
        )}
        {isAdmin && settings && (
          <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => act(() => api.turnOffOffsiteBackup(), () => 'Offsite backup is off. The copies already in the folder are still there.')}>
            Turn off
          </button>
        )}
      </div>
      {loadError && <Notice kind="error" text={loadError} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {status && !settings && !form && (
        <p className="px-2 py-1.5 text-sm text-qb-ink-muted">
          Off. Set it up to keep encrypted copies somewhere other than this disk: a folder your cloud storage keeps in sync, or a USB drive. {isAdmin ? '' : 'An Admin can set it up.'}
        </p>
      )}
      {settings && (
        <div className="px-2 py-1.5 text-sm">
          <p>
            Saving to <span className="font-mono text-xs">{settings.folder}</span>, keeping the newest {settings.keep}. Last copy {when(settings.lastBackupAt)}; last test restore {when(settings.lastTestRestoreAt)}.
          </p>
          {!status.hasKey && <Notice kind="error" text="This computer doesn't have the backup key (the company file was moved, or the key was removed). Click Change… and enter the backup password again." />}
          {settings.lastError && <Notice kind="error" text={`The last copy failed: ${errorText(settings.lastError)}`} />}
        </div>
      )}
      {form && (
        <div className="flex flex-col gap-2 border-t border-qb-border px-2 py-2 text-sm">
          <div className="flex items-end gap-2">
            <label className="flex-1">
              <span className="qb-label">Folder</span>
              <input className="qb-input w-full" readOnly value={form.folder} placeholder="Choose a folder…" />
            </label>
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={pickFolder}>Choose…</button>
            <label>
              <span className="qb-label">Copies to keep</span>
              <input className="qb-input w-[80px]" type="number" min={1} max={100} step={1} value={form.keep} aria-invalid={!keepOk} onChange={(e) => setForm({ ...form, keep: e.target.value })} />
            </label>
          </div>
          <div className="flex items-end gap-2">
            <label>
              <span className="qb-label">Backup password</span>
              <input className="qb-input w-[260px]" type="password" autoComplete="new-password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
            </label>
            <label>
              <span className="qb-label">Again</span>
              <input className="qb-input w-[260px]" type="password" autoComplete="new-password" value={form.again} onChange={(e) => setForm({ ...form, again: e.target.value })} />
            </label>
          </div>
          <p className="text-xs text-qb-ink-muted">
            Use your sign-in password, the company’s recovery key, or another password of at least 8 characters. Restoring a copy, on this or any computer, needs it: Tillhouse
            can’t open a copy without it. This computer keeps the key in its {navigator.platform.startsWith('Mac') ? 'Keychain' : 'credential store'} so copies need no password.
          </p>
          <div className="flex gap-2">
            <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || !form.folder || form.password.length < 8 || !keepOk} onClick={saveSetup}>Save</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => setForm(null)}>Cancel</button>
          </div>
        </div>
      )}
      {settings && status.copies.length > 0 && (
        <table className="qb-grid">
          <thead>
            <tr>
              <th className="qb-grid-head">Copy</th>
              <th className="qb-grid-head w-[90px] text-right">Size</th>
              <th className="qb-grid-head w-[200px]" aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {status.copies.map((c) => (
              <tr key={c.path} className="qb-grid-row">
                <td className="qb-grid-cell font-mono text-xs" title={c.path}>{c.fileName}</td>
                <td className="qb-grid-cell num">{formatBytes(c.sizeBytes)}</td>
                <td className="qb-grid-cell cell-tight">
                  <div className="flex gap-1">
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] flex-1" disabled={busy} onClick={() => setAsk({ kind: 'test', copy: c })}>Test restore</button>
                    {isAdmin && (
                      <button type="button" className="qb-btn qb-btn-secondary h-[20px] flex-1" disabled={busy} onClick={() => setAsk({ kind: 'restore', copy: c })}>Restore</button>
                    )}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {settings && (
        <div className="border-t border-qb-border px-2 py-1">
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row"
            disabled={busy}
            onClick={async () => {
              const file = await open({ multiple: false, directory: false, filters: [{ name: 'Tillhouse offsite backup', extensions: ['lbbackup'] }] });
              if (typeof file === 'string') setAsk({ kind: 'test', copy: { path: file, fileName: file.split(/[\\/]/).pop() } });
            }}
          >
            Test another copy…
          </button>
        </div>
      )}
      {ask && (
        <div className="flex items-end gap-2 border-t border-qb-border bg-qb-canvas px-2 py-2 text-sm" role="dialog" aria-label={ask.kind === 'test' ? 'Test restore' : 'Restore'}>
          <label>
            <span className="qb-label">Backup password for {ask.copy.fileName}</span>
            <input className="qb-input w-[300px]" type="password" autoFocus value={password} onChange={(e) => setPassword(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && password && confirmCopy()} />
          </label>
          <button type="button" className="qb-btn qb-btn-primary h-row" disabled={!password || busy} onClick={confirmCopy}>
            {ask.kind === 'test' ? 'Test restore' : 'Replace current books'}
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => (setAsk(null), setPassword(''))}>Cancel</button>
          {ask.kind === 'restore' && <span className="text-xs text-qb-ink-muted">Your current books are saved as a snapshot first.</span>}
        </div>
      )}
    </section>
  );
}
