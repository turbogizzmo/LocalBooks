import { useContext } from 'react';
import { api } from '../lib/api.js';
import { WindowManagerContext } from '../lib/windows.js';
import { windowName } from './WindowManager.jsx';

/**
 * View › Open Window List, as in QuickBooks Desktop: the windows open inside LocalBooks (forms,
 * registers, reports), in the left-hand pane beside the icon bar, the active one highlighted;
 * choosing one brings it forward (restoring it if it was minimized). `asSidebar`: the left pane
 * shows only this list (Edit › Preferences › Desktop View › Left pane), under the company's name.
 */
export default function OpenWindowList({ asSidebar = false, companyName = 'LocalBooks' }) {
  const { windows, activeId, focus } = useContext(WindowManagerContext);
  return (
    <nav className={`no-print flex w-[190px] flex-none flex-col bg-qb-nav text-sm text-qb-nav-text ${asSidebar ? '' : 'border-l border-white/10'}`} aria-label="Open Windows">
      {asSidebar && (
        <div className="flex h-toolbar flex-none items-center gap-2 border-b border-white/10 px-3">
          <span className="grid h-5 w-5 flex-none place-items-center rounded-xs bg-qb-green text-2xs font-bold text-white">LB</span>
          <span className="truncate font-semibold text-white">{companyName}</span>
        </div>
      )}
      <div className="flex h-toolbar flex-none items-center gap-2 border-b border-white/10 px-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-white">Open Windows</span>
        {asSidebar ? (
          <button
            type="button"
            className="ml-auto rounded-xs px-1 text-xs hover:bg-white/10 hover:text-white"
            title="Show the full menu in the left pane"
            onClick={() => api.setWindowPreferences({ sidebar: 'menu' }).catch(() => {})}
          >
            Menu
          </button>
        ) : (
          <button
            type="button"
            className="ml-auto rounded-xs px-1 text-xs hover:bg-white/10 hover:text-white"
            title="Hide the Open Window List (View › Open Window List)"
            aria-label="Hide the Open Window List"
            onClick={() => api.setWindowPreferences({ showOpenWindowList: false }).catch(() => {})}
          >
            ✕
          </button>
        )}
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto py-1 scrollbar-thin">
        {windows.length === 0 && <li className="px-3 py-1 text-xs">No windows open</li>}
        {windows.map((w) => {
          const active = w.id === activeId;
          const name = windowName(w);
          return (
            <li key={w.id}>
              <button
                type="button"
                className={`qb-nav-item h-row w-full truncate text-left text-sm ${active ? 'bg-white/15 font-semibold text-white' : ''} ${w.minimized ? 'italic' : ''}`}
                aria-current={active ? 'true' : undefined}
                title={w.minimized ? `${name} (minimized)` : name}
                onClick={() => focus(w.id)}
              >
                <span className="truncate">{name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
