import { useEffect, useMemo, useRef, useState } from 'react';
import { findNav } from './Sidebar.jsx';
import GuideView, { GUIDES, GUIDE_ORDER } from './GuideView.jsx';
import About from './About.jsx';
import { Icon } from '../lib/icons.js';

/** Help areas, in the order they are listed under "All topics". */
export const HELP_AREAS = [
  { id: 'sales', label: 'Sales & customers', icon: 'Sales' },
  { id: 'expenses', label: 'Expenses & vendors', icon: 'Expenses' },
  { id: 'employees', label: 'Employees & time', icon: 'Employees' },
  { id: 'banking', label: 'Banking', icon: 'Banking' },
  { id: 'accounting', label: 'Accounting & audit', icon: 'Accounting' },
  { id: 'reports', label: 'Reports', icon: 'Reports' },
  { id: 'company', label: 'Company & data', icon: 'Settings' },
];

/** Help topics and screen help, one file per area under `help/`, in file-name order. */
const HELP_FILES = import.meta.glob('./help/*.js', { eager: true });
const HELP = Object.keys(HELP_FILES).sort().map((file) => HELP_FILES[file]);
export const HELP_ARTICLES = HELP.flatMap((file) => file.topics);
/** Each screen's `help` text and main `guide`. */
const SCREENS = Object.assign({}, ...HELP.map((file) => file.screens));

/** True when every word of the query appears in the article (title, text or keywords). */
export function matchesQuery(article, query) {
  const haystack = `${article.title} ${article.body} ${article.keywords}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/** One topic: the title toggles the two-sentence answer, its shortcut and its full guide. */
function Article({ article, view, expanded, onToggle, onNavigate, onOpenGuide }) {
  const guide = article.guide;
  // A plain "Go to" this very screen is noise; a shortcut that opens a form here is still useful.
  const go = article.go && !(article.go.view === view && !article.go.action) ? article.go : null;
  return (
    <li className="border-b border-qb-border last:border-b-0">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        className="flex w-full items-start gap-1.5 px-3 py-1.5 text-left hover:bg-qb-row-hover"
      >
        <svg viewBox="0 0 10 10" width="8" height="8" className={`mt-1 flex-none transition-transform duration-75 ${expanded ? 'rotate-90' : ''}`} aria-hidden="true">
          <path d="M3 1.5L6.5 5 3 8.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
        <span className={`text-sm leading-tight ${expanded ? 'font-semibold' : ''}`}>{article.title}</span>
      </button>
      {expanded && (
        <div className="px-3 pb-2 pl-[26px]">
          <p className="leading-snug text-qb-ink-muted">{article.body}</p>
          {go && (
            <button
              type="button"
              onClick={() => onNavigate(go.view, go.action ?? null)}
              className="mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline"
            >
              {go.label}
              <Icon name="Go" />
            </button>
          )}
          {guide && GUIDES[guide] && (
            <button
              type="button"
              onClick={() => onOpenGuide(guide)}
              className={`mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-info hover:underline ${go ? 'ml-3' : ''}`}
            >
              <Icon name="Guide" />
              Step-by-step guide
            </button>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * Right-side help panel. Opens on the current screen's help: what the screen is for and the
 * tasks you can do there. Below that, every topic by area; searching looks across all topics.
 */
export default function HelpCenter({ view, onClose, onNavigate }) {
  const [query, setQuery] = useState('');
  const [expanded, setExpanded] = useState(() => new Set());
  const [openAreas, setOpenAreas] = useState(() => new Set());
  // The guide being read, with the trail of guides opened from it (for Back).
  const [reading, setReading] = useState([]);
  // An entry is a guide id, or `id#section` for a link to one of its headings.
  const entry = reading[reading.length - 1] ?? null;
  const [guideId, section] = entry ? entry.split('#') : [null];
  const openGuide = (id) => setReading((r) => [...r, id]);
  const body = useRef(null);
  // Braces matter: newer engines return a Promise from scrollTo, which React would call as cleanup.
  useEffect(() => {
    const heading = section && body.current?.querySelector?.(`#guide-${section}`);
    if (heading) heading.scrollIntoView?.({ block: 'start' });
    else body.current?.scrollTo?.(0, 0);
  }, [entry, reading.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const search = useRef(null);
  const screen = findNav(view);
  const screenTitle = screen?.item.label ?? 'This screen';
  // A Settings tab without help of its own shows Settings' help.
  const screenHelp = SCREENS[view] ?? SCREENS[screen?.parent?.id];
  const here = useMemo(() => HELP_ARTICLES.filter((a) => a.views.includes(view)), [view]);
  const results = useMemo(() => (query.trim() ? HELP_ARTICLES.filter((a) => matchesQuery(a, query)) : []), [query]);
  const guideResults = useMemo(
    () => (query.trim() ? GUIDE_ORDER.filter((id) => matchesQuery({ title: GUIDES[id].title, body: GUIDES[id].text, keywords: '' }, query)) : []),
    [query],
  );

  // A new screen starts with its first task open, so the answer is one click away at most.
  useEffect(() => {
    setExpanded(new Set(here.slice(0, 1).map((a) => a.id)));
  }, [here]);

  useEffect(() => {
    search.current?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const toggle = (setter) => (id) =>
    setter((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const toggleArticle = toggle(setExpanded);
  const toggleArea = toggle(setOpenAreas);
  const article = (a) => (
    <Article key={a.id} article={a} view={view} expanded={expanded.has(a.id)} onToggle={() => toggleArticle(a.id)} onNavigate={onNavigate} onOpenGuide={openGuide} />
  );

  return (
    <aside
      className={`no-print flex ${guideId ? 'w-[440px]' : 'w-80'} flex-none animate-[lb-slide-in_160ms_ease-out] flex-col border-l border-qb-border bg-white text-xs transition-[width] duration-150`}
      aria-label="Help"
    >
      <div className="flex h-toolbar flex-none items-center gap-2 border-b border-qb-border px-3">
        <Icon name="HelpCenter" className="text-qb-green" />
        <h2 className="text-md font-semibold">Help</h2>
        <button type="button" onClick={onClose} className="ml-auto rounded-xs p-1 text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink" aria-label="Close help">
          <Icon name="Close" />
        </button>
      </div>
      {guideId ? (
        <div className="flex flex-none items-center gap-2 border-b border-qb-border px-2 py-1.5">
          <button
            type="button"
            onClick={() => setReading((r) => r.slice(0, -1))}
            className="inline-flex items-center gap-1 rounded-xs px-1.5 py-0.5 font-semibold text-qb-ink-muted hover:bg-qb-row-hover hover:text-qb-ink"
          >
            <Icon name="Back" />
            {reading.length > 1 ? GUIDES[reading[reading.length - 2].split('#')[0]]?.title : 'Back to help'}
          </button>
          <span className="ml-auto text-2xs text-qb-ink-faint">Also in docs/guide on GitHub</span>
        </div>
      ) : (
      <div className="flex-none border-b border-qb-border p-2">
        <label className="flex items-center gap-1.5 rounded-xs border border-qb-border-strong px-2 focus-within:border-qb-green focus-within:shadow-focus">
          <Icon name="Search" className="text-qb-ink-muted" />
          <input
            ref={search}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search all help (e.g. undo, close the year)"
            className="h-row-lg min-w-0 flex-1 bg-transparent text-sm outline-none"
            aria-label="Search help"
          />
        </label>
      </div>
      )}

      <div ref={body} className="min-h-0 flex-1 overflow-y-auto scrollbar-thin">
        {guideId ? (
          <GuideView id={guideId} onOpenGuide={openGuide} />
        ) : query.trim() ? (
          <section aria-label="Search results">
            <h3 className="bg-qb-row-header px-3 py-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted" aria-live="polite">
              {results.length} topic{results.length === 1 ? '' : 's'}
            </h3>
            {results.length === 0 && guideResults.length === 0 ? (
              <p className="px-3 py-3 text-qb-ink-muted">Nothing matches “{query}”. Try fewer or different words.</p>
            ) : (
              <ul>{results.map(article)}</ul>
            )}
            {guideResults.length > 0 && (
              <>
                <h3 className="bg-qb-row-header px-3 py-1 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Guides that mention it</h3>
                <ul>
                  {guideResults.map((id) => (
                    <li key={id}>
                      <button type="button" onClick={() => openGuide(id)} className="flex w-full items-center gap-2 px-3 py-1 text-left text-sm hover:bg-qb-row-hover">
                        <Icon name="Guide" className="text-qb-ink-muted" />
                        {GUIDES[id].title}
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        ) : (
          <>
            <section aria-label={`Help for ${screenTitle}`} className="border-b border-qb-border">
              <div className="bg-qb-green-tint px-3 py-2">
                <div className="text-2xs font-semibold uppercase tracking-wide text-qb-green-active">On this screen</div>
                <h3 className="text-md font-semibold">{screenTitle}</h3>
                {screenHelp?.help && <p className="mt-0.5 leading-snug text-qb-ink">{screenHelp.help}</p>}
                {GUIDES[screenHelp?.guide] && (
                  <button
                    type="button"
                    onClick={() => openGuide(screenHelp.guide)}
                    className="mt-1.5 inline-flex items-center gap-1 font-semibold text-qb-green-active hover:underline"
                  >
                    <Icon name="Guide" />
                    Step-by-step: {GUIDES[screenHelp.guide].title}
                  </button>
                )}
              </div>
              {here.length > 0 && (
                <>
                  <h4 className="px-3 pb-0.5 pt-2 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">What you can do here</h4>
                  <ul>{here.map(article)}</ul>
                </>
              )}
            </section>
            <section aria-label="All topics">
              <h3 className="px-3 pb-1 pt-3 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">All topics</h3>
              <ul>
                {HELP_AREAS.map((area) => {
                  const topics = HELP_ARTICLES.filter((a) => a.area === area.id);
                  const open = openAreas.has(area.id);
                  return (
                    <li key={area.id} className="border-t border-qb-border">
                      <button
                        type="button"
                        onClick={() => toggleArea(area.id)}
                        aria-expanded={open}
                        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm font-semibold hover:bg-qb-row-hover"
                      >
                        <Icon name={area.icon} className="text-qb-ink-muted" />
                        <span className="flex-1">{area.label}</span>
                        <span className="text-2xs font-normal text-qb-ink-faint">{topics.length}</span>
                      </button>
                      {open && <ul className="bg-qb-canvas/40">{topics.map(article)}</ul>}
                    </li>
                  );
                })}
              </ul>
            </section>
            <section aria-label="Step-by-step guides" className="border-t border-qb-border pb-2">
              <h3 className="px-3 pb-1 pt-3 text-2xs font-semibold uppercase tracking-wide text-qb-ink-muted">Step-by-step guides</h3>
              <ul>
                {GUIDE_ORDER.map((id) => (
                  <li key={id}>
                    <button type="button" onClick={() => openGuide(id)} className="flex w-full items-center gap-2 px-3 py-1 text-left text-sm hover:bg-qb-row-hover">
                      <Icon name="Guide" className="text-qb-ink-muted" />
                      {GUIDES[id].title}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
            <About />
          </>
        )}
      </div>
    </aside>
  );
}
