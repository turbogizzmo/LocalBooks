import { useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';

/**
 * Help › Save Diagnostics: a zip of LocalBooks' version, the operating system, the company file's
 * schema and integrity check with row counts per table, and the diagnostics log (starts, stops,
 * files opened, crashes). Nothing from the books is in it, and nothing is sent anywhere: it's saved
 * where you choose, to send yourself to whoever is helping you. Show what's in it reads it first.
 */
export default function Diagnostics() {
  const [preview, setPreview] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const run = async (work) => {
    setBusy(true);
    try {
      await work();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const save = () =>
    run(async () => {
      const path = await api.saveDiagnostics();
      if (path) setNotice({ kind: 'success', text: `Saved ${path}. Nothing was sent: send it yourself if someone asked for it.` });
    });
  return (
    <section className="qb-panel flex min-h-0 flex-1 flex-col gap-2 p-3 text-sm">
      <h2 className="text-md font-semibold">Save diagnostics</h2>
      <p className="max-w-[720px]">
        If LocalBooks misbehaves, a diagnostics file helps whoever is helping you see why. It holds LocalBooks' version, your operating system, the company
        file's size, schema version and integrity check, how many rows each table has (counts only), and LocalBooks' own log of when it started, stopped,
        which file it opened and any crash. It holds no customers, vendors, amounts or other business data, and LocalBooks never sends it anywhere.
      </p>
      <div className="flex gap-2">
        <button type="button" className="qb-btn qb-btn-primary" onClick={save} disabled={busy}>
          Save diagnostics…
        </button>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => run(async () => setPreview(await api.previewDiagnostics()))} disabled={busy}>
          Show what's in it
        </button>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {preview && (
        <div className="min-h-0 flex-1 overflow-auto rounded-xs border border-qb-border bg-qb-canvas scrollbar-thin">
          {preview.map(([name, text]) => (
            <details key={name} open={name !== 'database.txt'} className="border-b border-qb-border px-2 py-1">
              <summary className="cursor-pointer font-semibold">{name}</summary>
              <pre className="whitespace-pre-wrap font-mono text-2xs">{text}</pre>
            </details>
          ))}
        </div>
      )}
    </section>
  );
}
