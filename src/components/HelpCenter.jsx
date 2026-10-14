import { useEffect, useMemo, useRef, useState } from 'react';
import { Icon } from '../lib/icons.js';

/**
 * Help articles: two sentences each, describing what LocalBooks actually does. `go` opens the
 * screen the article is about (`view` is a navigation id, `action` an optional intent such as
 * opening a form or a Settings tab).
 */
export const HELP_ARTICLES = [
  {
    id: 'qbw-import',
    icon: 'Import',
    title: 'How to import QuickBooks Desktop (.qbw) files',
    body: 'Under Accounting › QuickBooks Import, choose a .qbw file: LocalBooks reads it without changing it and brings in invoice line amounts only, because customers, payments and account names cannot be read from QuickBooks files yet. Map each line type to an income account, check the total against QuickBooks, then post; “Reverse posted entries” undoes an import.',
    keywords: 'qbw qbb migrate migration intuit desktop convert move',
    go: { label: 'Go to QuickBooks Import', view: 'accounting/qbw-import' },
  },
  {
    id: 'ledger-discrepancies',
    icon: 'Integrity',
    title: 'Fixing ledger discrepancies (verify and correct)',
    body: '“Verify ledger integrity” in the Audit Log compares every posted entry with the snapshot taken when it was posted and lists anything changed, deleted or out of balance. “Review corrections” proposes a balanced correcting entry where one can fix the problem and posts it only after you approve; a change that left debits and credits unequal needs a restore from a backup.',
    keywords: 'audit tamper heal auto-heal mismatch integrity forensic snapshot corrupt',
    go: { label: 'Go to the Audit Log', view: 'accounting/audit' },
  },
  {
    id: 'closing-date',
    icon: 'ClosingDate',
    title: 'Closing a fiscal or tax period',
    body: 'Set a closing date in Settings › Preferences Console to lock the books through that date, so nothing dated on or before it can be posted or voided from any screen. Add a passcode to require it for moving or removing the date; every change is kept in the closing history.',
    keywords: 'lock close year end period tax password passcode books',
    go: { label: 'Go to the closing date', view: 'settings', action: 'closing' },
  },
  {
    id: 'mcp',
    icon: 'AiAccess',
    title: 'Connecting Claude, Cursor or another AI assistant (local MCP)',
    body: 'Turn on Settings › AI Access (MCP) to let an assistant on this computer read your trial balance, profit and loss summary and overdue receivables; it cannot change anything, and the server answers only on this computer. Copy the ready-made Claude Code command, or the JSON configuration for clients such as Claude Desktop or Cursor, which includes the access token.',
    keywords: 'ai claude cursor mcp model context protocol assistant chatgpt token llm',
    go: { label: 'Go to AI Access', view: 'settings', action: 'ai' },
  },
  {
    id: 'invoice',
    icon: 'Sales',
    title: 'Creating an invoice',
    body: 'Open Sales › Invoices and choose New invoice, then add lines with a quantity, rate and income account; decimal quantities such as 1.5 hours are fine. Posting debits Accounts Receivable and credits each line’s income account, and the invoice can then be printed or saved as a PDF.',
    keywords: 'bill customer sale charge billing new',
    go: { label: 'Create an invoice', view: 'sales/invoices', action: 'invoice' },
  },
  {
    id: 'receive-payment',
    icon: 'Sales',
    title: 'Recording a customer payment',
    body: 'Use Receive Payment on the Invoices screen to apply money to one or several open invoices, in full or in part. Deposit it straight to a bank account, or to Undeposited Funds and later combine payments into one Bank Deposit.',
    keywords: 'paid payment customer receive partial deposit undeposited',
    go: { label: 'Receive a payment', view: 'sales/invoices', action: 'receive-payment' },
  },
  {
    id: 'pay-bills',
    icon: 'Expenses',
    title: 'Entering and paying bills',
    body: 'Enter a vendor bill under Expenses › Bills, splitting it across expense accounts if needed; it stays in Accounts Payable until paid. Pay Bills lets you pay several bills at once, fully or partly, from a bank or credit card account.',
    keywords: 'vendor payable ap supplier pay bills split',
    go: { label: 'Pay bills', view: 'expenses/bills', action: 'pay-bills' },
  },
  {
    id: 'reconcile',
    icon: 'Banking',
    title: 'Reconciling a bank account',
    body: 'In Banking › Reconcile, enter the statement end date and ending balance, then tick the transactions that cleared until the difference is $0.00. Finishing locks those lines, so the reconciled balance can never drift.',
    keywords: 'bank statement match cleared reconciliation',
    go: { label: 'Go to Reconcile', view: 'banking/reconcile' },
  },
  {
    id: 'void',
    icon: 'Accounting',
    title: 'Undoing a transaction (void)',
    body: 'Posted transactions are never deleted: voiding posts an exact reversing entry and both stay visible in Accounting › Journal Entries. Reconciled transactions and anything dated in a closed period cannot be voided.',
    keywords: 'delete undo reverse mistake cancel void correction',
    go: { label: 'Go to Journal Entries', view: 'accounting/journal' },
  },
  {
    id: 'backup',
    icon: 'Settings',
    title: 'Backing up, restoring or opening a company file',
    body: 'Settings › File & Data Controls backs up your books to a .localbooks file, restores a backup (keeping a safety copy of the current books) or opens another company file; a daily backup is also taken automatically. Restoring replaces the whole company file with the backup, so anything entered after it was taken is not kept.',
    keywords: 'backup restore export file save localbooks open copy',
    go: { label: 'Go to File & Data Controls', view: 'settings', action: 'data' },
  },
];

/** True when every word of the query appears in the article (title, text or keywords). */
export function matchesQuery(article, query) {
  const haystack = `${article.title} ${article.body} ${article.keywords}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/** Right-side help panel: searchable articles with shortcuts into the app. */
export default function HelpCenter({ onClose, onNavigate }) {
  const [query, setQuery] = useState('');
  const search = useRef(null);
  const results = useMemo(() => HELP_ARTICLES.filter((a) => matchesQuery(a, query)), [query]);

  useEffect(() => {
    search.current?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <aside
      className="no-print flex w-80 flex-none animate-[lb-slide-in_160ms_ease-out] flex-col border-l border-qb-border bg-white text-xs"
      aria-label="Help"
    >
      <div className="flex h-toolbar flex-none items-center gap-2 border-b border-qb-border px-3">
        <Icon name="HelpCenter" className="text-qb-green" />
        <h2 className="text-md font-semibold">Help</h2>
        <button type="button" onClick={onClose} className="ml-auto rounded-xs p-1 text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink" aria-label="Close help">
          <Icon name="Close" />
        </button>
      </div>
      <div className="flex-none border-b border-qb-border p-2">
        <label className="flex items-center gap-1.5 rounded-xs border border-qb-border-strong px-2 focus-within:border-qb-green focus-within:shadow-focus">
          <Icon name="Search" className="text-qb-ink-muted" />
          <input
            ref={search}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search help (e.g. close the year)"
            className="h-row-lg min-w-0 flex-1 bg-transparent text-sm outline-none"
            aria-label="Search help"
          />
        </label>
        <div className="mt-1 text-2xs text-qb-ink-faint" aria-live="polite">
          {query ? `${results.length} article${results.length === 1 ? '' : 's'}` : `${HELP_ARTICLES.length} articles · F1 opens help anywhere`}
        </div>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto scrollbar-thin">
        {results.length === 0 && (
          <li className="px-3 py-4 text-qb-ink-muted">No articles match “{query}”. Try fewer or different words.</li>
        )}
        {results.map((a) => (
          <li key={a.id} className="border-b border-qb-border px-3 py-2">
            <div className="flex items-start gap-2">
              <Icon name={a.icon} className="mt-0.5 text-qb-ink-muted" />
              <div className="min-w-0">
                <h3 className="text-sm font-semibold leading-tight">{a.title}</h3>
                <p className="mt-1 leading-snug text-qb-ink-muted">{a.body}</p>
                <button
                  type="button"
                  onClick={() => onNavigate(a.go.view, a.go.action ?? null)}
                  className="mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline"
                >
                  {a.go.label}
                  <Icon name="Go" />
                </button>
              </div>
            </div>
          </li>
        ))}
      </ul>
    </aside>
  );
}
