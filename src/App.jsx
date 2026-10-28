import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Sidebar, { canOpen, findNav, openableInstead, resolveView } from './components/Sidebar.jsx';
import QuickCreateModal from './components/QuickCreateModal.jsx';
import { UpdateBanner } from './components/Updates.jsx';
import HelpCenter from './components/HelpCenter.jsx';
import { useDueRecurring } from './components/Recurring.jsx';
import { StartupReminders } from './components/Reminders.jsx';
import GlobalSearch from './components/GlobalSearch.jsx';
import OpenWindowList from './components/OpenWindowList.jsx';
import Screen from './components/Screen.jsx';
import WindowManager, { useChildWindows, windowName, WindowButtons } from './components/WindowManager.jsx';
import { hasPendingMigration, takeWindowlessOpen, useCompanyFileActions, WINDOWLESS_KEY_NAME } from './components/CompanyFiles.jsx';
import { useIdleLock } from './components/Users.jsx';
import { Icon } from './lib/icons.js';
import Notice from './components/Notice.jsx';
import { CompanyContext } from './components/PrintDocuments.jsx';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { api, COMPANY_FILE_CHANGED, useLedgerQuery } from './lib/api.js';
import { formatDate } from './lib/dates.js';
import { setCurrencySymbol } from './lib/money.js';
import * as cw from './lib/childWindows.js';
import {
  guardWindowClose,
  isMainWindow,
  MENU_COMMAND,
  MENU_NAVIGATE,
  NEW_WINDOW_REQUESTED,
  openWindow,
  restoredDesktop,
  startOnPreference,
  startPage,
  useWindowPreferences,
  WindowManagerContext,
} from './lib/windows.js';

const MAIN_WINDOW = isMainWindow();
const IS_MAC = /Mac/.test(navigator.userAgent);
// The company was just opened without restoring windows: it starts on an empty desktop.
const WINDOWLESS = takeWindowlessOpen();
const leaf = (view) => !!view && !!findNav(view) && !findNav(view).item.children;

/**
 * The windows this LocalBooks window starts with. The page it was opened on (Window › New Window) or
 * the Import wizard for a QuickBooks file being migrated comes first. Otherwise Edit › Preferences ›
 * Desktop View › Start on decides: the company's saved desktop (else the Home page), the Home page,
 * one chosen screen, or no windows. A company opened without restoring windows starts with none.
 */
function initialWindows(area) {
  if (WINDOWLESS) return cw.EMPTY;
  const { startOn, startScreen } = startOnPreference();
  const opened = startPage();
  const saved = restoredDesktop();
  if (!opened && !hasPendingMigration()) {
    if (startOn === 'no-windows') return cw.EMPTY;
    if ((startOn ?? 'last-windows') === 'last-windows' && saved?.windows?.length) return cw.fromSaved(saved.windows, area);
  }
  const chosen = startOn === 'screen' && startScreen ? { view: startScreen } : { view: 'dashboard' };
  // A QuickBooks file picked in Open or Restore Company: the wizard reads it into this new company.
  const start = hasPendingMigration() ? { view: 'settings/import' } : (opened ?? chosen);
  const view = resolveView(start.view, start.action);
  const page = leaf(view) ? { ...start, view } : { view: 'dashboard' };
  return cw.open(cw.EMPTY, page, { area, maximized: true });
}

export default function App() {
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
  const list = useMemo(() => accounts.data ?? [], [accounts.data]);
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

  // Forms, registers and reports open in windows inside this one, as in QuickBooks Desktop.
  const windowPrefs = useWindowPreferences();
  const windowMode = windowPrefs.windowMode;
  // Edit › Preferences › Desktop View › Left pane: the full menu, the open windows only, or nothing.
  const sidebar = windowPrefs.sidebar ?? 'menu';
  const sidebarRef = useRef(sidebar);
  sidebarRef.current = sidebar;
  const manager = useChildWindows(windowMode, initialWindows);
  const { state, open: openChild, focus: focusChild, close: closeChild } = manager;
  const active = state.windows.find((w) => w.id === state.activeId) ?? null;
  const activeView = active?.view ?? null;
  const activeMaximized = !!active && !active.minimized && (windowMode === 'one' || active.maximized);

  // Opening a page brings forward the window that shows it, or opens one. `data` carries what the
  // destination needs, e.g. { estimateId } for an estimate loaded into the invoice form.
  // `settings` with an action opens that Settings tab; a moved page opens where it is now.
  const select = useCallback(
    (requested, action = null, data = null) => {
      const id = resolveView(requested, action);
      return leaf(id) && openChild(id, action, data);
    },
    [openChild],
  );
  // A window on an Admin-only page (from a desktop saved while an Admin, say) shows someone else
  // their first allowed Settings tab, and is titled for it. Only once the sign-in state is known.
  const signInKnown = !!signIn.data;
  const { redirect } = manager;
  useEffect(() => {
    if (signInKnown) redirect((view) => canOpen(view, isAdmin), openableInstead);
  }, [signInKnown, isAdmin, state.windows, redirect]);

  const managerContext = useMemo(
    () => ({ windows: state.windows, activeId: state.activeId, open: openChild, focus: focusChild, close: closeChild, windowMode }),
    [state.windows, state.activeId, openChild, focusChild, closeChild, windowMode],
  );

  // Another company file was opened: every window's state belongs to the old books, so start fresh.
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

  // Closing LocalBooks with unsaved entries in a form asks first.
  useEffect(() => guardWindowClose(), []);

  // File › New Company… and Open or Restore Company… (the window reloads on the company opened).
  const companyFiles = useCompanyFileActions();

  // Window › New Window: another LocalBooks window, for a second screen, on the active window's page.
  const [windowError, setWindowError] = useState(null);
  const openNewWindow = useCallback(() => {
    setWindowError(null);
    const page = active ? cw.currentPage(active) : { view: 'dashboard' };
    openWindow(page.view, page.action, page.data).catch((err) => setWindowError(String(err)));
  }, [active]);

  // The menu bar's Lists, Company, Customers, … items open a page (often with its entry form), and a
  // few act on the page itself (see app_menu.rs).
  useEffect(() => {
    const unlistens = [];
    let cancelled = false;
    const win = getCurrentWebviewWindow();
    const keep = (p) => p.then((fn) => (cancelled ? fn() : unlistens.push(fn))).catch(() => {});
    keep(win.listen(MENU_NAVIGATE, ({ payload }) => select(payload.view, payload.action, payload.data)));
    keep(win.listen(MENU_COMMAND, ({ payload }) => {
      if (payload === 'help') setHelpOpen(true);
      // View › Show or Hide Left Icon Bar: hides the left pane, or brings back the full menu; kept for next time.
      if (payload === 'sidebar') api.setWindowPreferences({ sidebar: sidebarRef.current === 'hidden' ? 'menu' : 'hidden' }).catch(() => {});
    }));
    keep(win.listen(NEW_WINDOW_REQUESTED, () => openNewWindow()));
    return () => {
      cancelled = true;
      unlistens.forEach((fn) => fn());
    };
  }, [select, openNewWindow]);

  // Ctrl/Cmd+Alt+N opens the + New matrix from anywhere; F1 toggles Help; Ctrl+Shift+N opens a new
  // LocalBooks window (on macOS the Window menu's Cmd+Shift+N does).
  useEffect(() => {
    const onKey = (e) => {
      if (!IS_MAC && e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        openNewWindow();
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
  }, [openNewWindow]);

  const dueRecurring = useDueRecurring();
  const nav = activeView ? findNav(activeView) : null;
  const parent = active && !active.content?.name ? nav?.parent : null;
  const renderScreen = useCallback(
    (w) => <Screen view={w.view} intent={w.intent} navigate={select} accounts={list} isAdmin={isAdmin} companyName={companyName} />,
    [select, list, isAdmin, companyName],
  );

  return (
    <CompanyContext.Provider value={company}>
    <WindowManagerContext.Provider value={managerContext}>
    <div className="flex h-full">
      {sidebar === 'menu' && <Sidebar current={activeView} onSelect={(id) => select(id)} onNew={() => setCreating(true)} companyName={openCompany ?? 'LocalBooks'} storage={storage.data?.storage} isAdmin={isAdmin} />}
      {/* The left pane as the Open Window List alone, or the list beside the menu (View › Open Window List). */}
      {(sidebar === 'open-windows' || windowPrefs.showOpenWindowList) && <OpenWindowList asSidebar={sidebar === 'open-windows'} companyName={openCompany ?? 'LocalBooks'} />}
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="qb-toolbar no-print flex-none">
          {parent && <span className="text-sm text-qb-ink-muted">{parent.label} ›</span>}
          <h1 className="min-w-0 truncate text-lg font-semibold">{active ? windowName(active) : (openCompany ?? companyName)}</h1>
          <div className="ml-auto flex min-w-0 items-center gap-3">
          {dueRecurring > 0 && activeView !== 'accounting/recurring' && (
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
              className="flex min-w-0 max-w-[480px] items-center gap-1.5 overflow-hidden text-xs text-qb-ink-muted"
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
            {/* A maximized window's buttons sit at the end of the toolbar, as they move to the menu bar in QuickBooks. */}
            {activeMaximized && (
              <WindowButtons
                name={windowName(active)}
                maximized
                onMinimize={windowMode === 'one' ? null : () => manager.minimize(active.id)}
                onMaximize={windowMode === 'one' ? null : () => manager.toggleMaximize(active.id)}
                onClose={() => closeChild(active.id)}
              />
            )}
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
            <Notice kind="info" text={`Archive of books condensed through ${formatDate(condensed.data.archiveThrough)}: read-only, for reports and look-ups. Open your current company file to make changes.`} />
          </div>
        )}
        {accounts.error && <Notice kind="error" text={`Could not load accounts: ${accounts.error}`} />}
        <WindowManager
          manager={manager}
          windowMode={windowMode}
          renderScreen={renderScreen}
          background={<EmptyDesktop companyName={openCompany ?? companyName} windowless={WINDOWLESS} onHome={() => select('dashboard')} />}
        />
      </main>
      {/* Beside the work area, not over it: the open windows stay mounted, so unsaved form entries survive. */}
      {helpOpen && (
        <HelpCenter
          view={activeView}
          onClose={() => setHelpOpen(false)}
          onNavigate={(viewId, action) => select(viewId, action)}
        />
      )}
      {companyFiles.dialog}
      {/* Reminders at startup, when the user chose them (and not when opened without restoring windows). */}
      {MAIN_WINDOW && !WINDOWLESS && signIn.data && (!signIn.data.usersEnabled || signedIn) && (
        <StartupReminders fileKey={storage.data?.path} userKey={signedIn?.id} navigate={select} />
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
    </WindowManagerContext.Provider>
    </CompanyContext.Provider>
  );
}

/** The work area with no window open: after Close All, or a company opened without restoring windows. */
function EmptyDesktop({ companyName, windowless, onHome }) {
  return (
    <div className="p-3">
      <section className="qb-panel max-w-[560px] p-3">
        <h2 className="mb-1 text-md font-semibold">{companyName} is open</h2>
        {windowless ? (
          <>
            <p className="mb-2 text-sm text-qb-ink-muted">
              It was opened without restoring windows, so the windows you had open were not reopened. Choose a page from the sidebar to carry on.
            </p>
            <p className="mb-3 text-sm text-qb-ink-muted">
              To open a company this way, hold {WINDOWLESS_KEY_NAME} while clicking Open, or choose File › Open Company Without Restoring Windows…
            </p>
          </>
        ) : (
          <p className="mb-3 text-sm text-qb-ink-muted">No windows are open. Choose a page from the sidebar or the menu bar to open one.</p>
        )}
        <button type="button" className="qb-btn qb-btn-primary" onClick={onHome}>
          Open the Home page
        </button>
      </section>
    </div>
  );
}
