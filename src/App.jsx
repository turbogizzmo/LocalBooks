import { useCallback, useEffect, useState } from 'react';
import Sidebar, { findNav } from './components/Sidebar.jsx';
import QuickCreateModal from './components/QuickCreateModal.jsx';
import Dashboard from './components/Dashboard.jsx';
import AllSales from './components/AllSales.jsx';
import Invoices from './components/Invoices.jsx';
import Estimates from './components/Estimates.jsx';
import PartyList from './components/PartyList.jsx';
import Products from './components/Products.jsx';
import SalesLists from './components/SalesLists.jsx';
import ExpensesChecks from './components/ExpensesChecks.jsx';
import Bills from './components/Bills.jsx';
import TimeTracking from './components/TimeTracking.jsx';
import BankFeed from './components/BankFeed.jsx';
import DepositsTransfers from './components/DepositsTransfers.jsx';
import Reconcile from './components/Reconcile.jsx';
import JournalEntries from './components/JournalEntries.jsx';
import AuditLog from './components/AuditLog.jsx';
import ChartOfAccounts from './components/ChartOfAccounts.jsx';
import Classes from './components/Classes.jsx';
import Reclassify from './components/Reclassify.jsx';
import Register from './components/Register.jsx';
import CreditMemos from './components/CreditMemos.jsx';
import VendorCredits from './components/VendorCredits.jsx';
import SalesTax from './components/SalesTax.jsx';
import Reports from './components/Reports.jsx';
import Settings from './components/Settings.jsx';
import QbwImporter from './components/QbwImporter.jsx';
import { UpdateBanner } from './components/Updates.jsx';
import HelpCenter from './components/HelpCenter.jsx';
import FxRevaluation from './components/FxRevaluation.jsx';
import Statements from './components/Statements.jsx';
import FinanceCharges from './components/FinanceCharges.jsx';
import SendForms from './components/SendForms.jsx';
import Recurring, { useDueRecurring } from './components/Recurring.jsx';
import PurchaseOrders from './components/PurchaseOrders.jsx';
import Budgets from './components/Budgets.jsx';
import GlobalSearch from './components/GlobalSearch.jsx';
import FindTransactions from './components/FindTransactions.jsx';
import { hasPendingMigration, takeWindowlessOpen, useCompanyFileActions, WINDOWLESS_KEY_NAME } from './components/CompanyFiles.jsx';
import { useIdleLock } from './components/Users.jsx';
import { Icon } from './lib/icons.js';
import Notice from './components/Notice.jsx';
import { CompanyContext } from './components/PrintDocuments.jsx';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { api, COMPANY_FILE_CHANGED, useLedgerQuery } from './lib/api.js';
import { setCurrencySymbol } from './lib/money.js';
import { isMainWindow, MENU_COMMAND, MENU_NAVIGATE, NEW_WINDOW_REQUESTED, openWindow, startPage } from './lib/windows.js';

const STORAGE_KEY = 'localbooks.view';
// A window opened on a page (Window › New Window, or a register or report in its own window).
const START = startPage();
const MAIN_WINDOW = isMainWindow();
const IS_MAC = /Mac/.test(navigator.userAgent);
// The company was just opened without restoring windows: start on a plain home page, not the page
// last shown (or this window's own page), and forget that page so it does not come back next time.
const WINDOWLESS = takeWindowlessOpen();
if (WINDOWLESS && MAIN_WINDOW) {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Storage unavailable: nothing is remembered to forget.
  }
}

function initialView() {
  if (WINDOWLESS) return 'dashboard';
  if (START && findNav(START.view) && !findNav(START.view).item.children) return START.view;
  // A QuickBooks file picked in Open or Restore Company: the wizard reads it into this new company.
  if (hasPendingMigration()) return 'accounting/qbw-import';
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && findNav(saved) && !findNav(saved).item.children) return saved;
  } catch {
    // Storage unavailable: fall through to the default view.
  }
  return 'dashboard';
}

export default function App() {
  const [view, setView] = useState(initialView);
  // An intent asks the destination view to open a specific entry form ({ action, nonce }).
  const [intent, setIntent] = useState(() => (START?.action && !WINDOWLESS ? { action: START.action, data: START.data, nonce: Date.now() } : null));
  // Opened without restoring windows: the plain home page shows until a page is chosen.
  const [windowless, setWindowless] = useState(WINDOWLESS);
  const [creating, setCreating] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const accounts = useLedgerQuery(api.listAccounts);
  const prefs = useLedgerQuery(api.getPreferences);
  const logo = useLedgerQuery(api.getCompanyLogo);
  const storage = useLedgerQuery(api.getStorageStatus);
  const signIn = useLedgerQuery(api.getSignInState);
  const condensed = useLedgerQuery(api.getCondenseStatus);
  const signedIn = signIn.data?.user ?? null;
  useIdleLock(signIn.data);
  // Without users, whoever opens the file can do everything. Until the sign-in state has loaded,
  // nobody counts as an Admin, so a View-only or Standard user never sees Admin screens first.
  const isAdmin = signIn.data ? !signIn.data.usersEnabled || signedIn?.role === 'ADMIN' : false;
  const list = accounts.data ?? [];
  // Applied during render so every amount on screen uses the company's symbol in the same pass.
  setCurrencySymbol(prefs.data?.currency_symbol);
  const companyName = prefs.data?.company_name ?? 'LocalBooks';
  // Which company is open, as the window title shows it: its name, or the file's name until it is named.
  const openCompany = storage.data?.companyName ?? null;
  const company = {
    name: companyName,
    legalName: prefs.data?.company_legal_name ?? '',
    address: prefs.data?.company_address ?? '',
    email: prefs.data?.company_email ?? '',
    phone: prefs.data?.company_phone ?? '',
    ein: prefs.data?.company_ein ?? '',
    logo: logo.data ?? null,
    homeCurrency: prefs.data?.home_currency_code ?? 'USD',
    fxLookup: prefs.data?.exchange_rate_lookup === 'true',
  };

  // `data` carries what the destination needs, e.g. { estimateId } for an estimate loaded into the invoice form.
  const select = useCallback((id, action = null, data = null) => {
    setWindowless(false);
    setView(id);
    setIntent(action ? { action, data, nonce: Date.now() } : null);
    if (!MAIN_WINDOW) return;
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      // Remembering the view is a convenience only.
    }
  }, []);

  // Another company file was opened: every view's state belongs to the old books, so start fresh.
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    listen(COMPANY_FILE_CHANGED, () => window.location.reload())
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // File › New Company… and Open or Restore Company… (the window reloads on the company opened).
  const companyFiles = useCompanyFileActions();

  const [windowError, setWindowError] = useState(null);
  const openNewWindow = useCallback((id) => {
    setWindowError(null);
    openWindow(id).catch((err) => setWindowError(String(err)));
  }, []);

  // The menu bar's Lists, Company, Customers, … items open a page (often with its entry form), and a
  // few act on the page itself (see app_menu.rs).
  const [sidebarHidden, setSidebarHidden] = useState(false);
  useEffect(() => {
    const unlistens = [];
    let cancelled = false;
    const win = getCurrentWebviewWindow();
    const keep = (p) => p.then((fn) => (cancelled ? fn() : unlistens.push(fn))).catch(() => {});
    keep(win.listen(MENU_NAVIGATE, ({ payload }) => {
      if (findNav(payload.view)) select(payload.view, payload.action, payload.data);
    }));
    keep(win.listen(MENU_COMMAND, ({ payload }) => {
      if (payload === 'help') setHelpOpen(true);
      if (payload === 'sidebar') setSidebarHidden((h) => !h);
    }));
    return () => {
      cancelled = true;
      unlistens.forEach((fn) => fn());
    };
  }, [select]);

  // Window › New Window asks the focused window to open the page it shows.
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    getCurrentWebviewWindow()
      .listen(NEW_WINDOW_REQUESTED, () => openNewWindow(view))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [view, openNewWindow]);

  // Ctrl/Cmd+Alt+N opens the + New matrix from anywhere; F1 toggles Help; Ctrl+Shift+N opens a new
  // window (on macOS the Window menu's Cmd+Shift+N does).
  useEffect(() => {
    const onKey = (e) => {
      if (!IS_MAC && e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        openNewWindow(view);
        return;
      }
      if (e.key === 'F1') {
        e.preventDefault();
        setHelpOpen((o) => !o);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.altKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        setCreating(true);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [view, openNewWindow]);

  const dueRecurring = useDueRecurring();
  const { item, parent } = windowless ? { item: { label: 'Home' }, parent: null } : findNav(view);
  const props = { accounts: list, intent, navigate: select };

  return (
    <CompanyContext.Provider value={company}>
    <div className="flex h-full">
      {!sidebarHidden && <Sidebar current={windowless ? null : view} onSelect={(id) => select(id)} onNew={() => setCreating(true)} companyName={openCompany ?? 'LocalBooks'} storage={storage.data?.storage} />}
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="qb-toolbar no-print flex-none">
          {parent && <span className="text-sm text-qb-ink-muted">{parent.label} ›</span>}
          <h1 className="text-lg font-semibold">{item.label}</h1>
          <div className="ml-auto flex min-w-0 items-center gap-3">
          {dueRecurring > 0 && view !== 'accounting/recurring' && (
            <button
              type="button"
              className="flex-none rounded-sm bg-qb-green-tint px-2 py-0.5 text-xs font-semibold text-qb-green-active hover:underline"
              onClick={() => select('accounting/recurring')}
            >
              {dueRecurring} recurring transaction{dueRecurring === 1 ? '' : 's'} due
            </button>
          )}
          <GlobalSearch navigate={select} />
          {signedIn && (
            <button
              type="button"
              className="flex-none rounded-xs px-2 py-0.5 text-xs font-semibold text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink"
              onClick={() => select('settings', 'users')}
              title="Your sign-in, in Settings › Users & Passwords"
            >
              {signedIn.displayName}
            </button>
          )}
          {storage.data && (
            <span
              className="flex min-w-0 max-w-[480px] items-center gap-1.5 text-xs text-qb-ink-muted"
              title={storage.data.path}
              aria-label={`Company: ${openCompany ?? 'unnamed'}, file ${storage.data.fileName}`}
            >
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 flex-none" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
                <path d="M2.5 14.5v-11l5-2v13M7.5 5.5h6v9M1 14.5h14M4.5 6h1M4.5 9h1M10 8h1.5M10 11h1.5" />
              </svg>
              {openCompany ? (
                <span className="truncate text-sm font-semibold text-qb-ink">{openCompany}</span>
              ) : (
                <button
                  type="button"
                  className="flex-none font-semibold text-qb-link hover:underline"
                  onClick={() => select('settings', 'preferences')}
                  title="Give this company a name in Settings › Preferences"
                >
                  Name this company
                </button>
              )}
              <span className="truncate">· {storage.data.fileName}</span>
              {storage.data.openedFromFile && (
                <span className="flex-none rounded-sm border border-qb-border px-1 text-2xs uppercase tracking-wide">Opened file</span>
              )}
            </span>
          )}
            <button
              type="button"
              onClick={() => openNewWindow(view)}
              title={`Open this page in a new window (${IS_MAC ? '⇧⌘N' : 'Ctrl+Shift+N'})`}
              aria-label="Open this page in a new window"
              className="flex flex-none items-center gap-1 rounded-xs px-2 py-0.5 text-sm font-semibold text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink"
            >
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
                <path d="M5.5 1.5h9v9h-9zM10.5 13v1.5h-9v-9H3" />
              </svg>
              New window
            </button>
            <button
              type="button"
              onClick={() => setHelpOpen((o) => !o)}
              aria-pressed={helpOpen}
              title="Help (F1)"
              className={`flex flex-none items-center gap-1 rounded-xs px-2 py-0.5 text-sm font-semibold ${
                helpOpen ? 'bg-qb-green-tint text-qb-green-active' : 'text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink'
              }`}
            >
              <Icon name="HelpCenter" />
              Help
            </button>
          </div>
        </header>
        {/* Updates are checked and offered in the main window only. */}
        {MAIN_WINDOW && <UpdateBanner />}
        {windowError && <Notice kind="error" text={windowError} onDismiss={() => setWindowError(null)} />}
        {companyFiles.error && <Notice kind="error" text={companyFiles.error} onDismiss={companyFiles.clearError} />}
        {storage.data?.startupWarning && (
          <div className="no-print">
            <Notice kind="info" text={`${storage.data.startupWarning} Change it in Settings › File & Data Controls.`} />
          </div>
        )}
        {signedIn?.role === 'VIEW_ONLY' && (
          <div className="no-print">
            <Notice kind="info" text={`Signed in as ${signedIn.displayName}, view only: you can look at the books and run reports, but not change anything.`} />
          </div>
        )}
        {condensed.data?.isArchive && (
          <div className="no-print">
            <Notice kind="info" text={`Archive of books condensed through ${condensed.data.archiveThrough}: read-only, for reports and look-ups. Open your current company file to make changes.`} />
          </div>
        )}
        {accounts.error && <Notice kind="error" text={`Could not load accounts: ${accounts.error}`} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {windowless && <WindowlessHome companyName={openCompany ?? companyName} onDashboard={() => select('dashboard')} />}
          {view === 'dashboard' && !windowless && <Dashboard />}
          {view === 'sales/all' && <AllSales {...props} />}
          {view === 'sales/invoices' && <Invoices {...props} />}
          {view === 'sales/estimates' && <Estimates {...props} />}
          {view === 'sales/credits' && <CreditMemos {...props} />}
          {view === 'sales/customers' && <PartyList kind="customer" />}
          {view === 'sales/products' && <Products accounts={list} intent={intent} />}
          {view === 'sales/lists' && <SalesLists />}
          {view === 'sales/tax' && <SalesTax accounts={list} />}
          {view === 'sales/statements' && <Statements />}
          {view === 'sales/finance-charges' && <FinanceCharges accounts={list} navigate={select} />}
          {view === 'sales/send-forms' && <SendForms />}
          {view === 'expenses/expenses' && <ExpensesChecks {...props} />}
          {view === 'expenses/bills' && <Bills {...props} />}
          {view === 'expenses/credits' && <VendorCredits {...props} />}
          {view === 'expenses/purchase-orders' && <PurchaseOrders {...props} />}
          {view === 'expenses/vendors' && <PartyList kind="vendor" />}
          {view === 'employees/time' && <TimeTracking {...props} />}
          {view === 'banking/feeds' && <BankFeed accounts={list} />}
          {view === 'banking/transfers' && <DepositsTransfers {...props} />}
          {view === 'banking/reconcile' && <Reconcile accounts={list} />}
          {view === 'accounting/chart-of-accounts' && <ChartOfAccounts accounts={list} intent={intent} navigate={select} />}
          {view === 'accounting/classes' && <Classes />}
          {view === 'accounting/reclassify' && <Reclassify accounts={list} />}
          {view === 'accounting/register' && <Register accounts={list} intent={intent} />}
          {view === 'accounting/journal' && <JournalEntries {...props} />}
          {view === 'accounting/find' && <FindTransactions {...props} />}
          {view === 'accounting/recurring' && <Recurring />}
          {view === 'accounting/budgets' && <Budgets accounts={list} navigate={select} />}
          {view === 'accounting/audit' && <AuditLog />}
          {view === 'accounting/qbw-import' &&
            (isAdmin ? <QbwImporter accounts={list} /> : <div className="p-2"><Notice kind="info" text="Only an Admin can import another program's books." /></div>)}
          {view === 'accounting/fx-revaluation' && <FxRevaluation />}
          {view === 'reports' && <Reports navigate={select} intent={intent} />}
          {view === 'settings' && <Settings companyName={companyName} intent={intent} isAdmin={isAdmin} navigate={select} />}
        </div>
      </main>
      {/* Beside the workspace, not over it: the open view stays mounted, so unsaved form entries survive. */}
      {helpOpen && (
        <HelpCenter
          view={view}
          onClose={() => setHelpOpen(false)}
          onNavigate={(viewId, action) => select(viewId, action)}
        />
      )}
      {companyFiles.dialog}
      {creating && (
        <QuickCreateModal
          onClose={() => setCreating(false)}
          onCreate={(target) => {
            setCreating(false);
            select(target.view, target.id);
          }}
        />
      )}
    </div>
    </CompanyContext.Provider>
  );
}

/** The page a company opens on without restoring windows: nothing on it reads the books. */
function WindowlessHome({ companyName, onDashboard }) {
  return (
    <div className="p-3">
      <section className="qb-panel max-w-[560px] p-3">
        <h2 className="mb-1 text-md font-semibold">{companyName} is open</h2>
        <p className="mb-2 text-sm text-qb-ink-muted">
          It was opened without restoring windows, so the page you last had open was not reopened and any other windows were closed. Choose a page from the sidebar to
          carry on.
        </p>
        <p className="mb-3 text-sm text-qb-ink-muted">
          To open a company this way, hold {WINDOWLESS_KEY_NAME} while clicking Open, or choose File › Open Company Without Restoring Windows…
        </p>
        <button type="button" className="qb-btn qb-btn-primary" onClick={onDashboard}>
          Go to the Dashboard
        </button>
      </section>
    </div>
  );
}
