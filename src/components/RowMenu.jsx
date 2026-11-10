import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { errorText } from '../lib/api.js';
import { myPreferences } from '../lib/myPreferences.js';

/**
 * Small ▾ action menu for register rows; with `text`, a toolbar button of that name (the list
 * toolbars' New Transactions, Print and Excel menus). Items with `confirm` open a blocking confirmation dialog
 * before running; errors from the action are shown inside the dialog.
 *
 * item: { label, onSelect: async () => void, danger?, disabled?, hint?, confirm?: { title, body, confirmLabel, unless? } }
 * `confirm.unless()` returning true skips the question (a My Preference turned off); the dialog
 * still opens to show the action's progress and any error.
 */
export default function RowMenu({ items, label = 'Actions', text = null }) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, right: 0 });
  // The button's top and bottom edges when the menu opened, to place the menu below or above it.
  const [anchor, setAnchor] = useState({ top: 0, bottom: 0 });
  const [pending, setPending] = useState(null);
  const buttonRef = useRef(null);
  const menuRef = useRef(null);

  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => {
      if (!menuRef.current?.contains(e.target) && !buttonRef.current?.contains(e.target)) setOpen(false);
    };
    const onKey = (e) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', onKey);
    menuRef.current?.querySelector('button:not(:disabled)')?.focus();
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const toggle = (e) => {
    e.stopPropagation();
    const rect = buttonRef.current.getBoundingClientRect();
    // Fixed positioning escapes the scrolling table; anchoring the right edge keeps it on screen.
    setAnchor({ top: rect.top, bottom: rect.bottom });
    setPosition({ top: rect.bottom + 2, right: Math.max(4, window.innerWidth - rect.right), visibility: 'hidden' });
    setOpen((o) => !o);
  };

  // Once the menu has its size: below the button when it fits, otherwise above it (rows near the
  // bottom of the window), and never past the window's top or bottom edge, where a fixed menu
  // could not be scrolled to.
  useLayoutEffect(() => {
    if (!open || !menuRef.current) return;
    const height = menuRef.current.offsetHeight;
    const room = window.innerHeight - 4;
    let top = anchor.bottom + 2;
    if (top + height > room) top = anchor.top - 2 - height;
    top = Math.max(4, Math.min(top, room - height));
    setPosition((p) => ({ top, right: p.right }));
  }, [open, anchor]);

  const choose = (item) => {
    setOpen(false);
    if (item.confirm) setPending({ ...item, skipQuestion: Boolean(item.confirm.unless?.()) });
    else item.onSelect();
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={toggle}
        className={text ? 'qb-btn qb-btn-secondary flex h-row flex-none items-center gap-1 px-2' : 'inline-grid h-[18px] w-[18px] place-items-center rounded-xs text-qb-ink-muted hover:bg-qb-border hover:text-qb-ink'}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={text ? undefined : label}
      >
        {text}
        <svg viewBox="0 0 10 10" width="8" height="8" aria-hidden="true">
          <path d="M1.5 3.5L5 7l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </button>
      {open && (
        <div
          ref={menuRef}
          role="menu"
          className="fixed z-40 max-h-[calc(100vh-8px)] min-w-[180px] max-w-[360px] overflow-y-auto rounded-xs border border-qb-border-strong bg-white py-0.5 shadow-popover"
          style={position}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              role="menuitem"
              disabled={item.disabled}
              title={item.hint}
              onClick={() => choose(item)}
              className={`block w-full whitespace-nowrap px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover focus:bg-qb-row-hover focus:outline-none disabled:cursor-not-allowed disabled:text-qb-ink-faint ${item.danger ? 'text-qb-negative' : ''}`}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
      {pending && <ConfirmDialog item={pending} onClose={() => setPending(null)} />}
    </>
  );
}

export function ConfirmDialog({ item, onClose }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const cancelRef = useRef(null);
  const started = useRef(false);

  useEffect(() => {
    if (item.skipQuestion && !started.current) {
      started.current = true;
      confirm();
    }
    // Runs once, when the dialog opens without asking.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    cancelRef.current?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape' && !busy) onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [busy, onClose]);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await item.onSelect();
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center whitespace-normal bg-qb-nav/40 text-left backdrop-blur-[2px]" onMouseDown={(e) => e.stopPropagation()}>
      <div role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" className="w-[420px] rounded-xs bg-white shadow-popover">
        <div className="border-b border-qb-border px-3 py-1.5">
          <h2 id="confirm-title" className="text-md font-semibold">{item.confirm.title}</h2>
        </div>
        <div className="space-y-1 px-3 py-2 text-sm">
          <p>{item.confirm.body}</p>
          {error && <p className="font-semibold text-qb-negative">{error}</p>}
        </div>
        <div className="flex justify-end gap-2 border-t border-qb-border px-3 py-1.5">
          <button ref={cancelRef} type="button" className="qb-btn qb-btn-secondary" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className={`qb-btn ${item.danger ? 'bg-qb-negative text-white hover:opacity-90' : 'qb-btn-primary'}`}
            onClick={confirm}
            disabled={busy}
          >
            {busy ? 'Working…' : item.confirm.confirmLabel ?? 'Confirm'}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Standard "Void Transaction" menu item for any posted document. */
export function voidItem({ journalEntryId, voided, description, onVoid }) {
  return {
    label: voided ? 'Voided' : 'Void Transaction',
    danger: true,
    disabled: voided || !journalEntryId,
    hint: voided ? 'This transaction is already voided' : undefined,
    confirm: {
      title: 'Void this transaction?',
      body: `${description} A reversing journal entry dated the same day will zero out its effect. The original stays in the books and the audit log. This cannot be undone.`,
      confirmLabel: 'Yes, void it',
      unless: () => !myPreferences().warnBeforeVoiding,
    },
    onSelect: () => onVoid(journalEntryId),
  };
}
