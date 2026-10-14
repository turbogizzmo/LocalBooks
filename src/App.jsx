import { useCallback, useEffect, useState } from 'react';
import Sidebar, { findNav } from './components/Sidebar.jsx';
import QuickCreateModal from './components/QuickCreateModal.jsx';
import Dashboard from './components/Dashboard.jsx';
import AllSales from './components/AllSales.jsx';
import Invoices from './components/Invoices.jsx';
import Estimates from './components/Estimates.jsx';
import PartyList from './components/PartyList.jsx';
import Products from './components/Products.jsx';
import ExpensesChecks from './components/ExpensesChecks.jsx';
import Bills from './components/Bills.jsx';
import TimeTracking from './components/TimeTracking.jsx';
import BankFeed from './components/BankFeed.jsx';
import DepositsTransfers from './components/DepositsTransfers.jsx';
import Reconcile from './components/Reconcile.jsx';
import JournalEntries from './components/JournalEntries.jsx';
import AuditLog from './components/AuditLog.jsx';
import Reports from './components/Reports.jsx';
import Settings from './components/Settings.jsx';
import QbwImporter from './components/QbwImporter.jsx';
import HelpCenter from './components/HelpCenter.jsx';
import { Icon } from './lib/icons.js';
import Notice from './components/Notice.jsx';
import { CompanyContext } from './components/PrintDocuments.jsx';
import { listen } from '@tauri-apps/api/event';
import { api, COMPANY_FILE_CHANGED, useLedgerQuery } from './lib/api.js';
import { setCurrencySymbol } from './lib/money.js';

const STORAGE_KEY = 'localbooks.view';

function initialView() {
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
  const [intent, setIntent] = useState(null);
  const [creating, setCreating] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const accounts = useLedgerQuery(api.listAccounts);
  const prefs = useLedgerQuery(api.getPreferences);
  const logo = useLedgerQuery(api.getCompanyLogo);
  const storage = useLedgerQuery(api.getStorageStatus);
  const list = accounts.data ?? [];
  // Applied during render so every amount on screen uses the company's symbol in the same pass.
  setCurrencySymbol(prefs.data?.currency_symbol);
  const companyName = prefs.data?.company_name ?? 'LocalBooks';
  const company = {
    name: companyName,
    legalName: prefs.data?.company_legal_name ?? '',
    address: prefs.data?.company_address ?? '',
    email: prefs.data?.company_email ?? '',
    phone: prefs.data?.company_phone ?? '',
    ein: prefs.data?.company_ein ?? '',
    logo: logo.data ?? null,
    homeCurrency: prefs.data?.home_currency_code ?? 'USD',
  };

  const select = useCallback((id, action = null) => {
    setView(id);
    setIntent(action ? { action, nonce: Date.now() } : null);
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

  // Ctrl/Cmd+Alt+N opens the + New matrix from anywhere; F1 toggles Help.
  useEffect(() => {
    const onKey = (e) => {
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
  }, []);

  const { item, parent } = findNav(view);
  const props = { accounts: list, intent };

  return (
    <CompanyContext.Provider value={company}>
    <div className="flex h-full">
      <Sidebar current={view} onSelect={(id) => select(id)} onNew={() => setCreating(true)} companyName={companyName} storage={storage.data?.storage} />
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="qb-toolbar no-print flex-none">
          {parent && <span className="text-sm text-qb-ink-muted">{parent.label} ›</span>}
          <h1 className="text-lg font-semibold">{item.label}</h1>
          <div className="ml-auto flex min-w-0 items-center gap-3">
          {storage.data && (
            <span
              className="flex min-w-0 max-w-[420px] items-center gap-1.5 text-xs text-qb-ink-muted"
              title={storage.data.path}
              aria-label={`Company file: ${storage.data.fileName}`}
            >
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 flex-none" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
                <path d="M4 1.5h5.5l3 3v10H4zM9.5 1.5v3h3" />
              </svg>
              <span className="truncate font-semibold text-qb-ink">{storage.data.fileName}</span>
              {storage.data.openedFromFile && (
                <span className="flex-none rounded-sm border border-qb-border px-1 text-2xs uppercase tracking-wide">Opened file</span>
              )}
            </span>
          )}
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
        {storage.data?.startupWarning && (
          <div className="no-print">
            <Notice kind="info" text={`${storage.data.startupWarning} Change it in Settings › File & Data Controls.`} />
          </div>
        )}
        {accounts.error && <Notice kind="error" text={`Could not load accounts: ${accounts.error}`} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {view === 'dashboard' && <Dashboard />}
          {view === 'sales/all' && <AllSales {...props} />}
          {view === 'sales/invoices' && <Invoices {...props} />}
          {view === 'sales/estimates' && <Estimates {...props} />}
          {view === 'sales/customers' && <PartyList kind="customer" />}
          {view === 'sales/products' && <Products accounts={list} />}
          {view === 'expenses/expenses' && <ExpensesChecks {...props} />}
          {view === 'expenses/bills' && <Bills {...props} />}
          {view === 'expenses/vendors' && <PartyList kind="vendor" />}
          {view === 'employees/time' && <TimeTracking {...props} />}
          {view === 'banking/feeds' && <BankFeed accounts={list} />}
          {view === 'banking/transfers' && <DepositsTransfers {...props} />}
          {view === 'banking/reconcile' && <Reconcile accounts={list} />}
          {view === 'accounting/journal' && <JournalEntries {...props} />}
          {view === 'accounting/audit' && <AuditLog />}
          {view === 'accounting/qbw-import' && <QbwImporter accounts={list} />}
          {view === 'reports' && <Reports />}
          {view === 'settings' && <Settings companyName={companyName} intent={intent} />}
        </div>
      </main>
      {/* Beside the workspace, not over it: the open view stays mounted, so unsaved form entries survive. */}
      {helpOpen && (
        <HelpCenter
          onClose={() => setHelpOpen(false)}
          onNavigate={(viewId, action) => select(viewId, action)}
        />
      )}
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
