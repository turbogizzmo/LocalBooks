/**
 * The windows inside a LocalBooks window, as QuickBooks Desktop for Windows keeps forms, registers
 * and reports inside its own window. Pure state changes, used by `components/WindowManager.jsx`.
 *
 * state = { windows: [...] in the order they opened, order: [ids] bottom to top, activeId, next }
 * window = { id, view, intent: { action, data, nonce } | null, content: { name, action, data } | null,
 *            bounds: { x, y, width, height } (in the work area), minimized, maximized, minimizedAt }
 */

/** The smallest a window inside LocalBooks can be made. */
export const MIN_WIDTH = 320;
export const MIN_HEIGHT = 180;
/** How much of a window's title bar stays in the work area, so it can always be dragged back. */
const GRIP = 80;
/** How far each new window steps from the last, so every title bar shows (as Cascade does). */
const STEP = 28;

export const EMPTY = { windows: [], order: [], activeId: null, next: 1 };

/** The page a window shows now: what its screen names (an invoice being edited, a report), else what it opened with. */
export function currentPage(w) {
  if (w.content?.action) return { view: w.view, action: w.content.action, data: w.content.data ?? null };
  return { view: w.view, action: w.intent?.action ?? null, data: w.intent?.data ?? null };
}

/**
 * What makes two requests the same window: a transaction (`edit` with its id), a report, or an
 * account's register each get a window of their own; any other page has one window, which a new
 * request (say, Create Invoices) reuses.
 */
export function pageKey({ view, action, data }) {
  if (action === 'edit') return `${view}|edit|${JSON.stringify(data ?? null)}`;
  if (view === 'reports' && data?.report) return `reports|${data.report}`;
  if (view === 'accounting/register' && data?.accountId != null) return `register|${data.accountId}`;
  // Settings is one window, as QuickBooks' Preferences is one dialog: its tabs switch inside it.
  if (view.startsWith('settings/')) return 'settings';
  return view;
}

/** `bounds` kept inside the work area `area`: no larger than it, and with the title bar reachable. */
export function clamp(bounds, area) {
  const width = Math.max(Math.min(bounds.width, area.width), Math.min(MIN_WIDTH, area.width));
  const height = Math.max(Math.min(bounds.height, area.height), Math.min(MIN_HEIGHT, area.height));
  const x = Math.min(Math.max(bounds.x, GRIP - width), Math.max(area.width - GRIP, 0));
  const y = Math.min(Math.max(bounds.y, 0), Math.max(area.height - 28, 0));
  return { x: Math.round(x), y: Math.round(y), width: Math.round(width), height: Math.round(height) };
}

/** Where a new window opens: a step down and right of the last one, starting again at the top-left corner. */
function nextBounds(state, area) {
  const width = Math.min(Math.max(Math.round(area.width * 0.72), 640), area.width);
  const height = Math.min(Math.max(Math.round(area.height * 0.82), 420), area.height);
  const floating = state.windows.filter((w) => !w.minimized && !w.maximized).length;
  const room = Math.max(Math.floor(Math.min(area.width - width, area.height - height) / STEP), 0) + 1;
  const step = (floating % room) * STEP;
  return clamp({ x: step, y: step, width, height }, area);
}

const topVisible = (state, except = null) => [...state.order].reverse().find((id) => id !== except && !state.windows.find((w) => w.id === id)?.minimized) ?? null;

/** Brings window `id` to the front, restored from the dock if it was minimized. */
export function focus(state, id) {
  if (!state.windows.some((w) => w.id === id)) return state;
  return {
    ...state,
    windows: state.windows.map((w) => (w.id === id && w.minimized ? { ...w, minimized: false } : w)),
    order: [...state.order.filter((o) => o !== id), id],
    activeId: id,
  };
}

/**
 * Opens a page: brings forward the window already showing it (handing it the request, if any),
 * else opens a new window for it on top. `forceNew` always opens another. Returns the state.
 */
export function open(state, { view, action = null, data = null }, { area, maximized = false, forceNew = false }) {
  const key = pageKey({ view, action, data });
  const existing = forceNew ? null : state.windows.find((w) => pageKey(currentPage(w)) === key);
  const intent = action ? { action, data, nonce: Date.now() + Math.random() } : null;
  if (existing) {
    const focused = focus(state, existing.id);
    // The same window on another of its pages (another Settings tab) shows that page.
    const moved = existing.view !== view;
    if (!intent && !moved) return focused;
    return { ...focused, windows: focused.windows.map((w) => (w.id === existing.id ? { ...w, view, content: moved ? null : w.content, intent: intent ?? w.intent } : w)) };
  }
  const id = `w${state.next}`;
  const win = { id, view, intent, content: null, bounds: nextBounds(state, area), minimized: false, maximized, minimizedAt: 0 };
  return { windows: [...state.windows, win], order: [...state.order, id], activeId: id, next: state.next + 1 };
}

/** Closes window `id`; the window below it becomes active. */
export function close(state, id) {
  const next = { ...state, windows: state.windows.filter((w) => w.id !== id), order: state.order.filter((o) => o !== id) };
  return { ...next, activeId: state.activeId === id ? topVisible(next) : state.activeId };
}

/** Minimizes window `id` to the dock at the bottom of the work area. */
export function minimize(state, id) {
  const windows = state.windows.map((w) => (w.id === id ? { ...w, minimized: true, minimizedAt: Date.now() } : w));
  return { ...state, windows, activeId: state.activeId === id ? topVisible({ ...state, windows }, id) : state.activeId };
}

/** Maximizes window `id` to fill the work area, or restores it to where it was. */
export function toggleMaximize(state, id) {
  const next = focus(state, id);
  return { ...next, windows: next.windows.map((w) => (w.id === id ? { ...w, maximized: !w.maximized, minimized: false } : w)) };
}

/** Moves or resizes window `id`. */
export function setBounds(state, id, bounds, area) {
  return { ...state, windows: state.windows.map((w) => (w.id === id ? { ...w, bounds: clamp(bounds, area), maximized: false } : w)) };
}

/** A window's screen names what it shows (for its title, the window lists and the saved desktop). */
export function setContent(state, id, content) {
  if (!state.windows.some((w) => w.id === id && JSON.stringify(w.content) !== JSON.stringify(content))) return state;
  return { ...state, windows: state.windows.map((w) => (w.id === id ? { ...w, content } : w)) };
}

/** Ctrl+F6 / Ctrl+Tab: the next window in the order they opened (`back` for the one before) comes forward. */
export function cycle(state, back = false) {
  if (state.windows.length === 0) return state;
  const at = state.windows.findIndex((w) => w.id === state.activeId);
  const n = state.windows.length;
  const next = state.windows[(((at < 0 ? 0 : at) + (back ? -1 : 1)) % n + n) % n];
  return focus(state, next.id);
}

/** Window › Cascade and Tile: the windows not minimized take `layout` (one place each, bottom to top). */
export function arrange(state, layout) {
  const ids = state.order.filter((id) => !state.windows.find((w) => w.id === id)?.minimized);
  const place = new Map(ids.map((id, i) => [id, layout[i]]));
  return { ...state, windows: state.windows.map((w) => (place.get(w.id) ? { ...w, bounds: roundBounds(place.get(w.id)), maximized: false } : w)) };
}

/** Window › Arrange Icons: the minimized windows line up along the bottom in the order they opened. */
export function arrangeIcons(state) {
  let i = 0;
  return { ...state, windows: state.windows.map((w) => (w.minimized ? { ...w, minimizedAt: ++i } : w)) };
}

const roundBounds = (b) => ({ x: Math.round(b.x), y: Math.round(b.y), width: Math.round(b.width), height: Math.round(b.height) });

/** The minimized windows, in their place along the dock. */
export function docked(state) {
  return state.windows.filter((w) => w.minimized).sort((a, b) => a.minimizedAt - b.minimizedAt);
}

/** The windows of a saved desktop (see `desktop.rs`), the last one active. */
export function fromSaved(saved, area) {
  let state = EMPTY;
  saved.forEach((s, i) => {
    state = open(state, s.page, { area, maximized: s.maximized, forceNew: true });
    const id = state.activeId;
    state = {
      ...state,
      windows: state.windows.map((w) => (w.id === id ? { ...w, bounds: s.bounds ? clamp(s.bounds, area) : w.bounds, minimized: !!s.minimized, minimizedAt: s.minimized ? i + 1 : 0 } : w)),
    };
  });
  return { ...state, activeId: topVisible(state) };
}
