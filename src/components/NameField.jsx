import { useId, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import { PartyDialog } from './PartyList.jsx';
import { api, errorText } from '../lib/api.js';
import { useMyPreferences } from '../lib/myPreferences.js';

/** How many matching names the list shows at once. */
const MAX_SHOWN = 12;

const sameName = (a, b) => a.trim().toLocaleLowerCase() === b.trim().toLocaleLowerCase();

const KINDS = {
  vendor: { noun: 'vendor', create: (name) => api.createVendor(name, null) },
  customer: { noun: 'customer', create: (name) => api.createCustomer(name, null) },
};

/**
 * The name box on forms: Vendor on bills, checks, expenses, purchase orders and vendor credits,
 * Customer on invoices, sales receipts, estimates and credit memos. Typing shows the names that
 * contain what was typed. A name that is not on the list can be added from the same list: "Add …
 * as a new vendor" (or customer) adds it straight away, "Add with details…" adds it and opens its
 * record.
 *
 * The value is the name (the forms post a name, and the ledger matches it to a vendor or customer),
 * so a name typed and saved without choosing an entry is still added on save, as before.
 */
export default function NameField({ kind, names, value, onChange, inputRef, className = 'qb-input w-[200px]', placeholder, autoFocus = false, label }) {
  const { noun, create } = KINDS[kind];
  const listId = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [details, setDetails] = useState(null);
  // Preferences › General: open the list while typing, or only on ↓.
  const autoOpen = useMyPreferences().autoOpenDropDowns;

  const typed = value.trim();
  const known = useMemo(() => names.filter((v) => v.isActive !== false), [names]);
  const exact = typed !== '' && names.some((v) => sameName(v.name, typed));
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

  const add = async (mode) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await create(typed);
      onChange(created.name);
      setOpen(false);
      if (mode === 'details') setDetails(created.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
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
        ref={inputRef}
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
          setError(null);
        }}
        onFocus={() => autoOpen && setOpen(true)}
        onBlur={() => setOpen(false)}
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
              {row.name ? row.name.name : row.add === 'quick' ? <>＋ Add “{typed}” as a new {noun}</> : 'Add with details…'}
            </li>
          ))}
        </ul>
      )}
      {error && <span className="absolute left-0 top-full z-40 mt-0.5 whitespace-nowrap text-xs text-qb-negative">{error}</span>}
      {details !== null &&
        // Rendered outside the field, whose <label> would otherwise take clicks meant for the dialog.
        createPortal(
          <PartyDialog
            kind={kind}
            id={details}
            onClose={() => setDetails(null)}
            onSaved={(_text, saved) => {
              if (saved) onChange(saved.name);
              setDetails(null);
            }}
          />,
          document.body,
        )}
    </span>
  );
}
