import { api } from '../lib/api.js';
import { useOpenWindows } from '../lib/windows.js';

/**
 * View › Open Window List, as in QuickBooks Desktop: every open LocalBooks window (forms, registers,
 * reports), the active one highlighted; choosing one brings it forward. Shown beside the sidebar in
 * every window while it is turned on.
 */
export default function OpenWindowList() {
  const windows = useOpenWindows();
  return (
    <nav className="no-print flex w-[190px] flex-none flex-col border-l border-white/10 bg-qb-nav text-sm text-qb-nav-text" aria-label="Open Windows">
      <div className="flex h-toolbar flex-none items-center gap-2 border-b border-white/10 px-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-white">Open Windows</span>
        <button
          type="button"
          className="ml-auto rounded-xs px-1 text-xs hover:bg-white/10 hover:text-white"
          title="Hide the Open Window List (View › Open Window List)"
          aria-label="Hide the Open Window List"
          onClick={() => api.setWindowPreferences({ showOpenWindowList: false }).catch(() => {})}
        >
          ✕
        </button>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto py-1 scrollbar-thin">
        {windows.map((w) => (
          <li key={w.label}>
            <button
              type="button"
              className={`qb-nav-item h-row w-full truncate text-left text-sm ${w.active ? 'bg-white/15 font-semibold text-white' : ''}`}
              aria-current={w.active ? 'true' : undefined}
              title={w.name}
              onClick={() => api.focusOpenWindow(w.label).catch(() => {})}
            >
              <span className="truncate">{w.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </nav>
  );
}
