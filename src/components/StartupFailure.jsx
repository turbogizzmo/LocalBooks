import { useState } from 'react';

/** Shown instead of the app when the company file could not be opened at launch. */
export default function StartupFailure({ failure }) {
  const [copied, setCopied] = useState(false);
  const details = `LocalBooks could not open its company file.\nFile: ${failure.companyFile}\nError: ${failure.message}`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(details);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <section className="qb-panel w-full max-w-[640px]" role="alert">
        <h1 className="border-b border-qb-border px-3 py-2 text-lg font-semibold">LocalBooks couldn’t open your company file</h1>
        <div className="flex flex-col gap-2 px-3 py-2 text-sm">
          <p>Nothing was changed: a company file is only upgraded when every step succeeds, so your books are as you left them.</p>
          <dl className="grid grid-cols-[90px_minmax(0,1fr)] gap-x-2 gap-y-1">
            <dt className="font-semibold text-qb-ink-muted">File</dt>
            <dd className="break-all font-mono text-xs">{failure.companyFile}</dd>
            <dt className="font-semibold text-qb-ink-muted">Reason</dt>
            <dd className="break-words font-mono text-xs text-qb-negative">{failure.message}</dd>
          </dl>
          <p className="text-qb-ink-muted">
            This is often fixed by updating LocalBooks (the file may come from a newer version). Daily backups are kept in the
            <span className="font-mono"> backups </span>folder next to the file, and the error is recorded in
            <span className="font-mono"> startup-error.log </span>there.
          </p>
          <div>
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={copy}>
              {copied ? '✓ Copied' : 'Copy details'}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
