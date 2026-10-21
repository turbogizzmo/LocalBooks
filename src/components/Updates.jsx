import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import Notice from './Notice.jsx';
import { errorText } from '../lib/api.js';

const RELEASES_URL = 'https://github.com/turbogizzmo/LocalBooks/releases';
const AUTO_KEY = 'localbooks.autoUpdateCheck';

// Checking automatically is a per-computer choice, so it lives in this computer's storage rather
// than the company file. Storage can be unavailable; the default (on) applies then.
export function autoCheckEnabled() {
  try {
    return localStorage.getItem(AUTO_KEY) !== 'off';
  } catch {
    return true;
  }
}
function setAutoCheck(on) {
  try {
    localStorage.setItem(AUTO_KEY, on ? 'on' : 'off');
  } catch {
    // ignore: the setting just won't persist
  }
}

/**
 * Downloads and installs `update`, then restarts. The download's signature is verified against the
 * public key built into LocalBooks; an unsigned or tampered download is refused. A company file that
 * needs upgrading is copied to the backups folder the first time the new version opens it.
 */
async function install(update, onProgress) {
  let total = 0;
  let received = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === 'Started') total = event.data.contentLength ?? 0;
    if (event.event === 'Progress') {
      received += event.data.chunkLength;
      onProgress(total ? Math.min(99, Math.round((received / total) * 100)) : null);
    }
    if (event.event === 'Finished') onProgress(100);
  });
  await relaunch();
}

function ReleaseNotes({ update }) {
  if (!update.body) return null;
  return (
    <details className="mt-1 text-xs">
      <summary className="cursor-pointer text-qb-info">What’s new in {update.version}</summary>
      <pre className="mt-1 max-h-[200px] overflow-auto whitespace-pre-wrap rounded-xs border border-qb-border bg-white p-2 font-sans scrollbar-thin">{update.body}</pre>
    </details>
  );
}

function InstallButton({ update, onError }) {
  const [progress, setProgress] = useState(undefined);
  const busy = progress !== undefined;
  const run = async () => {
    setProgress(null);
    try {
      await install(update, setProgress);
    } catch (err) {
      setProgress(undefined);
      onError(errorText(err));
    }
  };
  return (
    <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy} onClick={run}>
      {busy ? (progress === null ? 'Downloading…' : progress === 100 ? 'Restarting…' : `Downloading… ${progress}%`) : 'Install and restart'}
    </button>
  );
}

/** Shown above every screen when a newer version is available (checked shortly after launch). */
export function UpdateBanner() {
  const [update, setUpdate] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    if (!autoCheckEnabled()) return undefined;
    let stale = false;
    // Off the startup path; a failed check (offline) stays silent here and shows in Settings.
    const t = setTimeout(() => {
      check().then(
        (u) => !stale && u && setUpdate(u),
        () => {},
      );
    }, 4000);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, []);

  if (!update) return null;
  return (
    <div className="no-print border-b border-qb-info/40 bg-[#eaf4fb] px-3 py-1.5 text-sm" role="status">
      <div className="flex flex-wrap items-center gap-2">
        <span>
          <strong>LocalBooks {update.version}</strong> is available (you have {update.currentVersion}).
        </span>
        <div className="ml-auto flex gap-2">
          <InstallButton update={update} onError={setError} />
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={() => setUpdate(null)}>
            Later
          </button>
        </div>
      </div>
      <ReleaseNotes update={update} />
      {error && <Notice kind="error" text={`The update could not be installed: ${error}`} onDismiss={() => setError(null)} />}
    </div>
  );
}

/** Settings › Updates: current version, automatic checks, and a manual check. */
export function UpdatesPanel() {
  const [version, setVersion] = useState('');
  const [auto, setAuto] = useState(autoCheckEnabled);
  const [state, setState] = useState({ kind: 'idle' });

  useEffect(() => {
    getVersion().then(setVersion, () => setVersion(''));
  }, []);

  const checkNow = async () => {
    setState({ kind: 'checking' });
    try {
      const update = await check();
      setState(update ? { kind: 'available', update } : { kind: 'current', at: new Date() });
    } catch (err) {
      setState({ kind: 'error', text: errorText(err) });
    }
  };

  return (
    <section className="qb-panel max-w-[720px]">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Updates</h2>
      <div className="flex flex-col gap-2 p-3 text-sm">
        <div>
          You are using <strong>LocalBooks {version || '…'}</strong>.{' '}
          <span className="text-qb-ink-muted">
            Release notes and installers: <span className="select-all font-mono text-xs">{RELEASES_URL}</span>
          </span>
        </div>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            className="accent-qb-green"
            checked={auto}
            onChange={(e) => {
              setAuto(e.target.checked);
              setAutoCheck(e.target.checked);
            }}
          />
          Check for updates when LocalBooks starts
        </label>
        <div className="flex items-center gap-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={checkNow} disabled={state.kind === 'checking'}>
            {state.kind === 'checking' ? 'Checking…' : 'Check for updates'}
          </button>
          {state.kind === 'current' && <span className="text-qb-ink-muted">LocalBooks is up to date.</span>}
        </div>
        {state.kind === 'available' && (
          <div className="rounded-xs border border-qb-info/40 bg-[#eaf4fb] p-2">
            <div className="flex flex-wrap items-center gap-2">
              <span>
                <strong>LocalBooks {state.update.version}</strong> is available.
              </span>
              <span className="ml-auto">
                <InstallButton update={state.update} onError={(text) => setState({ kind: 'error', text })} />
              </span>
            </div>
            <ReleaseNotes update={state.update} />
          </div>
        )}
        {state.kind === 'error' && <Notice kind="error" text={`Could not check for updates: ${state.text}`} onDismiss={() => setState({ kind: 'idle' })} />}
        <p className="text-xs text-qb-ink-muted">
          Updates come from LocalBooks’ GitHub releases and are installed only if their signature matches the key built into
          LocalBooks. Before a new version changes your company file, it saves a copy under Settings › File &amp; Data Controls
          (“Before upgrade”). Nothing about your books is sent when checking.
        </p>
      </div>
    </section>
  );
}
