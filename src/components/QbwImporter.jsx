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
const STAGES = ['Unpacking backup', 'Reading file', 'Decoding pages', 'Reading table catalog', 'Extracting line items', 'Decoding the full ledger', 'Staging rows'];
/** Stages that only some files go through: shown once they start. */
const OPTIONAL_STAGES = ['Unpacking backup', 'Decoding the full ledger'];
const ACCOUNT_TYPE_NAMES = {
  BANK: 'Bank',
  ACCOUNTS_RECEIVABLE: 'Accounts Receivable',
  OTHER_CURRENT_ASSET: 'Other Current Asset',
  FIXED_ASSET: 'Fixed Asset',
  OTHER_ASSET: 'Other Asset',
  ACCOUNTS_PAYABLE: 'Accounts Payable',
  CREDIT_CARD: 'Credit Card',
  OTHER_CURRENT_LIABILITY: 'Other Current Liability',
  LONG_TERM_LIABILITY: 'Long Term Liability',
  EQUITY: 'Equity',
  INCOME: 'Income',
  COST_OF_GOODS_SOLD: 'Cost of Goods Sold',
  EXPENSE: 'Expense',
  OTHER_INCOME: 'Other Income',
  OTHER_EXPENSE: 'Other Expense',
};
const STAMP = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'short' });

function formatBytes(n) {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** Overall 0–1 progress: each stage is an equal slice, filled by its own done/total. */
function overallFraction(p, stages) {
  const index = Math.max(0, stages.indexOf(p.stage));
  const within = p.total > 0 ? Math.min(1, p.done / p.total) : 0;
  return (index + within) / stages.length;
}

function ProgressTrack({ progress, seen }) {
  const stages = STAGES.filter((s) => !OPTIONAL_STAGES.includes(s) || seen.includes(s));
  const pct = Math.round(overallFraction(progress, stages) * 100);
  const current = stages.indexOf(progress.stage);
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
        {stages.map((s, i) => (
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
    const path = await open({ multiple: false, directory: false, filters: [{ name: 'Desktop company file or backup (.qbw, .qbb)', extensions: EXTENSIONS }] });
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
          Choose a company file or backup…
        </button>
        <div className="text-xs text-qb-ink-muted">
          A .qbw company file or a .qbb backup, or drag it here. It is only read, never changed, and nothing is added to your books yet.
        </div>
      </div>
    </div>
  );
}

function ImportsTable({ batches, openKey, busy, onOpen, onReverse, onStartOver, onDelete }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col />
        <col className="w-[170px]" />
        <col className="w-[90px]" />
        <col className="w-[190px]" />
        <col className="w-[420px]" />
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
          const key = `${b.kind}-${b.id}`;
          const isOpen = openKey === key;
          const status = b.liveEntryCount > 0 ? `Posted · ${b.liveEntryCount.toLocaleString()} entries` : b.postedAt ? 'Reversed' : 'Not posted yet';
          const tone = b.liveEntryCount > 0 ? 'text-qb-green-active' : b.postedAt ? 'text-qb-ink-muted' : 'text-qb-info';
          return (
            <tr key={key} className={isOpen ? 'bg-qb-green-tint' : ''}>
              <td className="qb-grid-cell">
                <div className="font-semibold">{b.fileName}</div>
                <div className="text-2xs text-qb-ink-faint">{b.kind === 'ledger' ? 'Full ledger' : 'Invoice lines'}</div>
              </td>
              <td className="qb-grid-cell text-qb-ink-muted">{STAMP.format(new Date(b.importedAt))}</td>
              <td className="qb-grid-cell num">{b.lineCount.toLocaleString()}</td>
              <td className={`qb-grid-cell font-semibold ${tone}`}>{status}</td>
              <td className="qb-grid-cell">
                <div className="flex justify-end gap-1">
                  <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy || isOpen} onClick={() => onOpen(b)}>
                    {isOpen ? 'Open below' : 'Open'}
                  </button>
                  {b.liveEntryCount > 0 && b.kind === 'ledger' && (
                    <button
                      type="button"
                      className="qb-btn qb-btn-secondary h-row px-2 text-xs text-qb-negative"
                      disabled={busy || !b.startOver}
                      title={
                        b.startOver
                          ? `Put back the copy of your books saved just before this import (${STAMP.format(new Date(b.startOver.takenAt))})`
                          : 'No copy was saved before this import was posted; use Reverse posted entries'
                      }
                      onClick={() => onStartOver(b)}
                    >
                      Start over…
                    </button>
                  )}
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

/** Source accounts of a full-ledger import, each mapped to a LocalBooks account. */
function LedgerAccountGrid({ summary, accounts, onMap, busy }) {
  const used = summary.accounts.filter((a) => a.lineCount > 0);
  return (
    <table className="qb-grid">
      <colgroup>
        <col />
        <col className="w-[150px]" />
        <col className="w-[70px]" />
        <col className="w-[130px]" />
        <col className="w-[130px]" />
        <col className="w-[300px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Account in the company file</th>
          <th className="qb-grid-head">Type</th>
          <th className="qb-grid-head text-right">Lines</th>
          <th className="qb-grid-head text-right">Read</th>
          <th className="qb-grid-head text-right">File’s balance</th>
          <th className="qb-grid-head">Post to this LocalBooks account</th>
        </tr>
      </thead>
      <tbody>
        {used.map((a) => {
          // The file stores debits as negative; show balances the way the books do.
          const read = -a.readBalanceCents;
          const stored = a.storedBalanceCents == null ? null : -a.storedBalanceCents;
          const differs = stored != null && stored !== read;
          return (
            <tr key={a.sourceId} className={a.accountId ? '' : 'bg-[#fff8e6]'}>
              <td className="qb-grid-cell">
                <div className="font-semibold">{a.fullName}</div>
                {(!a.isActive || a.createdByImport) && (
                  <div className="text-2xs text-qb-ink-faint">
                    {[!a.isActive && 'inactive in the file', a.createdByImport && 'created by this import'].filter(Boolean).join(' · ')}
                  </div>
                )}
              </td>
              <td className="qb-grid-cell text-qb-ink-muted">{ACCOUNT_TYPE_NAMES[a.accountType] ?? a.accountType}</td>
              <td className="qb-grid-cell num">{a.lineCount.toLocaleString()}</td>
              <td className={`qb-grid-cell num ${differs ? 'font-semibold text-qb-negative' : ''}`}>{formatCents(read)}</td>
              <td className="qb-grid-cell num text-qb-ink-muted" title={differs ? `Differs by ${formatCents(read - stored)}` : undefined}>
                {stored == null ? '—' : formatCents(stored)}
              </td>
              <td className="qb-grid-cell">
                <AccountSelect
                  accounts={accounts}
                  value={a.accountId}
                  onChange={(id) => onMap(a.sourceId, id)}
                  filter={(x) => x.accountType === a.accountType}
                  placeholder="— Choose an account —"
                  disabled={busy}
                  aria-label={`Account for ${a.fullName}`}
                />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Transactions with a line that could not be read: listed for manual entry, never posted. */
function UnbalancedList({ items }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[110px]" />
        <col className="w-[160px]" />
        <col className="w-[90px]" />
        <col />
        <col className="w-[130px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Date</th>
          <th className="qb-grid-head">Type</th>
          <th className="qb-grid-head">Number</th>
          <th className="qb-grid-head">Name</th>
          <th className="qb-grid-head text-right">Missing</th>
        </tr>
      </thead>
      <tbody>
        {items.map((t, i) => (
          <tr key={i}>
            <td className="qb-grid-cell">{formatDate(t.date)}</td>
            <td className="qb-grid-cell">{t.kind}</td>
            <td className="qb-grid-cell">{t.docNum ?? ''}</td>
            <td className="qb-grid-cell">{t.name ?? ''}</td>
            <td className="qb-grid-cell num">{formatCents(Math.abs(t.imbalanceCents))}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Steps 2 and 3 for a full-ledger import. */
function LedgerReview({ summary, accounts, busy, run }) {
  const used = summary.accounts.filter((a) => a.lineCount > 0);
  const unmapped = used.filter((a) => !a.accountId).length;
  const matched = summary.accountsMatchingStored;
  const withStored = summary.accountsWithStoredBalance;
  const step2 = unmapped === 0 ? 'done' : 'active';
  const step3 = summary.liveEntryCount > 0 && summary.postableCount === 0 ? 'done' : unmapped === 0 && summary.postableCount > 0 ? 'active' : 'locked';

  const post = () => {
    if (
      !window.confirm(
        `Post ${summary.postableCount.toLocaleString()} journal entries to your books?\n\nEach is one transaction from the company file, balanced as it was there. Compare the account balances above with the original program’s trial balance first. You can undo it later with "Reverse posted entries".`,
      )
    )
      return;
    run(
      () => api.postLedgerBatch({ batchId: summary.id }),
      (r) =>
        `Posted ${r.entriesPosted.toLocaleString()} entries (${r.linesPosted.toLocaleString()} lines).` +
        (r.skippedUnbalanced ? ` ${r.skippedUnbalanced} incomplete transactions were left out; enter them by hand.` : '') +
        (r.skippedAlreadyPosted ? ` ${r.skippedAlreadyPosted} were already posted.` : ''),
    );
  };

  return (
    <>
      <Step number={2} title="Review what was read and choose accounts" state={step2}>
        <dl className="grid grid-cols-2 gap-x-3 gap-y-1 border-b border-qb-border px-3 py-2 text-xs sm:grid-cols-5">
          {[
            ['Transactions', (summary.transactionCount - summary.unbalanced.length).toLocaleString()],
            ['Lines', summary.lineCount.toLocaleString()],
            ['Dates', summary.firstDate ? `${formatDate(summary.firstDate)} – ${formatDate(summary.lastDate)}` : 'none'],
            ['Accounts used', used.length.toLocaleString()],
            ['Balances matching the file', withStored ? `${matched} of ${withStored}` : '—'],
          ].map(([k, v]) => (
            <div key={k} className="min-w-0">
              <dt className="text-2xs uppercase tracking-wide text-qb-ink-faint">{k}</dt>
              <dd className="truncate font-semibold" title={v}>{v}</dd>
            </div>
          ))}
        </dl>
        <p className="border-b border-qb-border px-3 py-1 text-2xs text-qb-ink-faint">
          {summary.byKind.map(([k, n]) => `${n.toLocaleString()} ${k.toLowerCase()}${n === 1 ? '' : 's'}`).join(' · ')}. Read from{' '}
          {summary.decodedPages.toLocaleString()} of {summary.rowPages.toLocaleString()} data pages
          {summary.unreadableLines > 0 && `; ${summary.unreadableLines.toLocaleString()} lines could not be read`}.
        </p>
        {summary.unbalanced.length > 0 && (
          <div className="border-b border-qb-border">
            <p className="px-3 py-1.5 text-xs">
              <strong>
                {summary.unbalanced.length} transaction{summary.unbalanced.length === 1 ? '' : 's'} could not be read completely
              </strong>{' '}
              and will not be posted. Enter {summary.unbalanced.length === 1 ? 'it' : 'them'} by hand after posting the rest:
            </p>
            <UnbalancedList items={summary.unbalanced} />
          </div>
        )}
        <div className="flex items-center gap-2 px-3 py-1.5">
          <span className="text-xs text-qb-ink-muted">
            {unmapped === 0 ? '✓ Every account has a LocalBooks account.' : `Choose an account for ${unmapped} account${unmapped === 1 ? '' : 's'}, or:`}
          </span>
          {unmapped > 0 && (
            <button
              type="button"
              className="qb-btn qb-btn-secondary h-row px-2 text-xs"
              disabled={busy}
              onClick={() =>
                run(
                  () => api.autoMapLedgerAccounts(summary.id),
                  (r) => `Matched ${r.matched} existing account${r.matched === 1 ? '' : 's'} and created ${r.created}.`,
                )
              }
            >
              Match existing accounts and create the rest
            </button>
          )}
        </div>
        <LedgerAccountGrid summary={summary} accounts={accounts} busy={busy} onMap={(sourceId, id) => run(() => api.setLedgerAccount(summary.id, sourceId, id))} />
      </Step>

      <Step number={3} title="Post to your books" state={step3}>
        <div className="flex flex-col gap-2 px-3 py-2">
          {summary.liveEntryCount > 0 && (
            <Notice kind="success" text={`${summary.liveEntryCount.toLocaleString()} entries from this import are in your books. Use “Reverse posted entries” above to undo them.`} />
          )}
          {summary.postableCount > 0 && (
            <div className="flex items-center gap-3">
              <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={busy || unmapped > 0} onClick={post}>
                Post {summary.postableCount.toLocaleString()} entries…
              </button>
              {unmapped > 0 && <span className="text-xs text-qb-ink-muted">Finish step 2 first.</span>}
            </div>
          )}
        </div>
      </Step>
    </>
  );
}

/** Reads a desktop company file or backup into a holding area, maps what it found, then posts balanced entries. */
export default function QbwImporter({ accounts }) {
  const [batches, setBatches] = useState([]);
  const [opened, setOpened] = useState(null);
  const [summary, setSummary] = useState(null);
  const [progress, setProgress] = useState(null);
  const [seenStages, setSeenStages] = useState([]);
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
      const [invoices, ledgers] = await Promise.all([api.listQbwBatches(), api.listLedgerBatches()]);
      const all = [
        ...invoices.map((b) => ({ ...b, kind: 'invoices' })),
        ...ledgers.map((b) => ({ ...b, kind: 'ledger' })),
      ];
      all.sort((a, b) => b.importedAt.localeCompare(a.importedAt));
      setBatches(all);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  }, []);

  const loadSummary = useCallback(async (ref) => {
    if (ref == null) return setSummary(null);
    try {
      setSummary(ref.kind === 'ledger' ? await api.getLedgerBatch(ref.id) : await api.getQbwBatch(ref.id));
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  }, []);

  useEffect(() => {
    refreshBatches();
  }, [refreshBatches]);
  useEffect(() => {
    loadSummary(opened);
  }, [opened, loadSummary]);

  useEffect(() => {
    let unlisten;
    let cancelled = false;
    // Events travel separately from the command's reply; ignore any that land after it returns.
    listen(QBW_PROGRESS, (e) => {
      if (!ingesting.current) return;
      setProgress(e.payload);
      setSeenStages((seen) => (seen.includes(e.payload.stage) ? seen : [...seen, e.payload.stage]));
    })
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
        setNotice({ kind: 'error', text: 'Choose a .qbw company file or a .qbb backup.' });
        return;
      }
      setBusy(true);
      setNotice(null);
      ingesting.current = true;
      setSeenStages(ext === 'qbb' ? ['Unpacking backup'] : []);
      setProgress({ stage: ext === 'qbb' ? 'Unpacking backup' : 'Reading file', done: 0, total: 1 });
      try {
        const result = await api.ingestRawQbw(path);
        const batch = result.batch;
        setOpened({ kind: result.kind, id: batch.id });
        setSummary(batch);
        await refreshBatches();
        if (result.kind === 'ledger') {
          const complete = batch.transactionCount - batch.unbalanced.length;
          setNotice({
            kind: 'success',
            text:
              `Read the full ledger: ${complete.toLocaleString()} transactions and ${batch.accounts.filter((a) => a.lineCount > 0).length} accounts.` +
              (batch.unbalanced.length ? ` ${batch.unbalanced.length} could not be read completely and are listed below.` : '') +
              ' Nothing is in your books yet: review step 2 below.',
          });
        } else {
          setNotice({
            kind: batch.lineCount ? 'success' : 'info',
            text: batch.lineCount
              ? `Read ${batch.lineCount.toLocaleString()} invoice lines from ${batch.invoiceCount.toLocaleString()} invoices. Nothing is in your books yet: review step 2 below.`
              : 'No invoice lines could be read from this file. Nothing was added to your books.',
          });
        }
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
      await loadSummary(opened);
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
      () => (b.kind === 'ledger' ? api.reverseLedgerBatch(b.id) : api.reverseQbwBatch(b.id)),
      (r) => `Reversed ${r.entriesVoided.toLocaleString()} entries${r.accountsDeactivated ? ` and deactivated ${r.accountsDeactivated} accounts the importer created` : ''}. Your balances are back to where they were.`,
    );
  };

  // Restores the copy of the company file saved just before the import posted. Two confirmations,
  // the second typed, because everything recorded since that moment is undone too.
  const startOver = async (b) => {
    const when = STAMP.format(new Date(b.startOver.takenAt));
    if (
      !window.confirm(
        `Start over: put your books back to how they were just before ${b.fileName} was posted (${when})?\n\n` +
          `This removes the ${b.liveEntryCount.toLocaleString()} imported entries, with no reversing entries left behind. The accounts set up for the import stay, so it can be posted again.\n\n` +
          `It also undoes ANYTHING else recorded since ${when}: invoices, payments, edits, other imports.\n\n` +
          `Your books as they are now are saved as a backup first (Settings › Backups), so this can be undone.`,
      )
    )
      return;
    const typed = window.prompt('This replaces your company file. Type START OVER to confirm.');
    if (typed?.trim().toUpperCase() !== 'START OVER') {
      if (typed !== null) setNotice({ kind: 'info', text: 'Start over cancelled: the confirmation text did not match. Nothing was changed.' });
      return;
    }
    setBusy(true);
    setNotice(null);
    try {
      const result = await api.startOverLedgerBatch(b.id);
      setNotice({
        kind: 'success',
        text: `Your books are back to how they were on ${when}. The import is still listed, not posted: post it again, or delete it and import a fresh copy. Your previous books were saved as ${result.safetyBackup.fileName}. Reloading…`,
      });
      setTimeout(() => window.location.reload(), 2500);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      setBusy(false);
    }
  };

  const remove = (b) => {
    if (!window.confirm(`Delete the import of ${b.fileName}? Its staged lines are removed. Your books are not affected.`)) return;
    run(() => (b.kind === 'ledger' ? api.discardLedgerBatch(b.id) : api.discardQbwBatch(b.id)), () => 'Import deleted.').then(() => {
      if (opened && b.kind === opened.kind && b.id === opened.id) setOpened(null);
    });
  };

  const isLedger = opened?.kind === 'ledger';
  const invoiceSummary = !isLedger ? summary : null;
  const unmapped = invoiceSummary?.labels.filter((l) => !l.accountId).length ?? 0;
  const postable = invoiceSummary ? invoiceSummary.invoiceCount - invoiceSummary.alreadyPostedInvoiceCount : 0;
  const ready = invoiceSummary && invoiceSummary.lineCount > 0 && unmapped === 0 && offsetId != null && postable > 0;
  const step1 = summary ? 'done' : 'active';
  const step2 = !invoiceSummary ? 'locked' : unmapped === 0 && invoiceSummary.lineCount > 0 ? 'done' : 'active';
  const step3 = !invoiceSummary ? 'locked' : invoiceSummary.liveEntryCount > 0 && postable === 0 ? 'done' : ready ? 'active' : 'locked';

  const post = () => {
    const total = formatCents(invoiceSummary.totalCents);
    if (
      !window.confirm(
        `Post ${postable.toLocaleString()} journal entries (${total} in total) to your books?\n\nCheck this total against the original program’s reports first. You can undo it later with "Reverse posted entries".`,
      )
    )
      return;
    run(
      () => api.postQbwBatch({ batchId: invoiceSummary.id, offsetAccountId: offsetId, undatedFallback: fallbackDate || null }),
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
          <span className="ml-auto text-xs text-qb-ink-muted">Direct .QBW data extractor · built on the open-source OpenQBW work</span>
        </div>
        <p className="border-t border-qb-border px-3 py-1.5 text-xs text-qb-ink-muted">
          Reads desktop company files (.qbw) and backups (.qbb). Newer files come in as the <strong>full ledger</strong>: the chart of accounts and every
          transaction, each posted as a balanced entry. Files the newer reader does not cover bring in <strong>invoice line amounts only</strong>, posted
          against an offset account you choose. Compare the totals with your original program’s reports before posting; anything posted can be reversed
          below.
        </p>
      </section>

      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}

      {batches.length > 0 && (
        <section className="qb-panel">
          <div className="flex min-h-[36px] items-center border-b border-qb-border px-3 py-1">
            <h3 className="text-md font-semibold">Your imports</h3>
            <span className="ml-2 text-xs text-qb-ink-muted">
              Open one to continue it. Start over puts your books back to just before the import; Reverse undoes what it posted; Delete removes the import only.
            </span>
          </div>
          <ImportsTable
            batches={batches}
            openKey={opened ? `${opened.kind}-${opened.id}` : null}
            busy={busy}
            onOpen={(b) => setOpened({ kind: b.kind, id: b.id })}
            onReverse={reverse}
            onStartOver={startOver}
            onDelete={remove}
          />
        </section>
      )}

      <Step
        number={1}
        title="Choose a company file"
        state={progress ? 'active' : step1}
        aside={
          summary &&
          !progress && (
            <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy} onClick={() => setOpened(null)}>
              Import another file
            </button>
          )
        }
      >
        {progress ? <ProgressTrack progress={progress} seen={seenStages} /> : !summary && <ChooseFile busy={busy} onFile={ingest} />}
        {summary && !progress && (
          <div className="px-3 py-1.5 text-sm">
            <span className="font-semibold">{summary.fileName}</span>
            <span className="text-qb-ink-muted"> · {formatBytes(summary.fileSizeBytes)} · read {STAMP.format(new Date(summary.importedAt))}</span>
          </div>
        )}
      </Step>

      {isLedger && summary ? (
        <LedgerReview summary={summary} accounts={list} busy={busy} run={run} />
      ) : (
        <>
          <Step number={2} title="Review what was found and choose accounts" state={step2}>
            {invoiceSummary && (
              <>
                <dl className="grid grid-cols-2 gap-x-3 gap-y-1 border-b border-qb-border px-3 py-2 text-xs sm:grid-cols-5">
                  {[
                    ['Invoices', invoiceSummary.invoiceCount.toLocaleString()],
                    ['Invoice lines', invoiceSummary.lineCount.toLocaleString()],
                    ['Total', formatCents(invoiceSummary.totalCents)],
                    ['Dates', invoiceSummary.firstDate ? `${formatDate(invoiceSummary.firstDate)} – ${formatDate(invoiceSummary.lastDate)}` : 'none readable'],
                    ['Ignored', `${(invoiceSummary.ignoredLineCount + invoiceSummary.skippedLineCount).toLocaleString()} lines`],
                  ].map(([k, v]) => (
                    <div key={k} className="min-w-0">
                      <dt className="text-2xs uppercase tracking-wide text-qb-ink-faint">{k}</dt>
                      <dd className="truncate font-semibold" title={v}>{v}</dd>
                    </div>
                  ))}
                </dl>
                {invoiceSummary.ignoredLineCount + invoiceSummary.skippedLineCount > 0 && (
                  <p className="border-b border-qb-border px-3 py-1 text-2xs text-qb-ink-faint">
                    Ignored: {invoiceSummary.ignoredLineCount.toLocaleString()} matches outside invoice tables (payroll, audit, internal data) and{' '}
                    {invoiceSummary.skippedLineCount.toLocaleString()} invoice lines whose amount could not be decoded.
                  </p>
                )}
                {invoiceSummary.lineCount === 0 ? (
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
                          onClick={() => run(() => api.autoCreateQbwAccounts(invoiceSummary.id), (n) => (n === 0 ? 'Matched existing accounts.' : `Created ${n} income account${n === 1 ? '' : 's'}.`))}
                        >
                          Create an income account for each
                        </button>
                      )}
                    </div>
                    <MappingGrid summary={invoiceSummary} accounts={list} busy={busy} onMap={(label, id) => run(() => api.setQbwMapping(invoiceSummary.id, label, id))} />
                  </>
                )}
              </>
            )}
          </Step>

          <Step number={3} title="Post to your books" state={step3}>
            {invoiceSummary && (
              <div className="flex flex-col gap-2 px-3 py-2">
                {invoiceSummary.liveEntryCount > 0 && (
                  <Notice kind="success" text={`${invoiceSummary.liveEntryCount.toLocaleString()} entries from this import are in your books. Use “Reverse posted entries” above to undo them.`} />
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
                        <span className="font-semibold text-qb-ink-muted">Date for invoices without one ({invoiceSummary.undatedInvoiceCount.toLocaleString()})</span>
                        <input type="date" className="qb-input" value={fallbackDate} onChange={(e) => setFallbackDate(e.target.value)} disabled={invoiceSummary.undatedInvoiceCount === 0} />
                        <span className="text-2xs text-qb-ink-faint">Leave blank to skip them.</span>
                      </label>
                    </div>
                    <div className="flex items-center gap-3">
                      <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={busy || !ready} onClick={post}>
                        Post {postable.toLocaleString()} entries ({formatCents(invoiceSummary.totalCents)})…
                      </button>
                      {!ready && unmapped > 0 && <span className="text-xs text-qb-ink-muted">Finish step 2 first.</span>}
                    </div>
                  </>
                )}
              </div>
            )}
          </Step>
        </>
      )}
    </div>
  );
}
