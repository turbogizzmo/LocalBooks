import { createContext, useContext, useEffect, useState } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { NAV } from '../components/Sidebar.jsx';
import { askConfirm } from './dialogs.jsx';
import desktop from './api/desktop.js';

/** Sent by Window › New Window to the focused window, which opens the page it shows. */
export const NEW_WINDOW_REQUESTED = 'new-window-requested';
/** Sent by a menu bar item to the focused window with the page to open: `{ view, action, data }`. */
export const MENU_NAVIGATE = 'menu-navigate';
/** Sent by a menu bar item that acts on the page: 'find', 'help' or 'sidebar'. */
export const MENU_COMMAND = 'menu-command';
/** Sent to every window with the open windows whenever they change (see `windows.rs`). */
const OPEN_WINDOWS_CHANGED = 'open-windows-changed';
/** Sent to every window when View › One Window / Multiple Windows or Open Window List changes. */
const WINDOW_PREFERENCES_CHANGED = 'window-preferences-changed';

/** The window LocalBooks opens at launch; extra windows leave its remembered page alone. */
export function isMainWindow() {
  try {
    return getCurrentWebviewWindow().label === 'main';
  } catch {
    return true; // outside the app shell (a plain browser): behave as the only window
  }
}

/** The pages the app has (the sidebar's), which a saved desktop may reopen. */
export const KNOWN_VIEWS = NAV.flatMap((item) => (item.children ? item.children.map((c) => c.id) : [item.id]));

/** The page from the window's URL fragment (`#view=…&action=…&data=…`, see `windows.rs`), or null. */
function fragmentPage() {
  const params = new URLSearchParams(window.location.hash.slice(1));
  const view = params.get('view');
  if (!view) return null;
  let data = null;
  try {
    data = params.has('data') ? JSON.parse(params.get('data')) : null;
  } catch {
    data = null;
  }
  return { view, action: params.get('action'), data };
}

let restored = null;

/**
 * Asked once, before the app starts in this window: if the company has just opened, its saved
 * desktop (Edit › Preferences › Desktop View) reopens, this window on the first saved page and the
 * others beside it. Never in the way of opening: any failure means no desktop.
 */
export async function loadStartPage() {
  restored = await desktop.takeDesktop(KNOWN_VIEWS).catch(() => null);
}

/**
 * The page this window starts on: the saved desktop's when the company just reopened it, else the
 * one it was opened on (Window › New Window, or a form, register or report in its own window), else
 * null for the page last shown.
 */
export function startPage() {
  return restored ?? fragmentPage();
}

/**
 * Opens another window on the open company at `view`, with the request it should open (as the app's
 * own navigation passes it). Every window shares the company file and refreshes when any changes it.
 * With `reuse`, a window already showing that page comes forward instead.
 */
export function openWindow(view, action = null, data = null, { reuse = false } = {}) {
  return invoke('open_window', { view, action, data, reuse });
}

/** View › One Window / Multiple Windows and View › Open Window List, kept current in every window. */
export function useWindowPreferences() {
  const [prefs, setPrefs] = useState({ windowMode: 'multiple', showOpenWindowList: false });
  useEffect(() => {
    let cancelled = false;
    let unlisten;
    desktop.getDesktopView().then(
      (view) => !cancelled && setPrefs({ windowMode: view.windowMode, showOpenWindowList: view.showOpenWindowList }),
      () => {},
    );
    listen(WINDOW_PREFERENCES_CHANGED, ({ payload }) => setPrefs(payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  return prefs;
}

/** The open windows (`{ label, name, active }`), kept current. */
export function useOpenWindows() {
  const [windows, setWindows] = useState([]);
  useEffect(() => {
    let cancelled = false;
    let unlisten;
    desktop.listOpenWindows().then((list) => !cancelled && setWindows(list), () => {});
    listen(OPEN_WINDOWS_CHANGED, ({ payload }) => setWindows(payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  return windows;
}

/**
 * What a screen shows, for the window's title, the window lists and the saved desktop. App provides
 * it; see `useWindowContent`.
 */
export const WindowContentContext = createContext({ setContent: () => {}, windowMode: 'multiple' });

/**
 * A screen names what it shows while it shows it: `{ name, action, data }`, as in
 * `{ name: 'Invoice 1001 · Acme', action: 'edit', data: { invoiceId: 7 } }`. `name` titles the
 * window; `action` and `data`, when given, are what the window reopens with. Pass null for nothing
 * in particular (the window is then named after the page).
 */
export function useWindowContent(content) {
  const { setContent } = useContext(WindowContentContext);
  const key = content ? JSON.stringify(content) : null;
  useEffect(() => {
    setContent(key ? JSON.parse(key) : null);
    return () => setContent(null);
  }, [key, setContent]);
}

/**
 * Multiple Windows (View menu): a list opens a transaction or report in a window of its own, as
 * QuickBooks keeps forms side by side; One Window opens it in place. Returns a function that takes
 * the page (`view`, `action`, `data`) and the in-place fallback, and runs one or the other.
 */
export function useOpenFromList() {
  const { windowMode } = useContext(WindowContentContext);
  return (view, action, data, inPlace) => {
    if (windowMode !== 'multiple') return inPlace();
    return openWindow(view, action, data, { reuse: true }).catch(() => inPlace());
  };
}

/**
 * A transaction opened in a window of its own (or reopened with the desktop) arrives as an
 * `edit` intent with its id under `idKey`; once `rows` (the screen's list) have loaded, the row with
 * that id is handed to `open`. One no longer in the books calls `onMissing` instead.
 */
export function useEditIntent(intent, idKey, rows, open, onMissing) {
  const [pending, setPending] = useState(null);
  useEffect(() => {
    if (intent?.action === 'edit' && intent.data?.[idKey] != null) setPending(intent.data[idKey]);
  }, [intent, idKey]);
  useEffect(() => {
    if (pending === null || !rows) return;
    setPending(null);
    const row = rows.find((r) => r.id === pending);
    if (row) open(row);
    else onMissing();
    // Runs once per request, when the rows are there; `open` and `onMissing` are read then.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, rows]);
}

const unsaved = new Set();

/**
 * Marks a form as holding entries that are not saved yet: closing its window asks first. Each
 * form passes its own `id` and whether it has unsaved entries now.
 */
export function useUnsavedChanges(id, dirty) {
  useEffect(() => {
    if (dirty) unsaved.add(id);
    else unsaved.delete(id);
    return () => unsaved.delete(id);
  }, [id, dirty]);
}

/**
 * `useUnsavedChanges` for a form whose entries start as `values` when it mounts: it has unsaved
 * entries once they differ. Pass only what the person types (not defaults filled in later).
 */
export function useDirtySince(id, values) {
  const now = JSON.stringify(values);
  const [baseline] = useState(now);
  useUnsavedChanges(id, now !== baseline);
}

/**
 * Asks before the window closes while a form in it has unsaved entries (the window's close button,
 * Window › Close All). Returns the function that stops listening.
 */
export function guardWindowClose() {
  let unlisten;
  let cancelled = false;
  try {
    const win = getCurrentWebviewWindow();
    win
      .onCloseRequested(async (event) => {
        if (unsaved.size === 0) return;
        event.preventDefault();
        if (await askConfirm('This window has entries that are not saved. Close it and lose them?', { confirmLabel: 'Close without saving', danger: true })) {
          unsaved.clear();
          await win.destroy();
        }
      })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
  } catch {
    // Outside the app shell there is no window to guard.
  }
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
