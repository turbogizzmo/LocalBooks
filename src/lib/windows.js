import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';

/** Sent by Window › New Window to the focused window, which opens the page it shows. */
export const NEW_WINDOW_REQUESTED = 'new-window-requested';
/** Sent by a menu bar item to the focused window with the page to open: `{ view, action, data }`. */
export const MENU_NAVIGATE = 'menu-navigate';
/** Sent by a menu bar item that acts on the page: 'find', 'help' or 'sidebar'. */
export const MENU_COMMAND = 'menu-command';

/** The window LocalBooks opens at launch; extra windows leave its remembered page alone. */
export function isMainWindow() {
  try {
    return getCurrentWebviewWindow().label === 'main';
  } catch {
    return true; // outside the app shell (a plain browser): behave as the only window
  }
}

/**
 * The page this window was opened on (`#view=…&action=…&data=…`, see `windows.rs`), or null for a
 * window opened without one.
 */
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

/**
 * Opens another window on the open company at `view`, with the request it should open (as the app's
 * own navigation passes it). Every window shares the company file and refreshes when any changes it.
 */
export function openWindow(view, action = null, data = null) {
  return invoke('open_window', { view, action, data });
}
