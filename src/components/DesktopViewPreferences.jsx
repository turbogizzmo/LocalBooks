import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import Notice from './Notice.jsx';
import { api, errorText } from '../lib/api.js';

const IS_MAC = /Mac/.test(navigator.userAgent);

/**
 * Edit › Preferences › Desktop View, the My Preferences part, as in QuickBooks Desktop: View (One
 * Window or Multiple Windows) for this computer, and Desktop (what reopens with this company) for
 * you on this company file. Each choice takes effect at once.
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
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
    </section>
  );
}
