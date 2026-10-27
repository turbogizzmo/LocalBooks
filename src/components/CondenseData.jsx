import { useEffect, useState } from 'react';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { formatDate } from '../lib/dates.js';

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

function Checks({ verification }) {
  return (
    <ul className="border-t border-qb-border">
      {verification.checks.map((c) => (
        <li key={c.id} className="flex gap-2 border-b border-qb-border px-2 py-0.5 text-sm last:border-b-0">
          <span className={`w-4 flex-none text-center font-bold ${c.passed ? 'text-qb-green-active' : 'text-qb-negative'}`}>{c.passed ? '✓' : '✕'}</span>
          <span className="w-[260px] flex-none font-semibold">{c.title}</span>
          <span className="min-w-0 flex-1">{c.detail}</span>
        </li>
      ))}
    </ul>
  );
}

/** Opens an archive left by condensing (read-only) in place of the current books. */
export async function openArchive(path) {
  await api.openCompanyFile(path);
}

/** Condense Data, in File & Data Controls: archive closed years and continue in a smaller file. */
export default function CondenseData({ isAdmin, onDone }) {
  const status = useLedgerQuery(api.getCondenseStatus);
  const closing = useLedgerQuery(api.getClosingStatus);
  const [cutoff, setCutoff] = useState('');
  const [preview, setPreview] = useState(null);
  const [result, setResult] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);

  const closingDate = closing.data?.lockDate ?? null;
  useEffect(() => {
    if (closingDate && !cutoff) setCutoff(closingDate);
  }, [closingDate, cutoff]);

  if (status.data?.isArchive) {
    return (
      <section className="qb-panel">
        <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Condense data</h2>
        <div className="px-2 py-1.5 text-sm">
          This is an archive: the full history of books condensed through {formatDate(status.data.archiveThrough)}. It opens read-only, for
          reports and look-ups. Open your current company file (File › Open Previous Company) to keep working.
        </div>
      </section>
    );
  }

  const runPreview = async () => {
    setBusy(true);
    setNotice(null);
    setResult(null);
    try {
      setPreview(await api.previewCondense(cutoff));
    } catch (err) {
      setPreview(null);
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const run = async () => {
    const ok = await askConfirm(
      `Condense transactions through ${formatDate(cutoff)}?\n\nLocalBooks first saves a verified archive of your full history (it opens read-only) and a plain backup. ` +
        `It then builds the condensed file and checks it against the archive before switching to it. If any check fails, your books are left as they are.`,
      { confirmLabel: 'Condense' },
    );
    if (!ok) return;
    setBusy(true);
    setNotice(null);
    try {
      const done = await api.condenseCompanyFile(cutoff);
      setResult(done);
      setPreview(null);
      setNotice({
        kind: 'success',
        text: `Condensed through ${formatDate(cutoff)}: ${done.preview.entriesSummarized} transactions became ${done.preview.summaryEntries} monthly summary entries. ` +
          `The file went from ${formatBytes(done.sizeBefore)} to ${formatBytes(done.sizeAfter)}. The archive is ${done.archive.fileName}; ` +
          `the backup to undo with is ${done.safetyBackup.fileName}.`,
      });
      status.reload();
      onDone?.();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const open = async () => {
    setNotice(null);
    try {
      await openArchive(status.data.archivePath);
    } catch (err) {
      setNotice({ kind: 'error', text: `The archive can't be opened: ${errorText(err)}` });
    }
  };

  return (
    <section className="qb-panel">
      <h2 className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Condense data</h2>
      <div className="flex flex-wrap items-center gap-2 px-2 py-1.5 text-sm">
        <label htmlFor="condense-cutoff">Condense transactions on or before</label>
        <input
          id="condense-cutoff"
          type="date"
          className="qb-input h-row w-[140px]"
          value={cutoff}
          max={closingDate ?? undefined}
          onChange={(e) => {
            setCutoff(e.target.value);
            setPreview(null);
          }}
          disabled={busy || !isAdmin}
        />
        <button type="button" className="qb-btn qb-btn-secondary h-row px-3" onClick={runPreview} disabled={busy || !cutoff || !isAdmin}>
          Preview
        </button>
        <button type="button" className="qb-btn qb-btn-primary h-row px-3" onClick={run} disabled={busy || !preview || preview.blockers.length > 0}>
          {busy ? 'Working…' : 'Condense…'}
        </button>
        {status.data?.condensedThrough && (
          <span className="text-xs text-qb-ink-muted">
            Condensed through {formatDate(status.data.condensedThrough)}.{' '}
            {status.data.archiveExists ? (
              <button type="button" className="text-qb-info hover:underline" onClick={open}>
                Open archive (read-only)
              </button>
            ) : (
              'The archive is no longer where it was saved.'
            )}
          </span>
        )}
      </div>
      <div className="px-2 pb-1.5 text-xs text-qb-ink-muted">
        Replaces closed transactions up to the cut-off with one summary entry per month, keeping anything still open: unpaid invoices and
        bills, unused credits, undeposited payments, uncleared bank and card lines, and anything linked to a later transaction. Only closed
        periods can be condensed. {isAdmin ? '' : 'Only an Admin can condense the books.'}
      </div>
      {preview && (
        <div className="border-t border-qb-border px-2 py-1.5 text-sm">
          {preview.blockers.map((b) => (
            <Notice key={b} kind="error" text={b} />
          ))}
          <div>
            {preview.entriesBeforeCutoff} posted transactions are dated on or before {formatDate(preview.cutoff)}.{' '}
            <strong>{preview.entriesSummarized}</strong> would become <strong>{preview.summaryEntries}</strong> monthly summary entries.
          </div>
          {preview.kept.length > 0 && (
            <ul className="ml-4 list-disc">
              {preview.kept.map((k) => (
                <li key={k.reason}>
                  {k.entries} kept: {k.description}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {result && <Checks verification={result.verification} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
    </section>
  );
}
