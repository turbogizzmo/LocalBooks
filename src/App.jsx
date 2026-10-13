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
import Notice from './components/Notice.jsx';
import { CompanyContext } from './components/PrintDocuments.jsx';
import { api, useLedgerQuery } from './lib/api.js';
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

  // Ctrl/Cmd+Alt+N opens the + New matrix from anywhere.
  useEffect(() => {
    const onKey = (e) => {
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
          {view === 'settings' && <Settings companyName={companyName} />}
        </div>
      </main>
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
