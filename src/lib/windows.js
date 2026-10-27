import { createContext, useContext, useEffect, useState } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { NAV } from '../components/Sidebar.jsx';
import { askConfirm } from './dialogs.jsx';
import desktop from './api/desktop.js';

/** Sent by Window › New Window to the focused LocalBooks window, which opens the page active in it. */
export const NEW_WINDOW_REQUESTED = 'new-window-requested';
/** Sent by a menu bar item to the focused LocalBooks window with the page to open: `{ view, action, data }`. */
export const MENU_NAVIGATE = 'menu-navigate';
/** Sent by a menu bar item that acts on the page: 'find', 'help' or 'sidebar'. */
export const MENU_COMMAND = 'menu-command';
/** Sent by the Window menu: 'close-all', 'close', 'cascade', 'tile-vertically', 'tile-horizontally', 'arrange-icons' or 'focus:<id>'. */
export const WINDOW_COMMAND = 'window-command';
/** Sent to every LocalBooks window when View › One Window / Multiple Windows or Open Window List changes. */
const WINDOW_PREFERENCES_CHANGED = 'window-preferences-changed';

/** The window LocalBooks opens at launch. */
export function isMainWindow() {
  try {
    return getCurrentWebviewWindow().label === 'main';
  } catch {
    return true; // outside the app shell (a plain browser): behave as the only window
  }
}

/** The pages the app has (the sidebar's), which a saved desktop may reopen. */
export const KNOWN_VIEWS = NAV.flatMap((item) => (item.children ? item.children.map((c) => c.id) : [item.id]));

/** The page from the LocalBooks window's URL fragment (`#view=…&action=…&data=…`, see `windows.rs`), or null. */
export function startPage() {
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
let preferences = { windowMode: 'multiple', showOpenWindowList: false };

/**
 * Asked once, before the app starts in this LocalBooks window: if the company has just opened, its
 * saved desktop (Edit › Preferences › Desktop View) and the View mode. Never in the way of opening:
 * any failure means no desktop.
 */
export async function loadStartPage() {
  const [taken, view] = await Promise.all([desktop.takeDesktop(KNOWN_VIEWS).catch(() => null), desktop.getDesktopView().catch(() => null)]);
  restored = taken;
  if (view) preferences = { windowMode: view.windowMode, showOpenWindowList: view.showOpenWindowList };
}

/** The saved desktop to reopen (`{ windows, homeOnly }`), or null. */
export function restoredDesktop() {
  return restored;
}

/**
 * Window › New Window: another LocalBooks window on the open company at `view`, with the request it
 * should open. Every LocalBooks window shares the company file and refreshes when any changes it.
 */
export function openWindow(view, action = null, data = null) {
  return invoke('open_window', { view, action, data });
}

/** View › One Window / Multiple Windows and View › Open Window List, kept current in every LocalBooks window. */
export function useWindowPreferences() {
  const [prefs, setPrefs] = useState(preferences);
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

/**
 * The windows inside this LocalBooks window (`components/WindowManager.jsx`): `windows`, `activeId`,
 * `open(view, action, data, { forceNew })`, `focus(id)`, `close(id)`, `windowMode`.
 */
export const WindowManagerContext = createContext({ windows: [], activeId: null, open: () => {}, focus: () => {}, close: () => {}, windowMode: 'multiple' });

/** The window inside LocalBooks a screen is showing in: `{ id, setContent }`. */
export const ChildContext = createContext({ id: 'page', setContent: () => {} });

/**
 * A screen names what it shows while it shows it: `{ name, action, data }`, as in
 * `{ name: 'Invoice 1001 · Acme', action: 'edit', data: { invoiceId: 7 } }`. `name` titles its
 * window; `action` and `data`, when given, are what the window reopens with. Pass null for nothing
 * in particular (the window is then named after the page).
 */
export function useWindowContent(content) {
  const { id, setContent } = useContext(ChildContext);
  const key = content ? JSON.stringify(content) : null;
  useEffect(() => {
    setContent(id, key ? JSON.parse(key) : null);
  }, [id, key, setContent]);
}

/**
 * A list opens a transaction or report in a window of its own, as QuickBooks keeps forms in their
 * own windows (or brings forward the window that already shows it). Returns `(view, action, data)`.
 */
export function useOpenFromList() {
  const { open } = useContext(WindowManagerContext);
  return (view, action, data) => open(view, action, data);
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

/** Unsaved form entries, by the window they are in: window id → the forms' own ids. */
const unsaved = new Map();

/** Whether the window `windowId` (or, without one, any window) has a form with entries not saved. */
export function hasUnsaved(windowId = null) {
  if (windowId === null) return [...unsaved.values()].some((forms) => forms.size > 0);
  return (unsaved.get(windowId)?.size ?? 0) > 0;
}

/** Forgets the unsaved entries of a window being closed. */
export function forgetUnsaved(windowId) {
  unsaved.delete(windowId);
}

/**
 * Marks a form as holding entries that are not saved yet: closing its window asks first. Each
 * form passes its own `id` and whether it has unsaved entries now.
 */
export function useUnsavedChanges(id, dirty) {
  const { id: windowId } = useContext(ChildContext);
  useEffect(() => {
    const forms = unsaved.get(windowId) ?? new Set();
    if (dirty) forms.add(id);
    else forms.delete(id);
    unsaved.set(windowId, forms);
    return () => unsaved.get(windowId)?.delete(id);
  }, [windowId, id, dirty]);
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

/** Asks before a window with unsaved form entries closes; resolves whether to close it. */
export async function confirmClose(windowId, name) {
  if (!hasUnsaved(windowId)) return true;
  return askConfirm(`${name} has entries that are not saved. Close it and lose them?`, { confirmLabel: 'Close without saving', danger: true });
}

/**
 * Asks before this LocalBooks window closes while a form in any window inside it has unsaved
 * entries. Returns the function that stops listening.
 */
export function guardWindowClose() {
  let unlisten;
  let cancelled = false;
  try {
    const win = getCurrentWebviewWindow();
    win
      .onCloseRequested(async (event) => {
        if (!hasUnsaved()) return;
        event.preventDefault();
        if (await askConfirm('A window in LocalBooks has entries that are not saved. Close LocalBooks and lose them?', { confirmLabel: 'Close without saving', danger: true })) {
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
