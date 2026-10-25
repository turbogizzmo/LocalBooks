import { useCallback, useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';
import { askConfirm, askText } from '../lib/dialogs.jsx';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

export const QBW_PROGRESS = 'qbw-import-progress';
/** QuickBooks report exports: several can be read together (a report and its lists). */
const REPORT_EXTENSIONS = ['csv', 'xlsx', 'xls'];
const EXTENSIONS = ['qbw', 'qbb', 'qbm', 'iif', 'qif', ...REPORT_EXTENSIONS];
const FORMAT_NAMES = { QBW: 'company file', IIF: 'IIF export', QIF: 'QIF export', REPORT: 'QuickBooks report export' };
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
            if (!busy && paths?.length) onFile(paths);
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
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [{ name: 'Company file, backup or export (.qbw, .qbb, .iif, .qif, .csv, .xlsx)', extensions: EXTENSIONS }],
    });
    const paths = typeof picked === 'string' ? [picked] : picked;
    if (paths?.length) onFile(paths);
  };

  return (
    <div className="p-3">
      <div
        className={`flex flex-col items-center justify-center gap-2 border-2 border-dashed px-4 py-5 text-center ${
          hover ? 'border-qb-green bg-qb-green-tint' : 'border-qb-border bg-white'
        }`}
      >
        <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4 text-md" onClick={browse} disabled={busy}>
          Choose a file…
        </button>
        <div className="text-xs text-qb-ink-muted">
          A QuickBooks .qbw company file or .qbb backup, a QuickBooks .iif export, a Quicken .qif export, or QuickBooks reports exported to .csv or
          .xlsx: a Journal or General Ledger, with the Account Listing, contact lists and Item List if you have them (choose them together). You can
          also drag files here. They are only read, never changed, and nothing is added to your books yet.
        </div>
      </div>
    </div>
  );
}

/** What an import was read from, by its file's extension. */
function sourceLabel(fileName) {
  const ext = fileName.split('.').pop()?.toLowerCase();
  if (REPORT_EXTENSIONS.includes(ext)) return 'Report export';
  if (ext === 'iif' || ext === 'qif') return `${ext.toUpperCase()} export`;
  return 'Company file';
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
          // In effect: posted entries, or the lists a lists-only file added.
          const live = b.liveEntryCount > 0 || b.listsAdded;
          const status = b.listsAdded
            ? 'Lists added'
            : b.liveEntryCount > 0
              ? `Posted · ${b.liveEntryCount.toLocaleString()} entries`
              : b.postedAt
                ? 'Reversed'
                : b.listsOnly
                  ? 'Not added yet'
                  : 'Not posted yet';
          const tone = live ? 'text-qb-green-active' : b.postedAt ? 'text-qb-ink-muted' : 'text-qb-info';
          return (
            <tr key={key} className={isOpen ? 'bg-qb-green-tint' : ''}>
              <td className="qb-grid-cell">
                <div className="font-semibold">{b.fileName}</div>
                <div className="text-2xs text-qb-ink-faint">
                  {b.kind !== 'ledger'
                    ? 'Invoice lines'
                    : b.listsOnly
                      ? 'Lists only'
                      : `${sourceLabel(b.fileName)}${b.liveEntryCount === 0 ? '' : b.mode === 'FULL' ? ' · full import' : ' · journal import'}`}
                </div>
              </td>
              <td className="qb-grid-cell text-qb-ink-muted">{STAMP.format(new Date(b.importedAt))}</td>
              <td className="qb-grid-cell num">{b.lineCount.toLocaleString()}</td>
              <td className={`qb-grid-cell font-semibold ${tone}`}>{status}</td>
              <td className="qb-grid-cell">
                <div className="flex justify-end gap-1">
                  <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy || isOpen} onClick={() => onOpen(b)}>
                    {isOpen ? 'Open below' : 'Open'}
                  </button>
                  {live && b.kind === 'ledger' && (
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
                  {((b.liveEntryCount > 0 && b.mode !== 'FULL') || b.listsAdded) && (
                    <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs text-qb-negative" disabled={busy} onClick={() => onReverse(b)}>
                      {b.listsAdded ? 'Reverse added lists…' : 'Reverse posted entries…'}
                    </button>
                  )}
                  <button
                    type="button"
                    className="qb-btn qb-btn-secondary h-row px-2 text-xs"
                    disabled={busy || live}
                    title={
                      !live
                        ? 'Delete this import (your books are not affected)'
                        : b.listsAdded
                          ? 'Reverse the added lists first'
                          : b.mode === 'FULL'
                            ? 'Start over first'
                            : 'Reverse the posted entries first'
                    }
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

/** Source accounts the import brings in: those with lines, or every one from a file of lists only. */
const usedAccounts = (summary) => (summary.listsOnly ? summary.accounts : summary.accounts.filter((a) => a.lineCount > 0));

/** Source accounts of a full-ledger import, each mapped to a LocalBooks account. */
function LedgerAccountGrid({ summary, accounts, onMap, busy }) {
  const used = usedAccounts(summary);
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
          <th className="qb-grid-head">Account in the file</th>
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
const OUTCOMES = {
  RECOVERED: ['Recovered', 'Account and amount read; its transaction balances. Only the line memo is missing.'],
  NO_AMOUNT: ['No amount', 'A blank or text line: nothing is missing from the books.'],
  UNREADABLE: ['Not read', 'Could not be used. If its transaction is listed above, enter that transaction by hand.'],
};

/** Line rows that did not decode in full, with whatever was read and what became of each. */
function UnreadableList({ items }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[110px]" />
        <col className="w-[150px]" />
        <col className="w-[80px]" />
        <col />
        <col className="w-[120px]" />
        <col className="w-[120px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Date</th>
          <th className="qb-grid-head">Type</th>
          <th className="qb-grid-head">Number</th>
          <th className="qb-grid-head">Name · account</th>
          <th className="qb-grid-head text-right">Amount</th>
          <th className="qb-grid-head">Result</th>
        </tr>
      </thead>
      <tbody>
        {items.map((u, i) => {
          const [label, help] = OUTCOMES[u.outcome] ?? [u.outcome, ''];
          return (
            <tr key={i}>
              <td className="qb-grid-cell">{u.date ? formatDate(u.date) : '—'}</td>
              <td className="qb-grid-cell">{u.kind}</td>
              <td className="qb-grid-cell">{u.docNum ?? ''}</td>
              <td className="qb-grid-cell">{[u.name, u.accountName].filter(Boolean).join(' · ') || '—'}</td>
              <td className="qb-grid-cell num">{u.cents == null ? '—' : formatCents(Math.abs(u.cents))}</td>
              <td className={`qb-grid-cell font-semibold ${u.outcome === 'UNREADABLE' ? 'text-qb-negative' : 'text-qb-ink-muted'}`} title={help}>
                {label}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

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

/** Customers, vendors and items from the file, brought in as LocalBooks records. */
function ListsPanel({ summary, busy, accountsReady, run }) {
  const l = summary.lists;
  if (!l || l.customers + l.vendors + l.items === 0) return null;
  const all = l.customers + l.vendors + l.items;
  const done = l.customersDone + l.vendorsDone + l.itemsDone;
  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-qb-border px-3 py-2 text-xs">
      <span className="font-semibold">Customers, vendors and items</span>
      <span className="text-qb-ink-muted">
        {l.customers.toLocaleString()} customers · {l.vendors.toLocaleString()} vendors · {l.items.toLocaleString()} items in the file
        {done > 0 && ` · ${done.toLocaleString()} of ${all.toLocaleString()} in LocalBooks`}
      </span>
      {done < all && !summary.listsOnly && (
        <button
          type="button"
          className="qb-btn qb-btn-secondary ml-auto h-row px-2 text-xs"
          disabled={busy || !accountsReady}
          title={accountsReady ? 'Matches existing records by name and creates the rest, with addresses, emails and terms' : 'Map the accounts first: items post to them'}
          onClick={() =>
            run(
              () => api.importLedgerLists(summary.id),
              (r) =>
                `Customers: ${r.customersCreated} added, ${r.customersMatched} already there. Vendors: ${r.vendorsCreated} added, ${r.vendorsMatched} already there. ` +
                `Items: ${r.itemsCreated} added, ${r.itemsMatched} already there` +
                (r.itemsOnFallbackAccount ? ` (${r.itemsOnFallbackAccount} post to your first income account: LocalBooks items need an income account; their description names the original account)` : '') +
                '.',
            )
          }
        >
          Bring in customers, vendors and items
        </button>
      )}
      {done === all && <span className="ml-auto text-qb-green-active">✓ All in LocalBooks</span>}
    </div>
  );
}

/** Sales document types read from the file, as `SourceDocument` tags them. */
const DOCUMENT_WORDS = {
  invoice: ['invoice', 'invoices'],
  creditMemo: ['credit memo', 'credit memos'],
  salesReceipt: ['sales receipt', 'sales receipts'],
  payment: ['payment', 'payments'],
  deposit: ['deposit', 'deposits'],
  bill: ['bill', 'bills'],
  billPayment: ['bill payment', 'bill payments'],
  expense: ['check or card charge', 'checks and card charges'],
  transfer: ['transfer', 'transfers'],
  estimate: ['estimate', 'estimates'],
  purchaseOrder: ['purchase order', 'purchase orders'],
};

const countOf = (n, [one, many]) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

function FallbackList({ items }) {
  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[110px]" />
        <col className="w-[140px]" />
        <col className="w-[90px]" />
        <col className="w-[200px]" />
        <col />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head">Date</th>
          <th className="qb-grid-head">Type</th>
          <th className="qb-grid-head">Number</th>
          <th className="qb-grid-head">Name</th>
          <th className="qb-grid-head">Why it is not a LocalBooks document</th>
        </tr>
      </thead>
      <tbody>
        {items.map((t, i) => (
          <tr key={i}>
            <td className="qb-grid-cell">{formatDate(t.date)}</td>
            <td className="qb-grid-cell">{t.kind}</td>
            <td className="qb-grid-cell">{t.docNum ?? ''}</td>
            <td className="qb-grid-cell">{t.name ?? ''}</td>
            <td className="qb-grid-cell">{t.reason}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ModeChoice({ mode, setMode, documents, busy }) {
  const docs = documents.filter(([type]) => DOCUMENT_WORDS[type]).map(([type, n]) => countOf(n, DOCUMENT_WORDS[type]));
  const options = [
    [
      'FULL',
      'Full import',
      docs.length
        ? `${docs.join(', ')} come in as LocalBooks documents, so open invoices and bills, customer and vendor balances, and A/R and A/P aging carry over. Customers, vendors and items are brought in first. Anything that cannot be a document posts as a journal entry and is listed. Undo with Start over.`
        : 'No sales documents were found in this file, so a full import posts the same journal entries.',
    ],
    ['JOURNAL', 'Journal import', 'Every transaction becomes one journal entry. Account balances are the same, but invoices and payments are not recreated. Undo with Reverse or Start over.'],
  ];
  return (
    <fieldset className="grid gap-2 sm:grid-cols-2" disabled={busy}>
      <legend className="sr-only">How to bring in the transactions</legend>
      {options.map(([value, title, text]) => (
        <label
          key={value}
          className={`flex cursor-pointer gap-2 rounded border px-3 py-2 text-xs ${mode === value ? 'border-qb-green bg-qb-green-tint' : 'border-qb-border'}`}
        >
          <input type="radio" name="ledger-import-mode" value={value} checked={mode === value} onChange={() => setMode(value)} className="mt-0.5" />
          <span>
            <span className="block font-semibold">
              {title}
              {value === 'FULL' && docs.length > 0 && <span className="ml-1 font-normal text-qb-ink-muted">(recommended)</span>}
            </span>
            <span className="text-qb-ink-muted">{text}</span>
          </span>
        </label>
      ))}
    </fieldset>
  );
}

/** Steps 2 and 3 for a full-ledger import. */
function LedgerReview({ summary, accounts, busy, run }) {
  const documents = summary.documents ?? [];
  const [mode, setMode] = useState(documents.length > 0 ? 'FULL' : 'JOURNAL');
  const fallbacks = summary.fallbacks ?? [];
  const used = usedAccounts(summary);
  const unmapped = used.filter((a) => !a.accountId).length;
  const matched = summary.accountsMatchingStored;
  const withStored = summary.accountsWithStoredBalance;
  const step2 = unmapped === 0 ? 'done' : 'active';
  const step3 = summary.listsOnly
    ? summary.listsAdded
      ? 'done'
      : 'active'
    : summary.liveEntryCount > 0 && summary.postableCount === 0
      ? 'done'
      : unmapped === 0 && summary.postableCount > 0
        ? 'active'
        : 'locked';

  // A file of lists only: add its accounts, customers, vendors and items, and post nothing.
  const addLists = async () => {
    const l = summary.lists;
    if (
      !(await askConfirm(
        `Add this file’s lists to your books?\n\n${countOf(used.length, ['account', 'accounts'])}, ${countOf(l.customers, ['customer', 'customers'])}, ` +
          `${countOf(l.vendors, ['vendor', 'vendors'])} and ${countOf(l.items, ['item', 'items'])}: each matches an existing record of the same name or is created. ` +
          'No transactions are posted. You can undo it later with "Reverse added lists" or "Start over".',
      ))
    )
      return;
    run(
      () => api.postLedgerBatch({ batchId: summary.id, mode: 'JOURNAL' }),
      (r) => {
        const a = r.accounts;
        const li = r.lists;
        if (!a || !li) return 'These lists are already in your books.';
        return (
          `Accounts: ${a.created} added, ${a.matched} already there` +
          (a.reactivated ? `, ${a.reactivated} put back` : '') +
          `. Customers: ${li.customersCreated} added, ${li.customersMatched} already there. Vendors: ${li.vendorsCreated} added, ${li.vendorsMatched} already there. ` +
          `Items: ${li.itemsCreated} added, ${li.itemsMatched} already there` +
          (li.itemsOnFallbackAccount ? ` (${li.itemsOnFallbackAccount} post to your first income account: LocalBooks items need an income account; their description names the original account)` : '') +
          '.'
        );
      },
    );
  };

  const post = async () => {
    const question =
      mode === 'FULL'
        ? `Bring ${summary.postableCount.toLocaleString()} transactions into your books as a full import?\n\nCustomers, vendors and items are added first, then invoices, credit memos, sales receipts, payments, deposits, bills, bill payments, checks, card charges, transfers, estimates and purchase orders as LocalBooks documents, and everything else as journal entries. Compare the account balances above with the original program’s trial balance first. You can undo it later with "Start over".`
        : `Post ${summary.postableCount.toLocaleString()} journal entries to your books?\n\nEach is one transaction from the file, balanced as it was there. Compare the account balances above with the original program’s trial balance first. You can undo it later with "Reverse posted entries".`;
    if (!(await askConfirm(question, { confirmLabel: mode === 'FULL' ? 'Import' : 'Post' }))) return;
    run(
      () => api.postLedgerBatch({ batchId: summary.id, mode }),
      (r) => {
        const f = r.full;
        const made = f
          ? [
              [f.invoices, DOCUMENT_WORDS.invoice],
              [f.creditMemos, DOCUMENT_WORDS.creditMemo],
              [f.salesReceipts, DOCUMENT_WORDS.salesReceipt],
              [f.payments, DOCUMENT_WORDS.payment],
              [f.deposits, DOCUMENT_WORDS.deposit],
              [f.bills, DOCUMENT_WORDS.bill],
              [f.vendorCredits, ['vendor credit', 'vendor credits']],
              [f.billPayments, DOCUMENT_WORDS.billPayment],
              [f.checks, ['check', 'checks']],
              [f.expenses, ['card charge', 'card charges']],
              [f.cardCredits, ['card credit', 'card credits']],
              [f.transfers, DOCUMENT_WORDS.transfer],
              [f.estimates, DOCUMENT_WORDS.estimate],
              [f.purchaseOrders, DOCUMENT_WORDS.purchaseOrder],
              [f.journalEntries, ['journal entry', 'journal entries']],
            ]
              .filter(([n]) => n > 0)
              .map(([n, words]) => countOf(n, words))
          : [];
        return (
          (f ? `Brought in ${made.join(', ')}.` : `Posted ${r.entriesPosted.toLocaleString()} entries (${r.linesPosted.toLocaleString()} lines).`) +
          (f?.creditsApplied ? ` Applied ${countOf(f.creditsApplied, ['credit', 'credits'])} to invoices.` : '') +
          (f?.fellBack ? ` ${countOf(f.fellBack, ['document', 'documents'])} posted as journal entries instead (listed in step 3).` : '') +
          (f?.notImported ? ` ${countOf(f.notImported, ['estimate or purchase order', 'estimates and purchase orders'])} could not be brought in (listed in step 3; they post nothing).` : '') +
          (r.skippedUnbalanced ? ` ${r.skippedUnbalanced} incomplete transactions were left out; enter them by hand.` : '') +
          (r.skippedAlreadyPosted ? ` ${r.skippedAlreadyPosted} were already posted.` : '')
        );
      },
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
            [summary.listsOnly ? 'Accounts' : 'Accounts used', used.length.toLocaleString()],
            ['Balances matching the file', withStored ? `${matched} of ${withStored}` : '—'],
          ].map(([k, v]) => (
            <div key={k} className="min-w-0">
              <dt className="text-2xs uppercase tracking-wide text-qb-ink-faint">{k}</dt>
              <dd className="truncate font-semibold" title={v}>{v}</dd>
            </div>
          ))}
        </dl>
        <p className="border-b border-qb-border px-3 py-1 text-2xs text-qb-ink-faint">
          {summary.listsOnly
            ? 'Lists only: no transactions.'
            : `${summary.byKind.map(([k, n]) => `${n.toLocaleString()} ${k.toLowerCase()}${n === 1 ? '' : 's'}`).join(' · ')}.`}{' '}
          {summary.sourceFormat === 'QBW'
            ? `Read from ${summary.decodedPages.toLocaleString()} of ${summary.rowPages.toLocaleString()} data pages`
            : `Read from the ${FORMAT_NAMES[summary.sourceFormat]}`}
          {summary.unreadableLines > 0 && `; ${summary.unreadableLines.toLocaleString()} lines could not be read in full (listed below)`}
          {summary.sourceFormat === 'QIF' && summary.rejectedLines > 0 && `; ${summary.rejectedLines.toLocaleString()} investment transactions were skipped (LocalBooks does not track securities)`}.
        </p>
        {summary.unreadable.length > 0 && (
          <details className="border-b border-qb-border" open={summary.unreadable.some((u) => u.outcome === 'UNREADABLE')}>
            <summary className="cursor-pointer px-3 py-1.5 text-xs">
              <strong>
                {summary.unreadable.length} line{summary.unreadable.length === 1 ? '' : 's'} not read in full
              </strong>
              {': '}
              {[
                ['RECOVERED', 'recovered'],
                ['NO_AMOUNT', 'with no amount'],
                ['UNREADABLE', 'not read'],
              ]
                .map(([code, word]) => [summary.unreadable.filter((u) => u.outcome === code).length, word])
                .filter(([n]) => n > 0)
                .map(([n, word]) => `${n} ${word}`)
                .join(', ')}
              {summary.unbalanced.length === 0 && '. Every transaction still balances.'}
            </summary>
            <UnreadableList items={summary.unreadable} />
          </details>
        )}
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
                  (r) =>
                    `Matched ${r.matched} existing account${r.matched === 1 ? '' : 's'}` +
                    (r.reactivated ? `, put back ${r.reactivated} from an earlier import` : '') +
                    ` and created ${r.created}.`,
                )
              }
            >
              Match existing accounts and create the rest
            </button>
          )}
        </div>
        <LedgerAccountGrid summary={summary} accounts={accounts} busy={busy} onMap={(sourceId, id) => run(() => api.setLedgerAccount(summary.id, sourceId, id))} />
        <ListsPanel summary={summary} busy={busy} accountsReady={unmapped === 0} run={run} />
      </Step>

      <Step number={3} title={summary.listsOnly ? 'Add lists to your books' : 'Post to your books'} state={step3}>
        <div className="flex flex-col gap-2 px-3 py-2">
          {summary.listsOnly && summary.listsAdded && (
            <Notice
              kind="success"
              text={`This file’s lists are in your books: ${countOf(used.length, ['account', 'accounts'])} and ${countOf(
                summary.lists.customersDone + summary.lists.vendorsDone + summary.lists.itemsDone,
                ['customer, vendor or item', 'customers, vendors and items'],
              )}. No transactions were posted. Use “Reverse added lists” or “Start over” above to undo it.`}
            />
          )}
          {summary.listsOnly && !summary.listsAdded && (
            <div className="flex items-center gap-3">
              <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={busy} onClick={addLists}>
                Add lists to your books…
              </button>
              <span className="text-xs text-qb-ink-muted">
                This file has no transactions.{unmapped > 0 && ' Accounts you have not chosen in step 2 are matched by name or created.'}
              </span>
            </div>
          )}
          {summary.liveEntryCount > 0 && summary.mode === 'FULL' && (
            <Notice
              kind="success"
              text={`This import is in your books: ${(summary.postedAs ?? [])
                .map(([as, n]) => `${n.toLocaleString()} ${as.toLowerCase().replaceAll('_', ' ')}${n === 1 ? '' : 's'}`)
                .join(', ')}. Use “Start over” above to undo it.`}
            />
          )}
          {summary.liveEntryCount > 0 && summary.mode !== 'FULL' && (
            <Notice kind="success" text={`${summary.liveEntryCount.toLocaleString()} entries from this import are in your books. Use “Reverse posted entries” above to undo them.`} />
          )}
          {fallbacks.length > 0 && (
            <details className="rounded border border-qb-border">
              <summary className="cursor-pointer px-3 py-1.5 text-xs">
                <strong>{countOf(fallbacks.length, ['document', 'documents'])} not brought in as LocalBooks documents.</strong> Those that post are in your
                books as journal entries; estimates and purchase orders post nothing. The reasons are below.
              </summary>
              <FallbackList items={fallbacks} />
            </details>
          )}
          {summary.postableCount > 0 && summary.liveEntryCount === 0 && <ModeChoice mode={mode} setMode={setMode} documents={documents} busy={busy} />}
          {summary.postableCount > 0 && (
            <div className="flex items-center gap-3">
              <button type="button" className="qb-btn qb-btn-primary h-row-lg px-4" disabled={busy || unmapped > 0} onClick={post}>
                {mode === 'FULL' && summary.liveEntryCount === 0
                  ? `Bring in ${summary.postableCount.toLocaleString()} transactions…`
                  : `Post ${summary.postableCount.toLocaleString()} entries…`}
              </button>
              {unmapped > 0 && <span className="text-xs text-qb-ink-muted">Finish step 2 first.</span>}
            </div>
          )}
        </div>
      </Step>
    </>
  );
}

/**
 * What outlives the screen. The wizard unmounts when you go elsewhere, but a file being read keeps
 * going in the backend; coming back follows that read (its progress, then its result) instead of
 * offering to start again, and reopens the import that was open.
 */
let reading = null;
let lastOpened = null;

/** Starts reading a file, collecting its progress whether or not the screen is showing. */
function startReading(paths, ext) {
  const first = ext === 'qbb' ? 'Unpacking backup' : 'Reading file';
  const subscribers = new Set();
  const r = {
    progress: { stage: first, done: 0, total: 1 },
    seen: ext === 'qbb' ? [first] : [],
    subscribe: (fn) => {
      subscribers.add(fn);
      return () => subscribers.delete(fn);
    },
  };
  let unlisten;
  // Events travel separately from the command's reply; listening stops once it returns.
  const listening = listen(QBW_PROGRESS, (e) => {
    r.progress = e.payload;
    if (!r.seen.includes(e.payload.stage)) r.seen = [...r.seen, e.payload.stage];
    subscribers.forEach((fn) => fn(r));
  })
    .then((fn) => (unlisten = fn))
    .catch(() => {});
  const ingest = REPORT_EXTENSIONS.includes(ext) ? () => api.ingestReportExports(paths) : () => api.ingestRawQbw(paths[0]);
  r.promise = listening.then(ingest).finally(() => unlisten?.());
  return r;
}

/** Reads a desktop company file or backup into a holding area, maps what it found, then posts balanced entries. */
export default function QbwImporter({ accounts }) {
  const [batches, setBatches] = useState([]);
  const [opened, setOpened] = useState(() => lastOpened);
  const [summary, setSummary] = useState(null);
  const [progress, setProgress] = useState(null);
  const [seenStages, setSeenStages] = useState([]);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const [offsetId, setOffsetId] = useState(null);
  const [fallbackDate, setFallbackDate] = useState('');
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
    lastOpened = opened;
    loadSummary(opened);
  }, [opened, loadSummary]);

  // Shows a read's progress, then what it found. Also used when coming back to a read that
  // started before the screen was left.
  const follow = useCallback(
    async (r) => {
      setBusy(true);
      setNotice(null);
      const show = ({ progress: p, seen }) => {
        setProgress(p);
        setSeenStages(seen);
      };
      show(r);
      const unsubscribe = r.subscribe(show);
      try {
        const result = await r.promise;
        const batch = result.batch;
        lastOpened = { kind: result.kind, id: batch.id };
        setOpened(lastOpened);
        setSummary(batch);
        await refreshBatches();
        if (result.kind === 'ledger') {
          const complete = batch.transactionCount - batch.unbalanced.length;
          const l = batch.lists;
          const sources = result.sources?.length ? ` (${result.sources.join('; ')})` : '';
          setNotice({
            kind: 'success',
            text: batch.listsOnly
              ? `Read the ${FORMAT_NAMES[batch.sourceFormat]}${sources}: lists only, with ${batch.accounts.length} accounts, ${l.customers} customers, ${l.vendors} vendors and ${l.items} items, and no transactions. Nothing is in your books yet: review step 2 below, then add the lists in step 3.`
              :
              `Read the ${batch.sourceFormat === 'QBW' ? 'full ledger' : FORMAT_NAMES[batch.sourceFormat]}${sources}: ${complete.toLocaleString()} transactions and ${batch.accounts.filter((a) => a.lineCount > 0).length} accounts.` +
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
        unsubscribe();
        if (reading === r) reading = null;
        setBusy(false);
        setProgress(null);
      }
    },
    [refreshBatches],
  );

  useEffect(() => {
    if (reading) follow(reading);
  }, [follow]);

  const ingest = useCallback(
    async (paths) => {
      const exts = paths.map((p) => p.split('.').pop()?.toLowerCase());
      const ext = exts[0];
      if (!exts.every((e) => EXTENSIONS.includes(e))) {
        setNotice({ kind: 'error', text: 'Choose a .qbw company file, a .qbb backup, an .iif or .qif export, or QuickBooks reports exported to .csv or .xlsx.' });
        return;
      }
      if (paths.length > 1 && !exts.every((e) => REPORT_EXTENSIONS.includes(e))) {
        setNotice({
          kind: 'error',
          text: 'Choose one company file or export at a time. Only QuickBooks report exports (.csv, .xlsx) can be read together, such as a Journal with the Account Listing.',
        });
        return;
      }
      reading = startReading(paths, ext);
      follow(reading);
    },
    [follow],
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

  const reverse = async (b) => {
    if (
      !(await askConfirm(
        b.listsAdded
          ? `Reverse the lists added from ${b.fileName}?\n\nThe accounts, customers, vendors and items the import created are deactivated (accounts used since stay active). Records that were already in your books are left alone.`
          : `Reverse the import of ${b.fileName}?\n\nEach of its ${b.liveEntryCount.toLocaleString()} entries gets a reversing entry, so your balances return to what they were before the import. Accounts the importer created are deactivated. This can take a moment for large imports.`,
      ))
    )
      return;
    run(
      () => (b.kind === 'ledger' ? api.reverseLedgerBatch(b.id) : api.reverseQbwBatch(b.id)),
      (r) =>
        b.listsAdded
          ? `Deactivated ${r.accountsDeactivated} accounts and ${r.listRecordsDeactivated} customers, vendors and items the import created.`
          : `Reversed ${r.entriesVoided.toLocaleString()} entries` +
            (r.accountsDeactivated ? `, deactivated ${r.accountsDeactivated} accounts the importer created` : '') +
            (r.listRecordsDeactivated ? ` and ${r.listRecordsDeactivated} customers, vendors and items it added` : '') +
            '. Your balances are back to where they were.',
    );
  };

  // Restores the copy of the company file saved just before the import posted. Two confirmations,
  // the second typed, because everything recorded since that moment is undone too.
  const startOver = async (b) => {
    const when = STAMP.format(new Date(b.startOver.takenAt));
    if (
      !(await askConfirm(
        `Start over: put your books back to how they were just before ${b.fileName} was posted (${when})?\n\n` +
          (b.listsAdded
            ? 'This removes the customers, vendors and items it added, and the accounts it created then. Accounts set up in step 2 before that stay, so the lists can be added again.\n\n'
            : `This removes the ${b.liveEntryCount.toLocaleString()} imported entries, with no reversing entries left behind. The accounts set up for the import stay, so it can be posted again.\n\n`) +
          `It also undoes ANYTHING else recorded since ${when}: invoices, payments, edits, other imports.\n\n` +
          `Your books as they are now are saved as a backup first (Settings › Backups), so this can be undone.`,
      ))
    )
      return;
    const typed = await askText('This replaces your company file. Type START OVER to confirm.');
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

  // Closes the open import. Its summary goes in the same update, so no render pairs a closed
  // import with the summary of the one that was open.
  const close = () => {
    setOpened(null);
    setSummary(null);
  };

  const remove = async (b) => {
    if (!(await askConfirm(`Delete the import of ${b.fileName}? Its staged lines are removed. Your books are not affected.`, { confirmLabel: 'Delete', danger: true }))) return;
    setBusy(true);
    setNotice(null);
    try {
      await (b.kind === 'ledger' ? api.discardLedgerBatch(b.id) : api.discardQbwBatch(b.id));
      if (opened && b.kind === opened.kind && b.id === opened.id) close();
      setNotice({ kind: 'success', text: 'Import deleted.' });
      await refreshBatches();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const isLedger = opened?.kind === 'ledger';
  // Only an invoice-lines summary has labels; anything else is not shown as one.
  const invoiceSummary = !isLedger && summary?.labels ? summary : null;
  const unmapped = invoiceSummary?.labels.filter((l) => !l.accountId).length ?? 0;
  const postable = invoiceSummary ? invoiceSummary.invoiceCount - invoiceSummary.alreadyPostedInvoiceCount : 0;
  const ready = invoiceSummary && invoiceSummary.lineCount > 0 && unmapped === 0 && offsetId != null && postable > 0;
  const step1 = summary ? 'done' : 'active';
  const step2 = !invoiceSummary ? 'locked' : unmapped === 0 && invoiceSummary.lineCount > 0 ? 'done' : 'active';
  const step3 = !invoiceSummary ? 'locked' : invoiceSummary.liveEntryCount > 0 && postable === 0 ? 'done' : ready ? 'active' : 'locked';

  const post = async () => {
    const total = formatCents(invoiceSummary.totalCents);
    if (
      !(await askConfirm(
        `Post ${postable.toLocaleString()} journal entries (${total} in total) to your books?\n\nCheck this total against the original program’s reports first. You can undo it later with "Reverse posted entries".`,
      ))
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
          Reads desktop company files (.qbw) and backups (.qbb), QuickBooks .iif exports, Quicken .qif exports and QuickBooks report exports (a Journal or
          General Ledger as .csv or .xlsx, from QuickBooks Desktop for Windows or Mac, or QuickBooks Online). Newer company files and the exports
          come in as the <strong>full ledger</strong>: the chart of accounts and every transaction, each posted as a balanced entry or, in a full import,
          as LocalBooks invoices, bills, checks, deposits and transfers. Older company files the newer reader does not cover bring in{' '}
          <strong>invoice line amounts only</strong>, posted against an offset account you choose. Compare the totals with your original program’s reports before posting; anything posted can be reversed
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
            onOpen={(b) => {
              setSummary(null);
              setOpened({ kind: b.kind, id: b.id });
            }}
            onReverse={reverse}
            onStartOver={startOver}
            onDelete={remove}
          />
        </section>
      )}

      <Step
        number={1}
        title="Choose a file"
        state={progress ? 'active' : step1}
        aside={
          summary &&
          !progress && (
            <button type="button" className="qb-btn qb-btn-secondary h-row px-2 text-xs" disabled={busy} onClick={close}>
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

      {isLedger && summary?.accounts ? (
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
