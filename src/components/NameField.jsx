import { useId, useMemo, useRef, useState } from 'react';
import { askNewName, focusNameBox } from './NewNamePrompt.jsx';
import { useMyPreferences } from '../lib/myPreferences.js';

/** How many matching names the list shows at once. */
const MAX_SHOWN = 12;

const sameName = (a, b) => a.trim().toLocaleLowerCase() === b.trim().toLocaleLowerCase();

/**
 * The name box on forms: Vendor on bills, checks, expenses, purchase orders and vendor credits,
 * Customer on invoices, sales receipts, estimates and credit memos. Typing shows the names that
 * contain what was typed. A name that is not on the list can be added from the same list: "Add …
 * as a new vendor" (or customer) adds it straight away, "Add with details…" adds it and opens its
 * record.
 *
 * A name is never added without asking: leaving the box with a name that is not on the list asks
 * "<name> isn't on your customer list." with Quick add, Set up… and Cancel (`NewNamePrompt`), and so
 * does saving the form (the backend refuses a name that is not on the list, and the save asks the
 * same question). After Cancel, leaving the box again doesn't ask until the name changes; the save
 * still does.
 */
export default function NameField({ kind, names, value, onChange, inputRef, className = 'qb-input w-[200px]', placeholder, autoFocus = false, label }) {
  const noun = kind;
  const listId = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [busy, setBusy] = useState(false);
  // The name the person last answered Cancel for: leaving the box doesn't ask about it again.
  const declined = useRef(null);
  // Names added from this box, until the list it was given catches up.
  const added = useRef(new Set());
  const boxRef = useRef(null);
  const setBox = (el) => {
    boxRef.current = el;
    if (typeof inputRef === 'function') inputRef(el);
    else if (inputRef) inputRef.current = el;
  };
  // Preferences › General: open the list while typing, or only on ↓.
  const autoOpen = useMyPreferences().autoOpenDropDowns;

  const typed = value.trim();
  const known = useMemo(() => names.filter((v) => v.isActive !== false), [names]);
  const exact = typed !== '' && (names.some((v) => sameName(v.name, typed)) || added.current.has(typed.toLocaleLowerCase()));
  const matches = useMemo(() => {
    const needle = typed.toLocaleLowerCase();
    const hits = needle === '' ? known : known.filter((v) => v.name.toLocaleLowerCase().includes(needle));
    // Names that start with what was typed come first.
    return [...hits]
      .sort((a, b) => Number(!a.name.toLocaleLowerCase().startsWith(needle)) - Number(!b.name.toLocaleLowerCase().startsWith(needle)) || a.name.localeCompare(b.name))
      .slice(0, MAX_SHOWN);
  }, [known, typed]);

  // The rows of the list: matching names, then the two add rows when the name is new.
  const rows = [
    ...matches.map((v) => ({ key: `n${v.id}`, name: v })),
    ...(typed !== '' && !exact ? [{ key: 'quick', add: 'quick' }, { key: 'details', add: 'details' }] : []),
  ];
  const shown = open && rows.length > 0;
  // Never below 0, so ↓ on an empty list can't leave the highlight outside it.
  const activeRow = Math.max(0, Math.min(active, rows.length - 1));

  // `ask`: asks about the typed name. The list's own add rows are the answer already: `quick` adds
  // it, `details` adds it and opens its record.
  const add = async (mode) => {
    if (busy) return;
    setBusy(true);
    setOpen(false);
    const asked = typed;
    try {
      const answer = await askNewName(kind, asked, { start: mode });
      if (answer) {
        added.current.add(answer.name.toLocaleLowerCase());
        onChange(answer.name);
      } else {
        declined.current = asked.toLocaleLowerCase();
        focusNameBox(asked);
      }
    } finally {
      setBusy(false);
    }
  };

  // Leaving the box with a name that is not on the list asks about it.
  const onBlur = () => {
    setOpen(false);
    // Switching to another app isn't leaving the box.
    // An empty list is one still loading: saving asks if the name really is new.
    if (!document.hasFocus() || busy || typed === '' || exact || names.length === 0) return;
    if (declined.current === typed.toLocaleLowerCase()) return;
    // After this event: a form closing takes the box with it, and then there is nothing to ask.
    setTimeout(() => {
      if (boxRef.current?.isConnected) add('ask');
    }, 0);
  };

  const choose = (row) => {
    if (!row) return;
    if (row.name) {
      onChange(row.name.name);
      setOpen(false);
    } else {
      add(row.add);
    }
  };

  const onKeyDown = (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setOpen(true);
      setActive((a) => Math.max(0, Math.min(a + 1, rows.length - 1)));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter' && shown) {
      // Enter picks the highlighted row; with nothing highlighted it does what it would anyway.
      e.preventDefault();
      choose(rows[activeRow]);
    } else if (e.key === 'Escape' && shown) {
      e.stopPropagation();
      setOpen(false);
    }
  };

  return (
    <span className="relative block">
      <input
        ref={setBox}
        data-name-box={kind}
        className={className}
        value={value}
        placeholder={placeholder ?? (noun[0].toUpperCase() + noun.slice(1))}
        autoFocus={autoFocus}
        aria-label={label}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={shown}
        aria-controls={listId}
        aria-activedescendant={shown ? `${listId}-${activeRow}` : undefined}
        autoComplete="off"
        onChange={(e) => {
          onChange(e.target.value);
          setActive(0);
          setOpen(autoOpen);
          declined.current = null;
        }}
        onFocus={() => autoOpen && setOpen(true)}
        onBlur={onBlur}
        onKeyDown={onKeyDown}
      />
      {shown && (
        <ul id={listId} role="listbox" className="qb-panel absolute left-0 top-full z-50 mt-0.5 max-h-[260px] min-w-full w-max max-w-[340px] overflow-y-auto py-0.5 shadow-panel scrollbar-thin">
          {rows.map((row, i) => (
            <li
              key={row.key}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === activeRow}
              className={`cursor-pointer truncate px-2 py-0.5 text-sm ${i === activeRow ? 'bg-qb-row-selected' : 'hover:bg-qb-row-hover'} ${row.add === 'quick' && i > 0 ? 'border-t border-qb-border' : ''} ${row.add ? 'text-qb-link' : ''}`}
              onMouseEnter={() => setActive(i)}
              // Keep focus in the box, so blur doesn't close the list before the click lands.
              onMouseDown={(e) => {
                e.preventDefault();
                choose(row);
              }}
            >
              {row.name ? (
                <>
                  {row.name.name}
                  {row.name.listLabel && <span className="ml-1 text-2xs text-qb-ink-muted">· {row.name.listLabel}</span>}
                </>
              ) : row.add === 'quick' ? <>＋ Add “{typed}” as a new {noun}</> : 'Add with details…'}
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}
