import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import Notice from './Notice.jsx';
import { NAV } from './Sidebar.jsx';
import { api, errorText } from '../lib/api.js';

const IS_MAC = /Mac/.test(navigator.userAgent);
/** The screens a company can open on: every page of the sidebar, named as the sidebar groups it. */
const SCREENS = NAV.flatMap((item) => (item.children ? item.children.map((c) => [c.id, `${item.label} › ${c.label}`]) : [[item.id, item.label]]));

/**
 * Edit › Preferences › Desktop View, the My Preferences part, as in QuickBooks Desktop: View (One
 * Window or Multiple Windows) for this computer, and Desktop (what reopens with this company) for
 * you on this company file. Then the left pane (the full menu, the open windows only, or hidden) and
 * what a company opens on (the last windows, the Home page, a chosen screen or nothing), for this
 * computer. Each choice takes effect at once.
 */
export default function DesktopViewPreferences() {
  const [view, setView] = useState(null);
  const [notice, setNotice] = useState(null);
  useEffect(() => {
    let cancelled = false;
    let unlisten;
    api.getDesktopView().then(
      (next) => !cancelled && setView(next),
      (err) => !cancelled && setNotice({ kind: 'error', text: errorText(err) }),
    );
    // The View and Window › Workspace menus change the same choices.
    listen('desktop-view-changed', ({ payload }) => setView(payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  if (!view) return null;

  const run = (call, done) =>
    call.then(
      (next) => {
        setView(next);
        setNotice(done ? { kind: 'success', text: done(next) } : null);
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const chooseDesktop = (choice) =>
    run(api.setDesktopView(choice), choice === 'save-current' ? (next) => `Desktop saved: ${next.savedWindows} window${next.savedWindows === 1 ? '' : 's'} will reopen every time you open this company.` : null);
  const radio = (name, checked, onChange, label, hint, disabled = false) => (
    <label className={`flex items-start gap-1.5 px-2 py-0.5 text-sm ${disabled ? 'text-qb-ink-faint' : ''}`}>
      <input type="radio" name={name} className="mt-0.5 accent-qb-green" checked={checked} onChange={onChange} disabled={disabled} />
      <span>
        {label}
        {hint && <span className="block text-2xs text-qb-ink-faint">{hint}</span>}
      </span>
    </label>
  );

  return (
    <section className="qb-panel" aria-labelledby="desktop-view-title">
      <h2 id="desktop-view-title" className="border-b border-qb-border bg-qb-row-header px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">
        Desktop View · My Preferences
      </h2>
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 py-1">
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">View</legend>
          {radio('window-mode', view.windowMode === 'one', () => run(api.setWindowPreferences({ windowMode: 'one' })), 'One Window', 'One window shows at a time, filling the work area; switch with the Window menu or the Open Window List.')}
          {radio(
            'window-mode',
            view.windowMode === 'multiple',
            () => run(api.setWindowPreferences({ windowMode: 'multiple' })),
            'Multiple Windows',
            'Windows float inside LocalBooks, so you can keep an invoice, a register and a report side by side, move and resize them, or minimize them to the bottom.',
          )}
          <p className="px-2 pt-1 text-2xs text-qb-ink-faint">Also in the View menu, with Open Window List. Kept with this company, and used for companies with no choice of their own.</p>
        </fieldset>
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Desktop</legend>
          {radio('desktop', view.mode === 'save-when-closing', () => chooseDesktop('save-when-closing'), 'Save when closing company', 'The windows open when you close the company reopen next time.', !view.available)}
          {radio('desktop', false, () => chooseDesktop('save-current'), 'Save current desktop', 'Saves the windows open now; they reopen every time.', !view.available)}
          {radio('desktop', view.mode === 'dont-save', () => chooseDesktop('dont-save'), "Don't save the desktop", 'The company opens on the Home page alone.', !view.available)}
          {view.mode === 'keep-saved' &&
            radio('desktop', true, () => {}, 'Keep previously saved desktop', `${view.savedWindows} saved window${view.savedWindows === 1 ? '' : 's'} reopen every time.`)}
          <p className="px-2 pt-1 text-2xs text-qb-ink-faint">
            For you, on this company file and this computer. To open the company once without its windows, choose File › Open Company Without Restoring Windows…
            {IS_MAC ? ' Window › Workspace sets the same choice: AutoSave is Save when closing company, Manual keeps the saved desktop, and Save Workspace saves the windows open now.' : ''}
          </p>
        </fieldset>
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-2 border-t border-qb-border py-1">
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Left pane</legend>
          {radio('sidebar', view.sidebar === 'menu', () => run(api.setWindowPreferences({ sidebar: 'menu' })), 'Full menu', 'Every page, grouped as Sales, Expenses, Banking and so on.')}
          {radio('sidebar', view.sidebar === 'open-windows', () => run(api.setWindowPreferences({ sidebar: 'open-windows' })), 'Open windows only', 'Just the windows open now, as the Open Window List; open pages from the menu bar or search.')}
          {radio('sidebar', view.sidebar === 'hidden', () => run(api.setWindowPreferences({ sidebar: 'hidden' })), 'Hidden', 'More room for windows. View › Show or Hide Left Icon Bar brings the menu back.')}
        </fieldset>
        <fieldset>
          <legend className="px-2 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">Start on</legend>
          {radio('start-on', view.startOn === 'last-windows', () => run(api.setWindowPreferences({ startOn: 'last-windows' })), 'Last open windows', 'The desktop chosen under Desktop; the Home page when none is saved.')}
          {radio('start-on', view.startOn === 'home', () => run(api.setWindowPreferences({ startOn: 'home' })), 'Home page', 'The Home page alone.')}
          <label className="flex items-start gap-1.5 px-2 py-0.5 text-sm">
            <input
              type="radio"
              name="start-on"
              className="mt-0.5 accent-qb-green"
              checked={view.startOn === 'screen'}
              onChange={() => run(api.setWindowPreferences({ startOn: 'screen', startScreen: view.startScreen ?? 'sales/invoices' }))}
            />
            <span className="flex min-w-0 flex-wrap items-center gap-1">
              This screen:
              <select
                className="qb-input h-6 max-w-full py-0 text-sm"
                aria-label="Screen to start on"
                value={view.startScreen ?? 'sales/invoices'}
                onChange={(e) => run(api.setWindowPreferences({ startOn: 'screen', startScreen: e.target.value }))}
              >
                {SCREENS.map(([id, label]) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
            </span>
          </label>
          {radio('start-on', view.startOn === 'no-windows', () => run(api.setWindowPreferences({ startOn: 'no-windows' })), 'No windows', 'An empty desktop; choose a page to begin.')}
          <p className="px-2 pt-1 text-2xs text-qb-ink-faint">For this computer, from the next time a company opens. Holding {IS_MAC ? 'Option' : 'Alt'} while clicking Open still opens with no windows.</p>
        </fieldset>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
    </section>
  );
}
