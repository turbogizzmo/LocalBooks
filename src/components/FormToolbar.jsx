import { forwardRef, useEffect, useRef, useState } from 'react';
import { Icon } from '../lib/icons.js';
import { pickAttachments } from './Attachments.jsx';
import { ConfirmDialog } from './RowMenu.jsx';
import { FORM_KEYS } from '../lib/formFlow.jsx';

/**
 * The strip of large icon-over-label buttons across the top of a transaction form: Find, Previous,
 * Next, New, Save, Void, Print ▾, Email ▾, Attach and so on, in groups split by a thin rule.
 *
 * `groups` is a list of groups, each a list of buttons:
 *   { id, label, icon, onClick, disabled?, hint?, keys?, hidden?, danger?,
 *     confirm?: { title, body, confirmLabel },   // asks first, as the row menus do
 *     menu?: [{ label, onSelect, disabled?, hint? }] }   // a ▾ button that opens a menu
 * `icon` names an icon registered in src/lib/icons.js. Hidden buttons are left out, and a group
 * with nothing left in it is dropped with its rule, so a form only shows what it can do.
 *
 * Every button is a tab stop in reading order; Left and Right arrows also move along the strip.
 * A button that can't be used now stays focusable with `aria-disabled` (as the ARIA toolbar
 * pattern recommends), so its hint saying why can be reached by keyboard and hover; it does
 * nothing when pressed.
 * `keys` names the shortcut that presses it, shown in its tip (`save`, `void`, `delete` and
 * `memorize` have theirs already). On a form using `useFormFlow` (lib/formFlow.jsx), Ctrl+D presses
 * the button with id `void` (or `delete`) and Ctrl+M `memorize`.
 */
export default function FormToolbar({ label, groups, children }) {
  const [pending, setPending] = useState(null);
  const shown = groups.map((g) => g.filter((b) => b && !b.hidden)).filter((g) => g.length > 0);

  const onKeyDown = (e) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    // Arrows inside an open menu or a field belong to it.
    if (e.target.closest('[role="menu"]') || e.target.tagName === 'INPUT') return;
    const buttons = [...e.currentTarget.querySelectorAll('[data-toolbar-button]')];
    const at = buttons.indexOf(e.target);
    if (at < 0) return;
    e.preventDefault();
    const next = buttons[(at + (e.key === 'ArrowRight' ? 1 : buttons.length - 1)) % buttons.length];
    next?.focus();
  };

  const run = (item) => {
    if (item.disabled) return;
    if (item.confirm) setPending({ ...item, onSelect: item.onClick });
    else item.onClick?.();
  };

  return (
    <div
      role="toolbar"
      aria-label={label}
      onKeyDown={onKeyDown}
      className="flex flex-none flex-wrap items-stretch gap-y-1 border-b border-qb-border bg-qb-canvas px-1 py-0.5"
    >
      {shown.map((group, g) => (
        <div key={g} role="group" className="flex items-stretch">
          {g > 0 && <span aria-hidden="true" className="mx-1 my-1 w-px bg-qb-border-strong" />}
          {group.map((item) =>
            item.menu ? <MenuButton key={item.id} item={item} /> : <ToolButton key={item.id} item={item} onClick={() => run(item)} />,
          )}
        </div>
      ))}
      {children && <div className="ml-auto flex items-center gap-2 px-1">{children}</div>}
      {pending && <ConfirmDialog item={pending} onClose={() => setPending(null)} />}
    </div>
  );
}

const BUTTON =
  'flex min-w-[52px] flex-col items-center justify-start gap-0.5 rounded-sm px-1.5 pb-0.5 pt-1 text-2xs font-semibold leading-tight ' +
  'focus-visible:bg-qb-surface focus-visible:shadow-focus focus-visible:outline-none';
const ENABLED = 'text-qb-ink hover:bg-qb-surface hover:shadow-panel';
const DISABLED = 'cursor-not-allowed text-qb-ink-faint opacity-60';

/** The keys that press a button, by its id, unless the button names its own. */
const BUTTON_KEYS = { save: FORM_KEYS['save-new'], void: FORM_KEYS.delete, delete: FORM_KEYS.delete, memorize: FORM_KEYS.memorize, new: FORM_KEYS.new, print: FORM_KEYS.print, previous: FORM_KEYS.previous, next: FORM_KEYS.next };

const ToolButton = forwardRef(function ToolButton({ item, onClick, ...rest }, ref) {
  const keys = item.keys ?? BUTTON_KEYS[item.id];
  return (
    <button
      ref={ref}
      type="button"
      data-toolbar-button
      data-command={item.id}
      className={`${BUTTON} ${item.disabled ? DISABLED : ENABLED} ${item.danger && !item.disabled ? 'text-qb-negative' : ''}`}
      aria-disabled={item.disabled ? true : undefined}
      title={keys ? `${item.hint ?? item.label} (${keys})` : item.hint}
      onClick={item.disabled ? undefined : onClick}
      {...rest}
    >
      <Icon name={item.icon} size={20} />
      <span className="flex items-center gap-0.5 whitespace-nowrap">
        {item.label}
        {item.menu && <Icon name="MenuArrow" size={10} strokeWidth={2.5} />}
      </span>
    </button>
  );
});

/** A toolbar button that opens a short menu (Print ▾, Email ▾). */
function MenuButton({ item }) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef(null);
  const buttonRef = useRef(null);
  const menuRef = useRef(null);
  const entries = item.menu.filter(Boolean);
  // A menu with nothing it can do is shown as a disabled button.
  const disabled = item.disabled || entries.every((m) => m.disabled);

  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => {
      if (!wrapRef.current?.contains(e.target)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    menuRef.current?.querySelector('button:not(:disabled)')?.focus();
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const onMenuKey = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      setOpen(false);
      buttonRef.current?.focus();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const items = [...menuRef.current.querySelectorAll('button:not(:disabled)')];
      const at = items.indexOf(document.activeElement);
      items[(at + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (e.key === 'Tab') {
      setOpen(false);
    }
  };

  const choose = (entry) => {
    setOpen(false);
    buttonRef.current?.focus();
    entry.onSelect();
  };

  return (
    <div ref={wrapRef} className="relative flex">
      <ToolButton
        ref={buttonRef}
        item={{ ...item, disabled }}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown' && !disabled) {
            e.preventDefault();
            setOpen(true);
          }
        }}
        aria-haspopup="menu"
        aria-expanded={open}
      />
      {open && (
        <div
          ref={menuRef}
          role="menu"
          aria-label={item.label}
          onKeyDown={onMenuKey}
          className="absolute left-0 top-full z-40 mt-0.5 min-w-[180px] rounded-xs border border-qb-border-strong bg-white py-0.5 shadow-popover"
        >
          {entries.map((entry) => (
            <button
              key={entry.label}
              type="button"
              role="menuitem"
              disabled={entry.disabled}
              title={entry.hint}
              onClick={() => choose(entry)}
              className="block w-full whitespace-nowrap px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover focus:bg-qb-row-hover focus:outline-none disabled:cursor-not-allowed disabled:text-qb-ink-faint"
            >
              {entry.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * The buttons most forms share, so each form only says what it can do:
 *   navButtons({ onFind, onPrevious, onNext, blockedHint })   Find, ◀ Previous, Next ▶
 *   fileButtons({ onNew, onSave, saveLabel, saveDisabled, busy, onVoid, voidConfirm })
 * Leave a handler out to hide that button.
 */
export function navButtons({ onFind, onPrevious, onNext, previousDisabled, nextDisabled, blockedHint }) {
  return [
    { id: 'find', label: 'Find', icon: 'Search', hint: 'Find transactions', onClick: onFind, hidden: !onFind },
    { id: 'previous', label: 'Previous', icon: 'Back', hint: (previousDisabled && blockedHint) || 'Open the previous one', onClick: onPrevious, hidden: !onPrevious, disabled: previousDisabled },
    { id: 'next', label: 'Next', icon: 'Go', hint: (nextDisabled && blockedHint) || 'Open the next one', onClick: onNext, hidden: !onNext, disabled: nextDisabled },
  ];
}

export function fileButtons({ onNew, onSave, saveLabel = 'Save', saveHint, saveDisabled = false, busy = false, onVoid, voidDisabled, voidHint, voidConfirm }) {
  return [
    { id: 'new', label: 'New', icon: 'NewForm', hint: 'Clear the form and start a new one', onClick: onNew, hidden: !onNew, disabled: busy },
    { id: 'save', label: saveLabel, icon: 'SaveForm', hint: saveHint ?? 'Save and start a new one', onClick: onSave, hidden: !onSave, disabled: busy || saveDisabled },
    {
      id: 'void',
      label: 'Void',
      icon: 'Void',
      danger: true,
      hint: voidHint ?? 'Void this transaction with a reversing entry',
      onClick: onVoid,
      hidden: !onVoid,
      disabled: busy || voidDisabled,
      confirm: voidConfirm,
    },
  ];
}

/**
 * Attach File for a form that has not posted yet: picks files to attach once it posts (shown on the
 * form with PendingAttachments). `setFiles` is the form's state setter for those paths.
 */
export function attachLaterButton({ setFiles, busy = false, what = 'it' }) {
  return {
    id: 'attach',
    label: 'Attach File',
    icon: 'Attach',
    hint: `Pick files to attach when ${what} posts`,
    disabled: busy,
    onClick: async () => {
      const picked = await pickAttachments();
      if (picked.length) setFiles((f) => [...new Set([...f, ...picked])]);
    },
  };
}

