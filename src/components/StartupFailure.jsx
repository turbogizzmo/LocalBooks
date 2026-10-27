import { useState } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { InstallButton, ReleaseNotes } from './Updates.jsx';
import { errorText } from '../lib/api.js';

/** What a startup error usually means, in words for the people keeping the books, and what to do. */
export function explainStartupError(message) {
  const m = String(message ?? '').toLowerCase();
  if (m.includes('os error 32') || m.includes('os error 33') || m.includes('used by another process') || m.includes('database is locked') || m.includes('busy')) {
    return 'Another program is using the company file right now, often a backup, sync (OneDrive, Dropbox, iCloud) or antivirus program, or another copy of LocalBooks. Wait a minute or close that program, then click Try again.';
  }
  if (m.includes('newer') || m.includes('was previously applied but is missing')) {
    return 'The company file was last saved by a newer version of LocalBooks. Click Check for updates to install it; your books will open as they were.';
  }
  if (m.includes('permission denied') || m.includes('os error 5') || m.includes('os error 13') || m.includes('access is denied') || m.includes('read-only')) {
    return 'LocalBooks isn’t allowed to open the file where it is. Check that the folder is still there and that you can save files in it (an unplugged drive or a disconnected network folder does this), then click Try again.';
  }
  if (m.includes('no such file') || m.includes('os error 2') || m.includes('cannot find')) {
    return 'The company file isn’t where LocalBooks last found it. If it is on a drive or network folder, connect it and click Try again.';
  }
  if (m.includes('not a database') || m.includes('malformed') || m.includes('corrupt')) {
    return 'The company file looks damaged. Don’t delete it: restore the newest copy from the backups folder next to it, or send the details below to support.';
  }
  return 'Click Try again. If it happens again, check for updates, and send the details below to support if that doesn’t help.';
}

/** Shown instead of the app when the company file could not be opened at launch. */
export default function StartupFailure({ failure }) {
  const [copied, setCopied] = useState(false);
  const [update, setUpdate] = useState(undefined); // undefined: not checked · null: up to date
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState(null);
  const details = `LocalBooks could not open its company file.\nFile: ${failure.companyFile}\nError: ${failure.message}`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(details);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  const checkUpdates = async () => {
    setChecking(true);
    setError(null);
    try {
      setUpdate((await check()) ?? null);
    } catch (err) {
      setError(`Could not check for updates: ${errorText(err)}`);
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <section className="qb-panel w-full max-w-[640px]" role="alert">
        <h1 className="border-b border-qb-border px-3 py-2 text-lg font-semibold">LocalBooks couldn’t open your company file</h1>
        <div className="flex flex-col gap-2 px-3 py-2 text-sm">
          <p className="text-md">{explainStartupError(failure.message)}</p>
          <p className="text-qb-ink-muted">Nothing was changed: a company file is only upgraded when every step succeeds, so your books are as you left them.</p>
          <div className="flex flex-wrap items-center gap-2">
            <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={() => relaunch().catch((err) => setError(errorText(err)))}>
              Try again
            </button>
            {update ? (
              <InstallButton update={update} onError={setError} />
            ) : (
              <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={checkUpdates} disabled={checking}>
                {checking ? 'Checking…' : 'Check for updates'}
              </button>
            )}
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={copy}>
              {copied ? '✓ Copied' : 'Copy details'}
            </button>
          </div>
          {update === null && <p className="text-qb-ink-muted">You have the latest version of LocalBooks.</p>}
          {update && (
            <div>
              <p>LocalBooks {update.version} is available. Install it, and LocalBooks restarts and opens your books.</p>
              <ReleaseNotes update={update} />
            </div>
          )}
          {error && <p className="text-qb-negative">{error}</p>}
          <details className="text-xs">
            <summary className="cursor-pointer text-qb-ink-muted">Technical details</summary>
            <dl className="mt-1 grid grid-cols-[90px_minmax(0,1fr)] gap-x-2 gap-y-1">
              <dt className="font-semibold text-qb-ink-muted">File</dt>
              <dd className="break-all font-mono">{failure.companyFile}</dd>
              <dt className="font-semibold text-qb-ink-muted">Reason</dt>
              <dd className="break-words font-mono text-qb-negative">{failure.message}</dd>
            </dl>
            <p className="mt-1 text-qb-ink-muted">
              Daily backups are kept in the <span className="font-mono">backups</span> folder next to the file, and the error is recorded in
              <span className="font-mono"> startup-error.log</span> there.
            </p>
          </details>
        </div>
      </section>
    </div>
  );
}
