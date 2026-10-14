import { useCallback, useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

export const QBW_PROGRESS = 'qbw-import-progress';
const EXTENSIONS = ['qbw', 'qbb', 'qbm'];
const STAGES = ['Reading file', 'Decoding pages', 'Reading table catalog', 'Extracting line items', 'Staging rows'];
const STAMP = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** Overall 0–1 progress: each stage is an equal slice, filled by its own done/total. */
function overallFraction(p) {
  const index = Math.max(0, STAGES.indexOf(p.stage));
  const within = p.total > 0 ? Math.min(1, p.done / p.total) : 0;
  return (index + within) / STAGES.length;
}

function ProgressTrack({ progress }) {
  const pct = Math.round(overallFraction(progress) * 100);
  const current = STAGES.indexOf(progress.stage);
  return (
    <div className="flex flex-col gap-1 px-3 py-3" role="status" aria-live="polite">
      <div className="flex items-center justify-between text-sm">
        <span className="font-semibold">{progress.stage}…</span>
        <span className="tabular-nums text-qb-ink-muted">{pct}%</span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-sm bg-qb-canvas" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
        <div className="h-full bg-qb-green transition-[width] duration-150" style={{ width: `${pct}%` }} />
      </div>
      <ol className="flex flex-wrap gap-3 text-2xs text-qb-ink-faint">
        {STAGES.map((s, i) => (
          <li key={s} className={i < current ? 'text-qb-green-active' : i === current ? 'font-semibold text-qb-ink' : ''}>
            {i < current ? '✓ ' : ''}
            {s}
          </li>
        ))}
      </ol>
      <p className="text-2xs text-qb-ink-faint">Large files can take a few minutes. Keep LocalBooks open.</p>
    </div>
  );
}

/** A numbered step card. */
function Step({ number, title, state = 'active', aside, children }) {
  const badge =
    state === 'done' ? 'bg-qb-green text-white' : state === 'active' ? 'bg-qb-nav text-white' : 'bg-qb-canvas text-qb-ink-faint';
  return (
    <section className={`qb-panel ${state === 'locked' ? 'opacity-60' : ''}`}>
      <div className="flex min-h-[36px] items-center gap-2 border-b border-qb-border px-3 py-1">
        <span className={`flex h-5 w-5 flex-none items-center justify-center rounded-full text-xs font-semibold ${badge}`}>
          {state === 'done' ? '✓' : number}
        </span>
        <h3 className="text-md font-semibold">{title}</h3>
        {aside && <div className="ml-auto flex items-center gap-2">{aside}</div>}
      </div>
      {state !== 'locked' && children}
    </section>
  );
}

function ChooseFile({ busy, onFile }) {
  const [hover, setHover] = useState(false);

  useEffect(() => {
    let unlisten;
    let cancelled = false;
    // Native drag-and-drop delivers real file paths; without it (no Tauri window) the button still works.
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const { type, paths } = event.payload;
          if (type === 'enter' || type === 'over') setHover(true);
          else if (type === 'leave') setHover(false);
          else if (type === 'drop') {
            setHover(false);
            if (!busy && paths?.length) onFile(paths[0]);
          }
        })
        .then((fn) => (cancelled ? fn() : (unlisten = fn)))
        .catch(() => {});
    } catch {
      // Not running inside a Tauri webview.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [busy, onFile]);

  const browse = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'Desktop company file (.qbw)', extensions: EXTENSIONS }] });
    if (typeof path === 'string') onFile(path);
  };

  return (
    <div className="p-3">
      <div
        className={`flex flex-col items-center justify-center gap-2 border-2 border-dashed px-4 py-5 text-center ${
          hover ? 'border-qb-green bg-qb-green-tint' : 'border-qb-border bg-white'
        }`}
      >
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4 text-md" onClick={browse} disabled={busy}>
          Choose a .qbw company file…
        </button>
        <div className="text-xs text-qb-ink-muted">or drag the file here. It is only read, never changed, and nothing is added to your books yet.</div>
      </div>
    </div>
  );
}

function ImportsTable({ batches, openId, busy, onOpen, onReverse, onDelete }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col />
        <col className="w-[170px]" />
        <col className="w-[90px]" />
        <col className="w-[190px]" />
        <col className="w-[330px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">File</th>
          <th className="qb-grid-head">Read</th>
          <th className="qb-grid-head text-right">Lines</th>
          <th className="qb-grid-head">Status</th>
          <th className="qb-grid-head text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {batches.map((b) => {
          const status = b.liveEntryCount > 0 ? `Posted · ${b.liveEntryCount.toLocaleString()} entries` : b.postedAt ? 'Reversed' : 'Not posted yet';
          const tone = b.liveEntryCount > 0 ? 'text-qb-green-active' : b.postedAt ? 'text-qb-ink-muted' : 'text-qb-info';
          return (
            <tr key={b.id} className={b.id === openId ? 'bg-qb-green-tint' : ''}>
              <td className="qb-grid-cell font-semibold">{b.fileName}</td>
              <td className="qb-grid-cell text-qb-ink-muted">{STAMP.format(new Date(b.importedAt))}</td>
              <td className="qb-grid-cell num">{b.lineCount.toLocaleString()}</td>
              <td className={`qb-grid-cell font-semibold ${tone}`}>{status}</td>
              <td className="qb-grid-cell">
                <div className="flex justify-end gap-1">
                  <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy || b.id === openId} onClick={() => onOpen(b.id)}>
                    {b.id === openId ? 'Open below' : 'Open'}
                  </button>
                  {b.liveEntryCount > 0 && (
                    <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs text-qb-negative" disabled={busy} onClick={() => onReverse(b)}>
                      Reverse posted entries…
                    </button>
                  )}
                  <button
                    type="button"
                    className="qb-btn qb-btn-secondary h-row px-2 text-xs"
                    disabled={busy || b.liveEntryCount > 0}
                    title={b.liveEntryCount > 0 ? 'Reverse the posted entries first' : 'Delete this import (your books are not affected)'}
                    onClick={() => onDelete(b)}
                  >
                    Delete import
                  </button>
                </div>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function MappingGrid({ summary, accounts, onMap, busy }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col />
        <col className="w-[90px]" />
        <col className="w-[140px]" />
        <col className="w-[340px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Found in the .qbw file</th>
          <th className="qb-grid-head text-right">Lines</th>
          <th className="qb-grid-head text-right">Total</th>
          <th className="qb-grid-head">Post to this LocalBooks income account</th>
        </tr>
      </thead>
      <tbody>
        {summary.labels.map((l) => (
          <tr key={l.rawLabel} className={l.accountId ? '' : 'bg-[#fff8e6]'}>
            <td className="qb-grid-cell">
              <div className="font-semibold">{l.displayLabel}</div>
              <div className="text-2xs text-qb-ink-faint">{l.rawLabel}</div>
            </td>
            <td className="qb-grid-cell num">{l.lineCount.toLocaleString()}</td>
            <td className="qb-grid-cell num">{formatCents(l.totalCents)}</td>
            <td className="qb-grid-cell">
              <AccountSelect
                accounts={accounts}
                value={l.accountId}
                onChange={(id) => onMap(l.rawLabel, id)}
                filter={(a) => a.classification === 'INCOME'}
                placeholder="— Choose an account —"
                disabled={busy}
                aria-label={`Account for ${l.displayLabel}`}
              />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Reads a .qbw desktop company file into a holding area, maps what it found, then posts balanced entries. */
export default function QbwImporter({ accounts }) {
  const [batches, setBatches] = useState([]);
  const [batchId, setBatchId] = useState(null);
  const [summary, setSummary] = useState(null);
  const [progress, setProgress] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const [offsetId, setOffsetId] = useState(null);
  const [fallbackDate, setFallbackDate] = useState('');
  const ingesting = useRef(false);
  const list = accounts ?? [];

  useEffect(() => {
    if (offsetId == null) {
      const obe = list.find((a) => a.isSystem && a.fullName === 'Opening Balance Equity');
      if (obe) setOffsetId(obe.id);
    }
  }, [list, offsetId]);

  const refreshBatches = useCallback(async () => {
    try {
      setBatches(await api.listQbwBatches());
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  }, []);

  const loadSummary = useCallback(async (id) => {
    if (id == null) return setSummary(null);
    try {
      setSummary(await api.getQbwBatch(id));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  }, []);

  useEffect(() => {
    refreshBatches();
  }, [refreshBatches]);
  useEffect(() => {
    loadSummary(batchId);
  }, [batchId, loadSummary]);

  useEffect(() => {
    let unlisten;
    let cancelled = false;
    // Events travel separately from the command's reply; ignore any that land after it returns.
    listen(QBW_PROGRESS, (e) => ingesting.current && setProgress(e.payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const ingest = useCallback(
    async (path) => {
      const ext = path.split('.').pop()?.toLowerCase();
      if (!EXTENSIONS.includes(ext)) {
        setNotice({ kind: 'error', text: 'Choose a .qbw company file.' });
        return;
      }
      setBusy(true);
      setNotice(null);
      ingesting.current = true;
      setProgress({ stage: STAGES[0], done: 0, total: 1 });
      try {
        const result = await api.ingestRawQbw(path);
        setBatchId(result.id);
        setSummary(result);
        await refreshBatches();
        setNotice({
          kind: result.lineCount ? 'success' : 'info',
          text: result.lineCount
            ? `Read ${result.lineCount.toLocaleString()} invoice lines from ${result.invoiceCount.toLocaleString()} invoices. Nothing is in your books yet: review step 2 below.`
            : 'No invoice lines could be read from this file. Nothing was added to your books.',
        });
      } catch (err) {
        setNotice({ kind: 'error', text: errorText(err) });
      } finally {
        ingesting.current = false;
        setBusy(false);
        setProgress(null);
      }
    },
    [refreshBatches],
  );

  const run = async (fn, success) => {
    setBusy(true);
    setNotice(null);
    try {
      const value = await fn();
      if (success) setNotice({ kind: 'success', text: success(value) });
      await loadSummary(batchId);
      await refreshBatches();
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const reverse = (b) => {
    if (
      !window.confirm(
        `Reverse the import of ${b.fileName}?\n\nEach of its ${b.liveEntryCount.toLocaleString()} entries gets a reversing entry, so your balances return to what they were before the import. Accounts the importer created are deactivated. This can take a moment for large imports.`,
      )
    )
      return;
    run(
      () => api.reverseQbwBatch(b.id),
      (r) => `Reversed ${r.entriesVoided.toLocaleString()} entries${r.accountsDeactivated ? ` and deactivated ${r.accountsDeactivated} accounts the importer created` : ''}. Your balances are back to where they were.`,
    );
  };

  const remove = (b) => {
    if (!window.confirm(`Delete the import of ${b.fileName}? Its staged lines are removed. Your books are not affected.`)) return;
    run(() => api.discardQbwBatch(b.id), () => 'Import deleted.').then(() => {
      if (b.id === batchId) setBatchId(null);
    });
  };

  const unmapped = summary?.labels.filter((l) => !l.accountId).length ?? 0;
  const postable = summary ? summary.invoiceCount - summary.alreadyPostedInvoiceCount : 0;
  const ready = summary && summary.lineCount > 0 && unmapped === 0 && offsetId != null && postable > 0;
  const step1 = summary ? 'done' : 'active';
  const step2 = !summary ? 'locked' : unmapped === 0 && summary.lineCount > 0 ? 'done' : 'active';
  const step3 = !summary ? 'locked' : summary.liveEntryCount > 0 && postable === 0 ? 'done' : ready ? 'active' : 'locked';

  const post = () => {
    const total = formatCents(summary.totalCents);
    if (
      !window.confirm(
        `Post ${postable.toLocaleString()} journal entries (${total} in total) to your books?\n\nCheck this total against the original program’s reports first. You can undo it later with "Reverse posted entries".`,
      )
    )
      return;
    run(
      () => api.postQbwBatch({ batchId: summary.id, offsetAccountId: offsetId, undatedFallback: fallbackDate || null }),
      (r) =>
        `Posted ${r.entriesPosted.toLocaleString()} balanced entries (${formatCents(r.totalCents)}).` +
        (r.skippedUndated ? ` ${r.skippedUndated} invoices without a date were skipped.` : '') +
        (r.skippedZeroTotal ? ` ${r.skippedZeroTotal} zero-total invoices skipped.` : '') +
        (r.skippedAlreadyPosted ? ` ${r.skippedAlreadyPosted} were already posted.` : ''),
    );
  };

  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex items-center gap-2 px-3 py-2">
          <h2 className="text-lg font-semibold">Desktop Migration Wizard</h2>
          <span className="ml-auto text-xs text-qb-ink-muted">Direct .QBW data extractor · uses the open-source OpenQBW reader</span>
        </div>
        <p className="border-t border-qb-border px-3 py-1.5 text-xs text-qb-ink-muted">
          Brings in <strong>invoice line amounts only</strong> from desktop company files (.qbw). They cannot yet be read for customers, payments, bills or
          account names, so imported invoices post as income against an offset account you choose. Compare the totals with your
          original program’s reports before posting; anything posted can be reversed below.
        </p>
      </section>

      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}

      {batches.length > 0 && (
        <section className="qb-panel">
          <div className="flex min-h-[36px] items-center border-b border-qb-border px-3 py-1">
            <h3 className="text-md font-semibold">Your imports</h3>
            <span className="ml-2 text-xs text-qb-ink-muted">Open one to continue it. Delete removes the import only; Reverse undoes what it posted.</span>
          </div>
          <ImportsTable batches={batches} openId={batchId} busy={busy} onOpen={setBatchId} onReverse={reverse} onDelete={remove} />
        </section>
      )}

      <Step
        number={1}
        title="Choose a company file"
        state={progress ? 'active' : step1}
        aside={
          summary &&
          !progress && (
            <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy} onClick={() => setBatchId(null)}>
              Import another file
            </button>
          )
        }
      >
        {progress ? <ProgressTrack progress={progress} /> : !summary && <ChooseFile busy={busy} onFile={ingest} />}
        {summary && !progress && (
          <div className="px-3 py-1.5 text-sm">
            <span className="font-semibold">{summary.fileName}</span>
            <span className="text-qb-ink-muted"> · {formatBytes(summary.fileSizeBytes)} · read {STAMP.format(new Date(summary.importedAt))}</span>
          </div>
        )}
      </Step>

      <Step number={2} title="Review what was found and choose accounts" state={step2}>
        {summary && (
          <>
            <dl className="grid grid-cols-2 gap-x-3 gap-y-1 border-b border-qb-border px-3 py-2 text-xs sm:grid-cols-5">
              {[
                ['Invoices', summary.invoiceCount.toLocaleString()],
                ['Invoice lines', summary.lineCount.toLocaleString()],
                ['Total', formatCents(summary.totalCents)],
                ['Dates', summary.firstDate ? `${formatDate(summary.firstDate)} – ${formatDate(summary.lastDate)}` : 'none readable'],
                ['Ignored', `${(summary.ignoredLineCount + summary.skippedLineCount).toLocaleString()} lines`],
              ].map(([k, v]) => (
                <div key={k} className="min-w-0">
                  <dt className="text-2xs uppercase tracking-wide text-qb-ink-faint">{k}</dt>
                  <dd className="truncate font-semibold" title={v}>{v}</dd>
                </div>
              ))}
            </dl>
            {summary.ignoredLineCount + summary.skippedLineCount > 0 && (
              <p className="border-b border-qb-border px-3 py-1 text-2xs text-qb-ink-faint">
                Ignored: {summary.ignoredLineCount.toLocaleString()} matches outside invoice tables (payroll, audit, internal data) and{' '}
                {summary.skippedLineCount.toLocaleString()} invoice lines whose amount could not be decoded.
              </p>
            )}
            {summary.lineCount === 0 ? (
              <p className="px-3 py-2 text-sm text-qb-ink-muted">Nothing to map. Delete this import above.</p>
            ) : (
              <>
                <div className="flex items-center gap-2 px-3 py-1.5">
                  <span className="text-xs text-qb-ink-muted">
                    {unmapped === 0 ? '✓ Every line type has an account.' : `Choose an account for ${unmapped} line type${unmapped === 1 ? '' : 's'}, or:`}
                  </span>
                  {unmapped > 0 && (
                    <button
                      type="button"
                      className="qb-btn qb-btn-secondary h-row px-2 text-xs"
                      disabled={busy}
                      onClick={() => run(() => api.autoCreateQbwAccounts(summary.id), (n) => (n === 0 ? 'Matched existing accounts.' : `Created ${n} income account${n === 1 ? '' : 's'}.`))}
                    >
                      Create an income account for each
                    </button>
                  )}
                </div>
                <MappingGrid summary={summary} accounts={list} busy={busy} onMap={(label, id) => run(() => api.setQbwMapping(summary.id, label, id))} />
              </>
            )}
          </>
        )}
      </Step>

      <Step number={3} title="Post to your books" state={step3}>
        {summary && (
          <div className="flex flex-col gap-2 px-3 py-2">
            {summary.liveEntryCount > 0 && (
              <Notice kind="success" text={`${summary.liveEntryCount.toLocaleString()} entries from this import are in your books. Use “Reverse posted entries” above to undo them.`} />
            )}
            {postable > 0 && (
              <>
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  <label className="flex flex-col gap-0.5 text-xs">
                    <span className="font-semibold text-qb-ink-muted">Balance each invoice against</span>
                    <AccountSelect accounts={list} value={offsetId} onChange={setOffsetId} filter={(a) => a.classification !== 'INCOME' && a.classification !== 'EXPENSE'} />
                    <span className="text-2xs text-qb-ink-faint">Opening Balance Equity is usual: the file has no customer payments, so these sales should not sit in A/R.</span>
                  </label>
                  <label className="flex flex-col gap-0.5 text-xs">
                    <span className="font-semibold text-qb-ink-muted">Date for invoices without one ({summary.undatedInvoiceCount.toLocaleString()})</span>
                    <input type="date" className="qb-input" value={fallbackDate} onChange={(e) => setFallbackDate(e.target.value)} disabled={summary.undatedInvoiceCount === 0} />
                    <span className="text-2xs text-qb-ink-faint">Leave blank to skip them.</span>
                  </label>
                </div>
                <div className="flex items-center gap-3">
                  <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={busy || !ready} onClick={post}>
                    Post {postable.toLocaleString()} entries ({formatCents(summary.totalCents)})…
                  </button>
                  {!ready && unmapped > 0 && <span className="text-xs text-qb-ink-muted">Finish step 2 first.</span>}
                </div>
              </>
            )}
          </div>
        )}
      </Step>
    </div>
  );
}
