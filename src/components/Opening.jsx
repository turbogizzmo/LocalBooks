import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api } from '../lib/api.js';
import { COMPANY_OPENING, openingStep, openingTitle } from '../lib/opening.js';

/** Which file is opening and the step it is on, with a spinner. */
function OpeningPanel({ state }) {
  return (
    <section className="qb-panel w-full max-w-[460px]" role="status" aria-live="polite" data-opening>
      <div className="flex items-center gap-3 px-4 py-4">
        <span aria-hidden="true" className="h-6 w-6 flex-none animate-spin rounded-full border-2 border-qb-border border-t-qb-green" />
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold">{openingTitle(state)}</h1>
          <p className="text-sm text-qb-ink-muted">{openingStep(state)}</p>
        </div>
      </div>
    </section>
  );
}

/**
 * At launch, while the company file opens (gap R28): shown until the open ends, then `onOpened`
 * starts the window as usual (the failure screen, sign-in or the app), in place.
 */
export function OpeningScreen({ initial, onOpened }) {
  const [state, setState] = useState(initial);
  useEffect(() => {
    let stop = null;
    let gone = false;
    // Once only: the end can arrive both as the event and from the check below. No reload: a page
    // reloading under a WebDriver command leaves WebKit's driver waiting on it for good (Linux E2E).
    let done = false;
    const reload = () => {
      if (done) return;
      done = true;
      onOpened();
    };
    listen(COMPANY_OPENING, (event) => {
      if (event.payload) setState(event.payload);
      else reload();
    }).then(async (unlisten) => {
      if (gone) return unlisten();
      stop = unlisten;
      // The open may have ended before the listener was in place.
      if (!(await api.getOpeningState().catch(() => null))) reload();
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, [onOpened]);
  return (
    <div className="flex h-full items-center justify-center bg-qb-canvas p-4">
      <OpeningPanel state={state} />
    </div>
  );
}

/**
 * Opens from inside the app (File › Open, the recent list, a second launch's file, Restore): a panel
 * over the screen while one runs, which keeps clicks and keys off the company being closed. The window
 * reloads once the new file is open; if the open fails, the panel goes and the screen says why.
 */
export function OpeningOverlay() {
  const [state, setState] = useState(null);
  useEffect(() => {
    let stop = null;
    let gone = false;
    listen(COMPANY_OPENING, (event) => setState(event.payload ?? null)).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);
  // Keys don't reach the screen behind either (no Save into the company being closed).
  useEffect(() => {
    if (!state) return undefined;
    const swallow = (event) => {
      event.preventDefault();
      event.stopPropagation();
    };
    window.addEventListener('keydown', swallow, true);
    return () => window.removeEventListener('keydown', swallow, true);
  }, [state]);
  if (!state) return null;
  return (
    <div className="no-print fixed inset-0 z-[10000] flex items-center justify-center bg-black/20 p-4">
      <OpeningPanel state={state} />
    </div>
  );
}
