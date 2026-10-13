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
    <div className="flex flex-col gap-1 px-2 py-2" role="status" aria-live="polite">
      <div className="flex items-center justify-between text-sm">
        <span className="font-semibold">{progress.stage}…</span>
        <span className="tabular-nums text-qb-ink-muted">{pct}%</span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-sm bg-qb-canvas" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
        <div className="h-full bg-qb-green transition-[width] duration-150" style={{ width: `${pct}%` }} />
      </div>
      <ol className="flex gap-3 text-2xs text-qb-ink-faint">
        {STAGES.map((s, i) => (
          <li key={s} className={i < current ? 'text-qb-green-active' : i === current ? 'font-semibold text-qb-ink' : ''}>
            {i < current ? '✓ ' : ''}
            {s}
          </li>
        ))}
      </ol>
    </div>
  );
}

function DropZone({ busy, onFile }) {
  const [hover, setHover] = useState(false);

  useEffect(() => {
    let unlisten;
    let cancelled = false;
    // Native drag-and-drop delivers real file paths; without it (no Tauri window) Browse still works.
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
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'QuickBooks Desktop company file', extensions: EXTENSIONS }] });
    if (typeof path === 'string') onFile(path);
  };

  return (
    <div
      className={`flex flex-col items-center justify-center gap-1 border-2 border-dashed px-4 py-6 text-center ${
        hover ? 'border-qb-green bg-qb-green-tint' : 'border-qb-border bg-white'
      }`}
    >
      <div className="text-md font-semibold">Drop a QuickBooks Desktop company file here</div>
      <div className="text-xs text-qb-ink-muted">.qbw company files are read directly. The file is opened read-only and is never changed.</div>
      <button type="button" className="qb-btn-secondary mt-1" onClick={browse} disabled={busy}>
        Browse…
      </button>
    </div>
  );
}

function MappingGrid({ summary, accounts, onMap, busy }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col />
        <col className="w-[90px]" />
        <col className="w-[130px]" />
        <col className="w-[340px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Found in file</th>
          <th className="qb-grid-head text-right">Lines</th>
          <th className="qb-grid-head text-right">Total</th>
          <th className="qb-grid-head">LocalBooks income account</th>
        </tr>
      </thead>
      <tbody>
        {summary.labels.map((l) => (
          <tr key={l.rawLabel} className={l.accountId ? '' : 'bg-[#fff8e6]'}>
            <td className="qb-grid-cell">
              <div className="font-semibold">{l.displayLabel}</div>
              <div className="text-2xs text-qb-ink-faint">{l.rawLabel}</div>
            </td>
            <td className="qb-grid-cell text-right tabular-nums">{l.lineCount.toLocaleString()}</td>
            <td className="qb-grid-cell text-right tabular-nums">{formatCents(l.totalCents)}</td>
            <td className="qb-grid-cell">
              <AccountSelect
                accounts={accounts}
                value={l.accountId}
                onChange={(id) => onMap(l.rawLabel, id)}
                filter={(a) => a.classification === 'INCOME'}
                placeholder="— Not mapped —"
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

/** Reads a QuickBooks Desktop file into staging, maps what it found, then posts balanced entries. */
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
        setNotice({ kind: 'error', text: 'Choose a QuickBooks Desktop file (.qbw).' });
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
          kind: 'success',
          text: `Read ${result.lineCount.toLocaleString()} line items from ${result.invoiceCount.toLocaleString()} invoices in ${result.fileName}. Nothing has been posted yet.`,
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
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const unmapped = summary?.labels.filter((l) => !l.accountId).length ?? 0;
  const postable = summary ? summary.invoiceCount - summary.alreadyPostedInvoiceCount : 0;
  const ready = summary && unmapped === 0 && offsetId != null && postable > 0;

  return (
    <div className="flex h-full flex-col gap-2 overflow-auto p-2 scrollbar-thin">
      <section className="qb-panel">
        <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Direct QuickBooks Desktop Importer</h2>
          <span className="ml-auto text-xs text-qb-ink-muted">Powered by the open-source OpenQBW parser · invoice line items only</span>
        </div>
        {progress ? <ProgressTrack progress={progress} /> : <div className="p-2"><DropZone busy={busy} onFile={ingest} /></div>}
        {notice && (
          <div className="px-2 pb-2">
            <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />
          </div>
        )}
      </section>

      {batches.length > 0 && (
        <section className="qb-panel">
          <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
            <h3 className="text-sm font-semibold">Staged imports</h3>
            <select
              className="qb-input h-row w-[360px] text-sm"
              value={batchId ?? ''}
              onChange={(e) => setBatchId(e.target.value ? Number(e.target.value) : null)}
              aria-label="Staged import"
            >
              <option value="">Choose an import…</option>
              {batches.map((b) => (
                <option key={b.id} value={b.id}>
                  #{b.id} · {b.fileName} · {b.lineCount.toLocaleString()} lines{b.postedAt ? ' · posted' : ''}
                </option>
              ))}
            </select>
            {summary && (
              <button
                type="button"
                className="qb-btn-secondary ml-auto"
                disabled={busy}
                onClick={() => {
                  if (!window.confirm(`Discard staged import #${summary.id}? Entries already posted stay in the ledger.`)) return;
                  run(() => api.discardQbwBatch(summary.id), () => 'Staged rows discarded.').then(() => setBatchId(null));
                }}
              >
                Discard staged rows
              </button>
            )}
          </div>

          {summary && (
            <>
              <dl className="grid grid-cols-6 gap-x-3 border-b border-qb-border px-2 py-1 text-xs">
                {[
                  ['File', `${summary.fileName} (${formatBytes(summary.fileSizeBytes)})`],
                  ['Pages', summary.pageCount.toLocaleString()],
                  ['Invoices', summary.invoiceCount.toLocaleString()],
                  ['Line items', summary.lineCount.toLocaleString()],
                  ['Date range', summary.firstDate ? `${formatDate(summary.firstDate)} – ${formatDate(summary.lastDate)}` : '—'],
                  ['Not decodable', `${summary.skippedLineCount.toLocaleString()} lines`],
                ].map(([k, v]) => (
                  <div key={k} className="min-w-0">
                    <dt className="text-2xs uppercase tracking-wide text-qb-ink-faint">{k}</dt>
                    <dd className="truncate font-semibold" title={v}>{v}</dd>
                  </div>
                ))}
              </dl>

              <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
                <h3 className="text-sm font-semibold">Map to your chart of accounts</h3>
                <span className="text-xs text-qb-ink-muted">
                  {unmapped === 0 ? '✓ Every label is mapped' : `${unmapped} of ${summary.labels.length} labels unmapped`}
                </span>
                <button
                  type="button"
                  className="qb-btn-secondary ml-auto"
                  disabled={busy || unmapped === 0}
                  onClick={() => run(() => api.autoCreateQbwAccounts(summary.id), (n) => (n === 0 ? 'Mapped to existing accounts.' : `Created ${n} income account${n === 1 ? '' : 's'}.`))}
                >
                  Auto-Generate Missing Accounts
                </button>
              </div>
              <MappingGrid
                summary={summary}
                accounts={list}
                busy={busy}
                onMap={(label, id) => run(() => api.setQbwMapping(summary.id, label, id))}
              />

              <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-3 border-t border-qb-border px-2 py-2">
                <div className="grid grid-cols-2 gap-3">
                  <label className="flex flex-col gap-0.5 text-xs">
                    <span className="font-semibold text-qb-ink-muted">Offset account (debited for each invoice)</span>
                    <AccountSelect accounts={list} value={offsetId} onChange={setOffsetId} filter={(a) => a.classification !== 'INCOME' && a.classification !== 'EXPENSE'} />
                    <span className="text-2xs text-qb-ink-faint">
                      The file has no customer payments, so imported sales should not sit in A/R. Opening Balance Equity is the usual choice.
                    </span>
                  </label>
                  <label className="flex flex-col gap-0.5 text-xs">
                    <span className="font-semibold text-qb-ink-muted">Date for undated invoices ({summary.undatedInvoiceCount})</span>
                    <input type="date" className="qb-input" value={fallbackDate} onChange={(e) => setFallbackDate(e.target.value)} disabled={summary.undatedInvoiceCount === 0} />
                    <span className="text-2xs text-qb-ink-faint">Leave blank to skip invoices whose date could not be read.</span>
                  </label>
                </div>
                <div className="flex flex-col items-end gap-1">
                  <span className="text-xs text-qb-ink-muted">
                    {summary.alreadyPostedInvoiceCount > 0 && `${summary.alreadyPostedInvoiceCount.toLocaleString()} already posted · `}
                    {postable.toLocaleString()} to post
                  </span>
                  <button
                    type="button"
                    className="qb-btn-primary"
                    disabled={busy || !ready}
                    onClick={() =>
                      run(
                        () => api.postQbwBatch({ batchId: summary.id, offsetAccountId: offsetId, undatedFallback: fallbackDate || null }),
                        (r) =>
                          `All historical lines mapped successfully. Posted ${r.entriesPosted.toLocaleString()} balanced entries (${formatCents(r.totalCents)}).` +
                          (r.skippedUndated ? ` ${r.skippedUndated} undated skipped.` : '') +
                          (r.skippedZeroTotal ? ` ${r.skippedZeroTotal} zero-total skipped.` : '') +
                          (r.skippedAlreadyPosted ? ` ${r.skippedAlreadyPosted} already posted.` : ''),
                      )
                    }
                  >
                    Post balancing transactions to LocalBooks
                  </button>
                </div>
              </div>
            </>
          )}
        </section>
      )}
    </div>
  );
}
