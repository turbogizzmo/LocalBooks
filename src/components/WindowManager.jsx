import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { Copy, Minus, Square, X } from 'lucide-react';
import { findNav, resolveView } from './Sidebar.jsx';
import { api } from '../lib/api.js';
import * as cw from '../lib/childWindows.js';
import { ChildContext, confirmClose, forgetUnsaved, WINDOW_COMMAND } from '../lib/windows.js';

/**
 * Windows inside the LocalBooks window, the way QuickBooks Desktop for Windows shows forms,
 * registers and reports: each in a window of its own within the program's window (not a separate
 * program window), with a title bar, Minimize, Maximize/Restore and Close. Minimized windows wait
 * along the bottom of the work area; a maximized window fills it and its buttons move up to the
 * toolbar. View › One Window shows one window at a time, filling the work area; View › Multiple
 * Windows lets them float side by side. Ctrl+F6 or Ctrl+Tab moves to the next window, Esc or
 * Ctrl+F4 closes the active one (⌘W on a Mac).
 */

/** The row the minimized windows take along the bottom of the work area. */
const DOCK_HEIGHT = 34;

/** The work area's size before it is measured: the LocalBooks window less the sidebar and toolbar. */
const guessArea = () => ({ x: 0, y: 0, width: Math.max(window.innerWidth - 220, 480), height: Math.max(window.innerHeight - 48, 320) });

/** What a window is called: what its screen names, else its page's name. */
export function windowName(w) {
  return w.content?.name || findNav(w.view)?.item.label || 'LocalBooks';
}

/**
 * The windows inside this LocalBooks window: their state, and the commands on them (open, focus,
 * close, minimize, maximize, move, Close All, Cascade, Tile, Arrange Icons, next window).
 * `initial(area)` gives the windows to start with.
 */
export function useChildWindows(windowMode, initial) {
  const areaRef = useRef(null);
  const area = useCallback(() => {
    const r = areaRef.current?.getBoundingClientRect();
    return r && r.width > 0 ? { x: 0, y: 0, width: r.width, height: r.height } : guessArea();
  }, []);
  const [state, setState] = useState(() => initial(guessArea()));
  const stateRef = useRef(state);
  stateRef.current = state;

  const open = useCallback(
    (view, action = null, data = null, { forceNew = false } = {}) =>
      setState((s) => {
        // As in Windows programs: while the active window is maximized, a new one opens maximized too.
        const active = s.windows.find((w) => w.id === s.activeId);
        // `settings` with an action is that Settings tab; a moved page opens where it is now.
        return cw.open(s, { view: resolveView(view, action), action, data }, { area: area(), maximized: s.windows.length === 0 || !!active?.maximized, forceNew });
      }),
    [area],
  );
  const focus = useCallback((id) => setState((s) => cw.focus(s, id)), []);
  const close = useCallback(async (id) => {
    const w = stateRef.current.windows.find((x) => x.id === id);
    if (!w || !(await confirmClose(id, windowName(w)))) return false;
    forgetUnsaved(id);
    setState((s) => cw.close(s, id));
    return true;
  }, []);
  const closeAll = useCallback(async () => {
    // Front to back, so each window asks about its own unsaved entries in turn.
    for (const id of [...stateRef.current.order].reverse()) {
      setState((s) => cw.focus(s, id));
      if (!(await close(id))) return;
    }
  }, [close]);
  const minimize = useCallback((id) => setState((s) => cw.minimize(s, id)), []);
  const toggleMaximize = useCallback((id) => setState((s) => cw.toggleMaximize(s, id)), []);
  const setBounds = useCallback((id, bounds) => setState((s) => cw.setBounds(s, id, bounds, area())), [area]);
  const setContent = useCallback((id, content) => setState((s) => cw.setContent(s, id, content)), []);
  const cycle = useCallback((back = false) => setState((s) => cw.cycle(s, back)), []);
  const arrange = useCallback(
    async (kind) => {
      if (kind === 'arrange-icons') return setState((s) => cw.arrangeIcons(s));
      const count = stateRef.current.windows.filter((w) => !w.minimized).length;
      // Arranged above the minimized windows, as Windows leaves their row free.
      const a = area();
      const docked = stateRef.current.windows.some((w) => w.minimized) ? DOCK_HEIGHT : 0;
      const layout = await api.arrangeWindows(kind, count, { ...a, height: Math.max(a.height - docked, cw.MIN_HEIGHT) }).catch(() => null);
      if (layout) setState((s) => cw.arrange(s, layout));
    },
    [area],
  );

  // The work area changed size (the LocalBooks window was resized, the sidebar hidden): every window
  // stays reachable inside it.
  useEffect(() => {
    const el = areaRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(() => {
      const a = area();
      setState((s) => {
        let changed = false;
        const windows = s.windows.map((w) => {
          const b = cw.clamp(w.bounds, a);
          if (JSON.stringify(b) === JSON.stringify(w.bounds)) return w;
          changed = true;
          return { ...w, bounds: b };
        });
        return changed ? { ...s, windows } : s;
      });
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [area]);

  // The Window menu's commands and its list of windows (see windows.rs).
  useEffect(() => {
    let unlisten;
    let cancelled = false;
    try {
      getCurrentWebviewWindow()
        .listen(WINDOW_COMMAND, ({ payload }) => {
          const active = stateRef.current.activeId;
          if (payload === 'close-all') closeAll();
          else if (payload === 'close') {
            if (active) close(active);
            else getCurrentWebviewWindow().close().catch(() => {});
          } else if (typeof payload === 'string' && payload.startsWith('focus:')) focus(payload.slice(6));
          else arrange(payload);
        })
        .then((fn) => (cancelled ? fn() : (unlisten = fn)))
        .catch(() => {});
    } catch {
      // Outside the app shell there is no menu bar.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [arrange, close, closeAll, focus]);

  // QuickBooks' keys: Ctrl+F6 or Ctrl+Tab for the next window (with Shift, the one before), Esc or
  // Ctrl+F4 to close the active one. Esc is left to anything open on top (a menu, a dialog, a list
  // of suggestions) and to fields being typed in.
  useEffect(() => {
    const onKey = (e) => {
      if (e.ctrlKey && (e.key === 'F6' || e.key === 'Tab')) {
        e.preventDefault();
        cycle(e.shiftKey);
        return;
      }
      const active = stateRef.current.activeId;
      if (!active) return;
      if (e.ctrlKey && e.key === 'F4') {
        e.preventDefault();
        close(active);
        return;
      }
      if (e.key !== 'Escape' || e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target;
      if (t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))) return;
      if (document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [aria-modal="true"]')) return;
      close(active);
    };
    // On the window, so a menu or dialog listening on the document handles Esc first.
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [close, cycle]);

  // Each change is reported, so the LocalBooks window is titled after the active window, the Window
  // menu lists them, and they are what the desktop saves.
  useEffect(() => {
    const timer = setTimeout(() => {
      const children = state.windows.map((w) => ({
        id: w.id,
        name: windowName(w),
        page: cw.currentPage(w),
        bounds: w.bounds,
        minimized: w.minimized,
        maximized: w.maximized || windowMode === 'one',
      }));
      api.reportChildWindows(children, state.activeId).catch(() => {});
    }, 120);
    return () => clearTimeout(timer);
  }, [state, windowMode]);

  return { state, areaRef, area, open, focus, close, closeAll, minimize, toggleMaximize, setBounds, setContent, cycle, arrange };
}

/**
 * The work area: every window inside LocalBooks, drawn by `renderScreen(window)`, with the minimized
 * ones docked along the bottom and `background` shown where no window covers it.
 */
export default function WindowManager({ manager, windowMode, renderScreen, background }) {
  const { state, areaRef } = manager;
  const one = windowMode === 'one';
  const z = useMemo(() => new Map(state.order.map((id, i) => [id, i + 1])), [state.order]);
  const dock = cw.docked(state);
  // One context object per window, kept for its life, so moving one window re-renders no screen's hooks.
  const contexts = useRef(new Map());
  const childContext = (id) => {
    if (!contexts.current.has(id)) contexts.current.set(id, { id, setContent: manager.setContent });
    return contexts.current.get(id);
  };

  return (
    <div ref={areaRef} className="relative min-h-0 flex-1 overflow-hidden bg-qb-desktop" aria-label="Open windows">
      {state.windows.every((w) => w.minimized) && background}
      {state.windows.map((w) => {
        const hidden = w.minimized || (one && w.id !== state.activeId);
        return (
          <ChildFrame
            key={w.id}
            win={w}
            name={windowName(w)}
            active={w.id === state.activeId}
            maximized={one || w.maximized}
            hidden={hidden}
            z={z.get(w.id) ?? 1}
            manager={manager}
          >
            <ChildContext.Provider value={childContext(w.id)}>{renderScreen(w)}</ChildContext.Provider>
          </ChildFrame>
        );
      })}
      {dock.length > 0 && !one && (
        <div className="no-print pointer-events-none absolute bottom-0 left-0 right-0 z-[900] flex flex-wrap-reverse gap-1 p-1">
          {dock.map((w) => (
            <DockedWindow key={w.id} name={windowName(w)} onRestore={() => manager.focus(w.id)} onMaximize={() => manager.toggleMaximize(w.id)} onClose={() => manager.close(w.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

const RESIZE_EDGES = [
  ['n', 'top-0 left-2 right-2 h-1 cursor-ns-resize'],
  ['s', 'bottom-0 left-2 right-2 h-1 cursor-ns-resize'],
  ['w', 'left-0 top-2 bottom-2 w-1 cursor-ew-resize'],
  ['e', 'right-0 top-2 bottom-2 w-1 cursor-ew-resize'],
  ['nw', 'left-0 top-0 h-2 w-2 cursor-nwse-resize'],
  ['ne', 'right-0 top-0 h-2 w-2 cursor-nesw-resize'],
  ['sw', 'left-0 bottom-0 h-2 w-2 cursor-nesw-resize'],
  ['se', 'right-0 bottom-0 h-2 w-2 cursor-nwse-resize'],
];

/** `start` moved by (dx, dy) along `edges` ("move", or any of n, s, e, w). */
function dragged(start, edges, dx, dy) {
  if (edges === 'move') return { ...start, x: start.x + dx, y: start.y + dy };
  let { x, y, width, height } = start;
  if (edges.includes('e')) width = Math.max(start.width + dx, cw.MIN_WIDTH);
  if (edges.includes('s')) height = Math.max(start.height + dy, cw.MIN_HEIGHT);
  if (edges.includes('w')) {
    width = Math.max(start.width - dx, cw.MIN_WIDTH);
    x = start.x + start.width - width;
  }
  if (edges.includes('n')) {
    height = Math.max(start.height - dy, cw.MIN_HEIGHT);
    y = start.y + start.height - height;
  }
  return { x, y, width, height };
}

/** One window inside LocalBooks: its frame, title bar and buttons around a screen. */
function ChildFrame({ win, name, active, maximized, hidden, z, manager, children }) {
  // While a title bar or edge is dragged, the window follows the pointer; the place is kept on release.
  const [draft, setDraft] = useState(null);
  const drag = useRef(null);
  const bounds = draft ?? win.bounds;

  const begin = (edges) => (e) => {
    if (e.button !== 0 || (edges === 'move' && e.target.closest('button'))) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { edges, x: e.clientX, y: e.clientY, start: win.bounds };
  };
  const move = (e) => {
    const d = drag.current;
    if (!d) return;
    setDraft(cw.clamp(dragged(d.start, d.edges, e.clientX - d.x, e.clientY - d.y), manager.area()));
  };
  const end = () => {
    if (drag.current && draft) manager.setBounds(win.id, draft);
    drag.current = null;
    setDraft(null);
  };

  const style = maximized ? { zIndex: z } : { zIndex: z, left: bounds.x, top: bounds.y, width: bounds.width, height: bounds.height };
  return (
    <section
      className={`absolute flex flex-col overflow-hidden bg-qb-surface ${maximized ? 'inset-0' : `rounded-xs border shadow-popover ${active ? 'border-qb-nav' : 'border-qb-border-strong'}`} ${hidden ? 'hidden' : ''}`}
      style={style}
      aria-label={name}
      aria-hidden={hidden || undefined}
      onPointerDownCapture={() => !active && manager.focus(win.id)}
      onFocusCapture={() => !active && manager.focus(win.id)}
    >
      {!maximized && (
        <div
          className={`no-print flex h-[26px] flex-none cursor-default select-none items-center gap-2 pl-2 ${active ? 'bg-qb-nav text-white' : 'bg-qb-row-header text-qb-ink-muted'}`}
          onPointerDown={begin('move')}
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={end}
          onDoubleClick={(e) => !e.target.closest('button') && manager.toggleMaximize(win.id)}
        >
          <span className="min-w-0 flex-1 truncate text-sm font-semibold">{name}</span>
          <WindowButtons name={name} onMinimize={() => manager.minimize(win.id)} onMaximize={() => manager.toggleMaximize(win.id)} onClose={() => manager.close(win.id)} light={active} />
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">{children}</div>
      {!maximized &&
        RESIZE_EDGES.map(([edges, place]) => (
          <div key={edges} className={`no-print absolute ${place}`} onPointerDown={begin(edges)} onPointerMove={move} onPointerUp={end} onPointerCancel={end} aria-hidden="true" />
        ))}
    </section>
  );
}

/** Minimize, Maximize (or Restore) and Close, as on a window's title bar. */
export function WindowButtons({ name, onMinimize, onMaximize, onClose, maximized = false, light = false }) {
  const cls = `grid h-[22px] w-[28px] place-items-center rounded-xs ${light ? 'hover:bg-white/15' : 'hover:bg-qb-row-hover'}`;
  return (
    <div className="flex flex-none items-center pr-0.5">
      {onMinimize && (
        <button type="button" className={cls} onClick={onMinimize} title="Minimize" aria-label={`Minimize ${name}`}>
          <Minus size={13} aria-hidden="true" />
        </button>
      )}
      {onMaximize && (
        <button type="button" className={cls} onClick={onMaximize} title={maximized ? 'Restore Down' : 'Maximize'} aria-label={`${maximized ? 'Restore' : 'Maximize'} ${name}`}>
          {maximized ? <Copy size={12} aria-hidden="true" /> : <Square size={11} aria-hidden="true" />}
        </button>
      )}
      <button type="button" className={`${cls} ${light ? 'hover:bg-qb-negative' : 'hover:bg-qb-negative hover:text-white'}`} onClick={onClose} title="Close (Esc)" aria-label={`Close ${name}`}>
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}

/** A minimized window, docked along the bottom of the work area. */
function DockedWindow({ name, onRestore, onMaximize, onClose }) {
  return (
    <div className="pointer-events-auto flex h-[26px] w-[200px] items-center gap-1 rounded-xs border border-qb-border-strong bg-qb-row-header pl-2 text-qb-ink shadow-popover" onDoubleClick={onRestore}>
      <button type="button" className="min-w-0 flex-1 truncate text-left text-sm font-semibold" onClick={onRestore} title={`Restore ${name}`}>
        {name}
      </button>
      <WindowButtons name={name} onMaximize={onMaximize} onClose={onClose} />
    </div>
  );
}
