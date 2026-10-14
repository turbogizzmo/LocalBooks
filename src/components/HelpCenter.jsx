import { useEffect, useMemo, useRef, useState } from 'react';
import { findNav } from './Sidebar.jsx';
import GuideView, { GUIDES, GUIDE_ORDER } from './GuideView.jsx';
import { Icon } from '../lib/icons.js';

/** Help areas, in the order they are listed under "All topics". */
export const HELP_AREAS = [
  { id: 'sales', label: 'Sales & customers', icon: 'Sales' },
  { id: 'expenses', label: 'Expenses & vendors', icon: 'Expenses' },
  { id: 'employees', label: 'Employees & time', icon: 'Employees' },
  { id: 'banking', label: 'Banking', icon: 'Banking' },
  { id: 'accounting', label: 'Accounting & audit', icon: 'Accounting' },
  { id: 'reports', label: 'Reports', icon: 'Reports' },
  { id: 'company', label: 'Company & data', icon: 'Settings' },
];

/** What each screen is for, shown at the top of Help while that screen is open. */
export const SCREEN_HELP = {
  dashboard: 'Your books at a glance: assets and liabilities over the last 12 months, open receivables by age, and a live trial balance that always shows whether debits equal credits.',
  'sales/all': 'Every sale in one register: invoices, payments and sales receipts. Record a sales receipt here when a customer pays at the time of sale.',
  'sales/invoices': 'Create invoices, see what each customer still owes, and record full or partial payments against one or several invoices.',
  'sales/estimates': 'Quotes for customers that do not touch your books until they become invoices. Mark an estimate accepted, then convert it to an invoice in one step.',
  'sales/customers': 'Everyone you sell to, with their open receivable balance. Add a customer here or while creating an invoice.',
  'sales/products': 'Products and services you sell. Choosing one on an invoice or sales receipt fills in its description, rate and income account. Track stock to keep quantity on hand and cost at moving average.',
  'expenses/expenses': 'Money paid straight away: expenses (card, cash or bank) and checks, each split across one or more expense accounts.',
  'expenses/bills': 'Bills you owe vendors, tracked in Accounts Payable until paid. Pay several at once with Pay bills.',
  'expenses/vendors': 'Everyone you buy from, with what you still owe each of them.',
  'employees/time': 'Hours worked by employees, entered one activity at a time or as a weekly timesheet. Time does not post to your books; billable hours can be invoiced.',
  'banking/feeds': 'Import your bank’s CSV, OFX or QFX statement, then approve each transaction into a category. LocalBooks skips anything already imported and learns from what you approve.',
  'banking/transfers': 'Bank deposits (including customer payments held in Undeposited Funds) and transfers between your own accounts.',
  'banking/reconcile': 'Match your books to a bank or card statement and lock the matched transactions so the reconciled balance can never change.',
  'accounting/journal': 'Every posted entry in date order, including voids and their reversals. Post a manual journal entry here for adjustments.',
  'accounting/audit': 'A history of every posting, void and reconciliation that cannot be edited, and a check that every posted entry still matches what was recorded.',
  'accounting/fx-revaluation': 'Restate open foreign-currency invoices at a period-end rate. The unrealized gain or loss is posted on that date and reversed the next day.',
  'accounting/qbw-import': 'Bring invoice amounts across from a QuickBooks Desktop company file, review them, and post them as balanced entries.',
  reports: 'Profit and Loss for any date range and the Balance Sheet as of any date, following your fiscal year. Export to CSV, print, or save as PDF.',
  settings: 'Company details and preferences, the closing date, backups and company files, and local AI access.',
};

/**
 * Help topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section).
 */
export const HELP_ARTICLES = [
  // Sales & customers
  {
    id: 'invoice', area: 'sales', views: ['sales/invoices', 'sales/all', 'sales/customers'],
    title: 'Create an invoice',
    body: 'Fill in the invoice form at the top of Sales › Invoices: type the customer, then add lines with a quantity, rate and income account (decimals such as 1.5 hours are fine). Posting records the amount in Accounts Receivable and income, and the invoice can be printed or saved as a PDF.',
    keywords: 'bill customer charge billing new print pdf',
    go: { label: 'New invoice', view: 'sales/invoices', action: 'invoice' },
  },
  {
    id: 'receive-payment', area: 'sales', views: ['sales/invoices', 'sales/all', 'banking/transfers'],
    title: 'Record a customer payment',
    body: 'Receive Payment applies money to one or several open invoices, in full or in part, and the invoice status updates to Partial or Paid. Deposit it straight to a bank account, or to Undeposited Funds to combine several payments into one bank deposit later.',
    keywords: 'paid payment customer receive partial undeposited',
    go: { label: 'Receive a payment', view: 'sales/invoices', action: 'receive-payment' },
  },
  {
    id: 'foreign-currency', area: 'sales', views: ['sales/invoices', 'expenses/expenses'],
    title: 'Invoice or pay in another currency',
    body: 'Choose a currency and exchange rate on an invoice, expense or check; your books record the home-currency amount and keep the original alongside. A payment received at a different rate posts the difference to Exchange Gain or Loss automatically.',
    keywords: 'currency fx euro foreign exchange rate gain loss',
    go: { label: 'New invoice', view: 'sales/invoices', action: 'invoice' },
  },
  {
    id: 'sales-receipt', area: 'sales', views: ['sales/all'],
    title: 'Record a sale paid on the spot (sales receipt)',
    body: 'Use New sales receipt in All Sales when the customer pays at the time of sale, so no invoice or receivable is needed. Choose whether the money goes to a bank account or to Undeposited Funds.',
    keywords: 'cash sale receipt point of sale paid',
    go: { label: 'New sales receipt', view: 'sales/all', action: 'sales-receipt' },
  },
  {
    id: 'estimate', area: 'sales', views: ['sales/estimates'],
    title: 'Quote a customer with an estimate',
    body: 'Create an estimate the same way as an invoice; it does not affect your books. When the customer agrees, Edit as invoice loads its lines into the invoice form to review before posting, and the estimate is marked converted when the invoice posts.',
    keywords: 'quote proposal bid estimate convert accept',
    go: { label: 'New estimate', view: 'sales/estimates', action: 'estimate' },
  },
  {
    id: 'customers', area: 'sales', views: ['sales/customers'],
    title: 'Add and review customers',
    body: 'Add a customer with a name and optional email, and see each customer’s open receivable balance. Customers are also available when creating invoices, estimates and billable time.',
    keywords: 'customer client contact receivable balance',
    go: { label: 'Go to Customers', view: 'sales/customers' },
  },
  {
    id: 'products', area: 'sales', views: ['sales/products', 'sales/invoices'],
    title: 'Set up products and services',
    body: 'Add each product or service with its rate and the income account it belongs to. Picking it on an invoice or sales receipt then fills in the description, rate and account for you.',
    keywords: 'item service product price rate catalog',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
  },

  {
    id: 'inventory', area: 'sales', views: ['sales/products', 'expenses/bills', 'sales/invoices'],
    title: 'Track inventory and cost of goods sold',
    body: 'Choose Track stock on a product, record opening stock with Adjust stock, and buy more on bills by picking it as the Stock item. Selling it on an invoice or sales receipt records its moving average cost in Cost of Goods Sold, and LocalBooks will not sell more than is on hand.',
    keywords: 'inventory stock quantity on hand cogs cost of goods sold average cost sku count adjust',
    go: { label: 'Go to Products & Services', view: 'sales/products' },
  },

  // Expenses & vendors
  {
    id: 'expense-check', area: 'expenses', views: ['expenses/expenses'],
    title: 'Record an expense or write a check',
    body: 'Record money already spent as an expense (from a bank or credit card) or a check (numbered automatically), split across as many expense accounts as needed. It posts immediately and reduces the account it was paid from.',
    keywords: 'spend purchase receipt check cheque card payment split',
    go: { label: 'New expense', view: 'expenses/expenses', action: 'expense' },
  },
  {
    id: 'bill', area: 'expenses', views: ['expenses/bills', 'expenses/vendors'],
    title: 'Enter a vendor bill',
    body: 'Choose New bill, pick the vendor and split the total across expense accounts, by amount or by quantity × rate; it must add up to the bill total. The bill sits in Accounts Payable until you pay it.',
    keywords: 'vendor payable ap supplier invoice split owe',
    go: { label: 'New bill', view: 'expenses/bills', action: 'bill' },
  },
  {
    id: 'pay-bills', area: 'expenses', views: ['expenses/bills', 'expenses/vendors'],
    title: 'Pay bills',
    body: 'Pay bills lists every open bill so you can pay several at once, fully or partly, from a bank or credit card account. Each payment reduces Accounts Payable and the account it was paid from.',
    keywords: 'pay vendor payable ap batch partial',
    go: { label: 'Pay bills', view: 'expenses/bills', action: 'pay-bills' },
  },
  {
    id: 'vendors', area: 'expenses', views: ['expenses/vendors'],
    title: 'Add and review vendors',
    body: 'Add a vendor with a name and optional email, and see what you still owe each one. Vendors are also available when entering bills, expenses and checks.',
    keywords: 'vendor supplier payee contact payable balance',
    go: { label: 'Go to Vendors', view: 'expenses/vendors' },
  },

  // Employees & time
  {
    id: 'time', area: 'employees', views: ['employees/time'],
    title: 'Track time',
    body: 'Enter a single time activity, or a weekly timesheet with hours for each day; mark hours billable with a customer and hourly rate to invoice them later. Time does not post to your books by itself.',
    keywords: 'hours timesheet employee billable weekly activity',
    go: { label: 'Weekly timesheet', view: 'employees/time', action: 'weekly-timesheet' },
  },

  // Banking
  {
    id: 'bank-feed', area: 'banking', views: ['banking/feeds'],
    title: 'Import and categorize bank transactions',
    body: 'Import a CSV, OFX or QFX statement from your bank (overlapping statements are fine: anything already imported is skipped), then approve each transaction into a category or exclude it. Approved lines become real expenses or deposits, and LocalBooks learns from your choices to suggest a category next time.',
    keywords: 'bank feed csv ofx qfx qbo quicken import statement categorize approve exclude rule download duplicate columns mapping',
    go: { label: 'Go to Bank Feeds', view: 'banking/feeds' },
  },
  {
    id: 'deposit', area: 'banking', views: ['banking/transfers', 'sales/invoices'],
    title: 'Make a bank deposit',
    body: 'Choose the customer payments waiting in Undeposited Funds that went into the bank together, and add any other money in the same deposit. The deposit total then matches the single line on your bank statement.',
    keywords: 'deposit undeposited funds bank payments combine',
    go: { label: 'New bank deposit', view: 'banking/transfers', action: 'bank-deposit' },
  },
  {
    id: 'transfer', area: 'banking', views: ['banking/transfers'],
    title: 'Move money between your accounts',
    body: 'A transfer moves money between two of your own accounts, such as checking to savings or paying down a credit card. It is not income or an expense, so it does not appear on Profit and Loss.',
    keywords: 'transfer move savings checking credit card',
    go: { label: 'New transfer', view: 'banking/transfers', action: 'transfer' },
  },
  {
    id: 'reconcile', area: 'banking', views: ['banking/reconcile', 'banking/feeds'],
    title: 'Reconcile a bank or card account',
    body: 'Enter the statement end date and ending balance, then tick the transactions that cleared until the difference is $0.00. Finishing locks those lines, so the reconciled balance can never drift.',
    keywords: 'reconcile statement match cleared balance',
    go: { label: 'Go to Reconcile', view: 'banking/reconcile' },
  },

  // Accounting & audit
  {
    id: 'journal-entry', area: 'accounting', views: ['accounting/journal'],
    title: 'Post a manual journal entry',
    body: 'Choose New journal entry and enter at least two lines whose debits equal credits; LocalBooks will not post an entry that does not balance. Use it for adjustments such as depreciation or opening balances.',
    keywords: 'journal entry adjust adjustment debit credit manual opening balance',
    go: { label: 'New journal entry', view: 'accounting/journal', action: 'journal-entry' },
  },
  {
    id: 'void', area: 'accounting', views: ['accounting/journal', 'sales/invoices', 'sales/all', 'expenses/expenses', 'expenses/bills', 'banking/transfers'],
    title: 'Undo a transaction (void)',
    body: 'Posted transactions are never deleted: Void Transaction (in the row’s menu) posts an exact reversing entry, and both stay visible in Journal Entries. Reconciled transactions and anything dated on or before the closing date cannot be voided.',
    keywords: 'delete undo reverse mistake cancel void correction',
    go: { label: 'Go to Journal Entries', view: 'accounting/journal' },
  },
  {
    id: 'ledger-discrepancies', area: 'accounting', views: ['accounting/audit'],
    title: 'Check and correct the ledger',
    body: 'Verify ledger integrity compares every posted entry with the snapshot taken when it was posted and lists anything changed, deleted or out of balance. Review corrections proposes a balanced correcting entry where one can fix the problem and posts only after you approve; a change that left debits and credits unequal needs a restore from a backup.',
    keywords: 'audit tamper heal auto-heal mismatch integrity forensic snapshot corrupt verify',
    go: { label: 'Go to the Audit Log', view: 'accounting/audit' },
  },
  {
    id: 'audit-log', area: 'accounting', views: ['accounting/audit'],
    title: 'Read the audit log',
    body: 'Every posting, void and reconciliation lock is recorded automatically by the database, with its time, and cannot be edited or deleted. Filter by event type to see just posts, voids or reconciliations.',
    keywords: 'audit history trail log events',
    go: { label: 'Go to the Audit Log', view: 'accounting/audit' },
  },
  {
    id: 'fx-revaluation', area: 'accounting', views: ['accounting/fx-revaluation', 'sales/invoices', 'reports'],
    title: 'Revalue foreign-currency invoices at period end',
    body: 'Enter period-end rates under Accounting › Currency Revaluation to restate what customers owe in other currencies. The unrealized gain or loss posts on that date and reverses the next day, so payments still realize the right gain or loss.',
    keywords: 'currency revaluation unrealized exchange gain loss foreign period end rate',
    go: { label: 'Go to Currency Revaluation', view: 'accounting/fx-revaluation' },
  },
  {
    id: 'qbw-import', area: 'accounting', views: ['accounting/qbw-import'],
    title: 'Import a QuickBooks Desktop (.qbw) file',
    body: 'Choose a .qbw file: LocalBooks reads it without changing it and brings in invoice line amounts only, because customers, payments and account names cannot be read from QuickBooks files yet. Map each line type to an income account, compare the total with QuickBooks, then post.',
    keywords: 'qbw qbb migrate migration intuit desktop convert move import',
    go: { label: 'Go to QuickBooks Import', view: 'accounting/qbw-import' },
  },
  {
    id: 'qbw-undo', area: 'accounting', views: ['accounting/qbw-import'],
    title: 'Undo or delete a QuickBooks import',
    body: 'Reverse posted entries voids everything an import posted and deactivates the accounts it created, returning your balances to where they were. Delete import removes only the staged lines and never touches your books.',
    keywords: 'undo reverse delete import qbw remove staged',
    go: { label: 'Go to QuickBooks Import', view: 'accounting/qbw-import' },
  },

  // Reports
  {
    id: 'profit-loss', area: 'reports', views: ['reports', 'dashboard'],
    title: 'Run a Profit and Loss',
    body: 'Pick a period such as This month, This fiscal quarter or Last fiscal year, or a custom date range, to see income, cost of goods sold, expenses and net income. Fiscal periods follow the fiscal year start set in Settings.',
    keywords: 'profit loss income statement p&l net income revenue',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'balance-sheet', area: 'reports', views: ['reports', 'dashboard'],
    title: 'Run a Balance Sheet',
    body: 'Choose an as-of date to see assets, liabilities and equity at that moment. Profit from past fiscal years appears as Retained Earnings and this year’s as Net Income.',
    keywords: 'balance sheet assets liabilities equity retained earnings',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'export', area: 'reports', views: ['reports'],
    title: 'Export, print or save a report as PDF',
    body: 'Use Export on a report to save it as a CSV file for a spreadsheet, or to print it. Choose Save as PDF in the print dialog to keep a copy with your company logo.',
    keywords: 'export csv excel print pdf save share',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'trial-balance', area: 'reports', views: ['dashboard'],
    title: 'Read the trial balance',
    body: 'The Dashboard’s trial balance lists every account with a balance and shows ✓ In balance when total debits equal total credits. Because every entry must balance to post, it always should.',
    keywords: 'trial balance dashboard debits credits in balance',
    go: { label: 'Go to the Dashboard', view: 'dashboard' },
  },

  // Company & data
  {
    id: 'preferences', area: 'company', views: ['settings'],
    title: 'Set company details and preferences',
    body: 'The Preferences Console holds your company name, address, logo, fiscal year start, currency symbol, home currency and tax form; each saves as soon as you leave the field. The logo and details appear on printed invoices and reports.',
    keywords: 'company name address logo fiscal year currency settings preferences tax form',
    go: { label: 'Go to Preferences', view: 'settings', action: 'preferences' },
  },
  {
    id: 'closing-date', area: 'company', views: ['settings', 'accounting/journal'],
    title: 'Close a fiscal or tax period',
    body: 'Set a closing date to lock the books through that date, so nothing dated on or before it can be posted or voided from any screen. Add a passcode to require it for moving or removing the date; every change is kept in the closing history.',
    keywords: 'lock close year end period tax password passcode books',
    go: { label: 'Go to the closing date', view: 'settings', action: 'closing' },
  },
  {
    id: 'backup', area: 'company', views: ['settings'],
    title: 'Back up, restore or open a company file',
    body: 'File & Data Controls backs up your books to a .localbooks file, restores a backup (keeping a safety copy of the current books) or opens another company file; a daily backup is also taken automatically. Restoring replaces the whole company file, so anything entered after the backup was taken is not kept.',
    keywords: 'backup restore export file save localbooks open copy',
    go: { label: 'Go to File & Data Controls', view: 'settings', action: 'data' },
  },
  {
    id: 'file-location', area: 'company', views: ['settings'],
    title: 'Keep the company file on a network or synced drive',
    body: 'Change Company File Location moves your books to another folder, including a network drive or a synced folder such as Dropbox. Only one person can have the books open at a time, and LocalBooks tells a second computer they are in use.',
    keywords: 'network drive share dropbox icloud onedrive location move folder',
    go: { label: 'Go to File & Data Controls', view: 'settings', action: 'data' },
  },
  {
    id: 'mcp', area: 'company', views: ['settings', 'reports', 'dashboard'],
    title: 'Connect Claude, Cursor or another AI assistant (local MCP)',
    body: 'Turn on AI Access (MCP) to let an assistant on this computer read your trial balance, profit and loss summary and overdue receivables; it cannot change anything, and it answers only on this computer. Copy the ready-made Claude Code command, or the JSON configuration for clients such as Claude Desktop or Cursor.',
    keywords: 'ai claude cursor mcp model context protocol assistant chatgpt token llm',
    go: { label: 'Go to AI Access', view: 'settings', action: 'ai' },
  },
  {
    id: 'shortcuts', area: 'company', views: [],
    title: 'Keyboard shortcuts',
    body: 'Press Ctrl+Alt+N (⌘⌥N on a Mac) to open + New from anywhere, and F1 to open or close Help. Esc closes Help and other panels.',
    keywords: 'keyboard shortcut hotkey keys f1 new',
    go: null,
  },
];

/** The step-by-step guide (docs/guide/<id>.md) behind each topic. */
const ARTICLE_GUIDES = {
  invoice: 'invoices', 'foreign-currency': 'invoices', 'receive-payment': 'customer-payments',
  'sales-receipt': 'sales-receipts-and-estimates', estimate: 'sales-receipts-and-estimates',
  customers: 'customers-and-products', products: 'customers-and-products', vendors: 'customers-and-products', inventory: 'inventory',
  'fx-revaluation': 'currency-revaluation',
  'expense-check': 'expenses-and-checks', bill: 'bills', 'pay-bills': 'bills', time: 'time-tracking',
  'bank-feed': 'bank-feeds', deposit: 'deposits-and-transfers', transfer: 'deposits-and-transfers', reconcile: 'reconciling',
  'journal-entry': 'journal-entries-and-voids', void: 'journal-entries-and-voids',
  'ledger-discrepancies': 'ledger-integrity', 'audit-log': 'ledger-integrity',
  'qbw-import': 'quickbooks-import', 'qbw-undo': 'quickbooks-import',
  'profit-loss': 'reports', 'balance-sheet': 'reports', export: 'reports', 'trial-balance': 'reports',
  preferences: 'getting-started', 'closing-date': 'closing-the-books', backup: 'backups-and-company-files',
  'file-location': 'backups-and-company-files', mcp: 'ai-assistant', shortcuts: 'troubleshooting',
};

/** The main guide for each screen, offered at the top of Help. */
const SCREEN_GUIDES = {
  dashboard: 'getting-started', 'sales/all': 'sales-receipts-and-estimates', 'sales/invoices': 'invoices',
  'sales/estimates': 'sales-receipts-and-estimates', 'sales/customers': 'customers-and-products',
  'sales/products': 'inventory', 'expenses/expenses': 'expenses-and-checks', 'expenses/bills': 'bills',
  'expenses/vendors': 'customers-and-products', 'employees/time': 'time-tracking', 'banking/feeds': 'bank-feeds',
  'banking/transfers': 'deposits-and-transfers', 'banking/reconcile': 'reconciling', 'accounting/journal': 'journal-entries-and-voids',
  'accounting/audit': 'ledger-integrity', 'accounting/qbw-import': 'quickbooks-import', reports: 'reports',
  'accounting/fx-revaluation': 'currency-revaluation',
  settings: 'getting-started',
};

/** True when every word of the query appears in the article (title, text or keywords). */
export function matchesQuery(article, query) {
  const haystack = `${article.title} ${article.body} ${article.keywords}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/** One topic: the title toggles the two-sentence answer, its shortcut and its full guide. */
function Article({ article, view, expanded, onToggle, onNavigate, onOpenGuide }) {
  const guide = ARTICLE_GUIDES[article.id];
  // A plain "Go to" this very screen is noise; a shortcut that opens a form here is still useful.
  const go = article.go && !(article.go.view === view && !article.go.action) ? article.go : null;
  return (
    <li className="border-b border-qb-border last:border-b-0">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        className="flex w-full items-start gap-1.5 px-3 py-1.5 text-left hover:bg-qb-row-hover"
      >
        <svg viewBox="0 0 10 10" width="8" height="8" className={`mt-1 flex-none transition-transform duration-75 ${expanded ? 'rotate-90' : ''}`} aria-hidden="true">
          <path d="M3 1.5L6.5 5 3 8.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
        <span className={`text-sm leading-tight ${expanded ? 'font-semibold' : ''}`}>{article.title}</span>
      </button>
      {expanded && (
        <div className="px-3 pb-2 pl-[26px]">
          <p className="leading-snug text-qb-ink-muted">{article.body}</p>
          {go && (
            <button
              type="button"
              onClick={() => onNavigate(go.view, go.action ?? null)}
              className="mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline"
            >
              {go.label}
              <Icon name="Go" />
            </button>
          )}
          {guide && GUIDES[guide] && (
            <button
              type="button"
              onClick={() => onOpenGuide(guide)}
              className={`mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-info hover:underline ${go ? 'ml-3' : ''}`}
            >
              <Icon name="Guide" />
              Step-by-step guide
            </button>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * Right-side help panel. Opens on the current screen's help: what the screen is for and the
 * tasks you can do there. Below that, every topic by area; searching looks across all topics.
 */
export default function HelpCenter({ view, onClose, onNavigate }) {
  const [query, setQuery] = useState('');
  const [expanded, setExpanded] = useState(() => new Set());
  const [openAreas, setOpenAreas] = useState(() => new Set());
  // The guide being read, with the trail of guides opened from it (for Back).
  const [reading, setReading] = useState([]);
  const guideId = reading[reading.length - 1] ?? null;
  const openGuide = (id) => setReading((r) => [...r, id]);
  const body = useRef(null);
  // Braces matter: newer engines return a Promise from scrollTo, which React would call as cleanup.
  useEffect(() => {
    body.current?.scrollTo?.(0, 0);
  }, [guideId]);
  const search = useRef(null);
  const screen = findNav(view);
  const screenTitle = screen?.item.label ?? 'This screen';
  const here = useMemo(() => HELP_ARTICLES.filter((a) => a.views.includes(view)), [view]);
  const results = useMemo(() => (query.trim() ? HELP_ARTICLES.filter((a) => matchesQuery(a, query)) : []), [query]);
  const guideResults = useMemo(
    () => (query.trim() ? GUIDE_ORDER.filter((id) => matchesQuery({ title: GUIDES[id].title, body: GUIDES[id].text, keywords: '' }, query)) : []),
    [query],
  );

  // A new screen starts with its first task open, so the answer is one click away at most.
  useEffect(() => {
    setExpanded(new Set(here.slice(0, 1).map((a) => a.id)));
  }, [here]);

  useEffect(() => {
    search.current?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const toggle = (setter) => (id) =>
    setter((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const toggleArticle = toggle(setExpanded);
  const toggleArea = toggle(setOpenAreas);
  const article = (a) => (
    <Article key={a.id} article={a} view={view} expanded={expanded.has(a.id)} onToggle={() => toggleArticle(a.id)} onNavigate={onNavigate} onOpenGuide={openGuide} />
  );

  return (
    <aside
      className={`no-print flex ${guideId ? 'w-[440px]' : 'w-80'} flex-none animate-[lb-slide-in_160ms_ease-out] flex-col border-l border-qb-border bg-white text-xs transition-[width] duration-150`}
      aria-label="Help"
    >
      <div className="flex h-toolbar flex-none items-center gap-2 border-b border-qb-border px-3">
        <Icon name="HelpCenter" className="text-qb-green" />
        <h2 className="text-md font-semibold">Help</h2>
        <button type="button" onClick={onClose} className="ml-auto rounded-xs p-1 text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink" aria-label="Close help">
          <Icon name="Close" />
        </button>
      </div>
      {guideId ? (
        <div className="flex flex-none items-center gap-2 border-b border-qb-border px-2 py-1.5">
          <button
            type="button"
            onClick={() => setReading((r) => r.slice(0, -1))}
            className="inline-flex items-center gap-1 rounded-xs px-1.5 py-0.5 font-semibold text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink"
          >
            <Icon name="Back" />
            {reading.length > 1 ? GUIDES[reading[reading.length - 2]]?.title : 'Back to help'}
          </button>
          <span className="ml-auto text-2xs text-qb-ink-faint">Also in docs/guide on GitHub</span>
        </div>
      ) : (
      <div className="flex-none border-b border-qb-border p-2">
        <label className="flex items-center gap-1.5 rounded-xs border border-qb-border-strong px-2 focus-within:border-qb-green focus-within:shadow-focus">
          <Icon name="Search" className="text-qb-ink-muted" />
          <input
            ref={search}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search all help (e.g. undo, close the year)"
            className="h-row-lg min-w-0 flex-1 bg-transparent text-sm outline-none"
            aria-label="Search help"
          />
        </label>
      </div>
      )}

      <div ref={body} className="min-h-0 flex-1 overflow-y-auto scrollbar-thin">
        {guideId ? (
          <GuideView id={guideId} onOpenGuide={openGuide} />
        ) : query.trim() ? (
          <section aria-label="Search results">
            <h3 className="bg-qb-row-header px-3 py-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted" aria-live="polite">
              {results.length} topic{results.length === 1 ? '' : 's'}
            </h3>
            {results.length === 0 && guideResults.length === 0 ? (
              <p className="px-3 py-3 text-qb-ink-muted">Nothing matches “{query}”. Try fewer or different words.</p>
            ) : (
              <ul>{results.map(article)}</ul>
            )}
            {guideResults.length > 0 && (
              <>
                <h3 className="bg-qb-row-header px-3 py-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Guides that mention it</h3>
                <ul>
                  {guideResults.map((id) => (
                    <li key={id}>
                      <button type="button" onClick={() => openGuide(id)} className="flex w-full items-center gap-2 px-3 py-1 text-left text-sm hover:bg-qb-row-hover">
                        <Icon name="Guide" className="text-qb-ink-muted" />
                        {GUIDES[id].title}
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        ) : (
          <>
            <section aria-label={`Help for ${screenTitle}`} className="border-b border-qb-border">
              <div className="bg-qb-green-tint px-3 py-2">
                <div className="text-2xs font-semibold uppercase tracking-wide text-qb-green-active">On this screen</div>
                <h3 className="text-md font-semibold">{screenTitle}</h3>
                {SCREEN_HELP[view] && <p className="mt-0.5 leading-snug text-qb-ink">{SCREEN_HELP[view]}</p>}
                {GUIDES[SCREEN_GUIDES[view]] && (
                  <button
                    type="button"
                    onClick={() => openGuide(SCREEN_GUIDES[view])}
                    className="mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline"
                  >
                    <Icon name="Guide" />
                    Step-by-step: {GUIDES[SCREEN_GUIDES[view]].title}
                  </button>
                )}
              </div>
              {here.length > 0 && (
                <>
                  <h4 className="px-3 pb-0.5 pt-2 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">What you can do here</h4>
                  <ul>{here.map(article)}</ul>
                </>
              )}
            </section>
            <section aria-label="All topics">
              <h3 className="px-3 pb-1 pt-3 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">All topics</h3>
              <ul>
                {HELP_AREAS.map((area) => {
                  const topics = HELP_ARTICLES.filter((a) => a.area === area.id);
                  const open = openAreas.has(area.id);
                  return (
                    <li key={area.id} className="border-t border-qb-border">
                      <button
                        type="button"
                        onClick={() => toggleArea(area.id)}
                        aria-expanded={open}
                        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm font-semibold hover:bg-qb-row-hover"
                      >
                        <Icon name={area.icon} className="text-qb-ink-muted" />
                        <span className="flex-1">{area.label}</span>
                        <span className="text-2xs font-normal text-qb-ink-faint">{topics.length}</span>
                      </button>
                      {open && <ul className="bg-qb-canvas/40">{topics.map(article)}</ul>}
                    </li>
                  );
                })}
              </ul>
            </section>
            <section aria-label="Step-by-step guides" className="border-t border-qb-border pb-2">
              <h3 className="px-3 pb-1 pt-3 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Step-by-step guides</h3>
              <ul>
                {GUIDE_ORDER.map((id) => (
                  <li key={id}>
                    <button type="button" onClick={() => openGuide(id)} className="flex w-full items-center gap-2 px-3 py-1 text-left text-sm hover:bg-qb-row-hover">
                      <Icon name="Guide" className="text-qb-ink-muted" />
                      {GUIDES[id].title}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          </>
        )}
      </div>
    </aside>
  );
}
