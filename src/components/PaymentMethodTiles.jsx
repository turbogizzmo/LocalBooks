import { useEffect, useRef, useState } from 'react';
import { Icon } from '../lib/icons.js';
import { NewPaymentMethodDialog, usePaymentMethods } from './ProfileFields.jsx';

// The kinds of payment with a tile of their own: each tile is the first active method of its kind.
// Every other active method (Wire, Other, a second card such as Visa) sits under More.
const TILE_KINDS = [
  { kind: 'CASH', label: 'Cash', icon: 'PayCash' },
  { kind: 'CHECK', label: 'Check', icon: 'PayCheck' },
  { kind: 'CREDIT_CARD', label: 'Card', icon: 'PayCard', hint: 'Credit or debit card' },
  { kind: 'ACH', label: 'Bank / ACH', icon: 'PayBank', hint: 'ACH, e-check or bank transfer' },
];

const TILE =
  'flex h-[58px] w-[66px] flex-col items-center justify-center gap-1 rounded-sm border text-2xs font-semibold uppercase tracking-wide ' +
  'focus-visible:outline-none focus-visible:shadow-focus';
const tileState = (pressed) =>
  pressed
    ? 'border-qb-green bg-qb-green-tint text-qb-green-active ring-1 ring-inset ring-qb-green'
    : 'border-qb-border-strong bg-qb-surface text-qb-ink-muted hover:border-qb-ink-faint hover:text-qb-ink';

/**
 * How a payment was received, as a row of large tiles from the Payment Method list: Cash, Check,
 * Card, Bank / ACH (the first active method of each kind, under its own name), and More ▾ for the
 * rest of the list and "＋ Add new…". The chosen tile is highlighted and `aria-pressed`. `value` is
 * the chosen method's id; `onChange` gets the method.
 */
export default function PaymentMethodTiles({ value, onChange, label = 'Payment method' }) {
  const methods = usePaymentMethods();
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const wrapRef = useRef(null);
  const moreRef = useRef(null);
  const menuRef = useRef(null);
  const tiles = TILE_KINDS.map((t) => ({ ...t, method: methods.find((m) => m.isActive && m.kind === t.kind) })).filter((t) => t.method);
  const MORE = methods.filter((m) => (m.isActive || m.id === value) && !tiles.some((t) => t.method.id === m.id));
  const moreChosen = MORE.find((m) => m.id === value);

  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => {
      if (!wrapRef.current?.contains(e.target)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    (menuRef.current?.querySelector('[aria-checked="true"]') ?? menuRef.current?.querySelector('button'))?.focus();
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const pickMore = (m) => {
    onChange(m);
    setOpen(false);
    moreRef.current?.focus();
  };
  const onMenuKey = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      setOpen(false);
      moreRef.current?.focus();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const items = [...menuRef.current.querySelectorAll('button')];
      const at = items.indexOf(document.activeElement);
      items[(at + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (e.key === 'Tab') {
      setOpen(false);
    }
  };

  return (
    <div ref={wrapRef} role="group" aria-label={label} className="relative flex items-end gap-1">
      {tiles.map((t) => (
        <button
          key={t.kind}
          type="button"
          className={`${TILE} ${tileState(value === t.method.id)}`}
          aria-pressed={value === t.method.id}
          title={t.method.name === t.label ? t.hint : t.method.name}
          onClick={() => onChange(t.method)}
        >
          <Icon name={t.icon} size={22} />
          <span className="max-w-[62px] truncate">{t.method.name.length <= 10 ? t.method.name : t.label}</span>
        </button>
      ))}
      {(
        <button
          ref={moreRef}
          type="button"
          className={`${TILE} ${tileState(Boolean(moreChosen))}`}
          aria-pressed={Boolean(moreChosen)}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={moreChosen ? `More payment methods: ${moreChosen.name} chosen` : 'More payment methods'}
          onClick={() => setOpen((o) => !o)}
        >
          <Icon name="PayMore" size={22} />
          <span className="flex items-center gap-0.5">
            <span className="max-w-[48px] truncate">{moreChosen ? moreChosen.name : 'More'}</span>
            <Icon name="MenuArrow" size={10} strokeWidth={2.5} />
          </span>
        </button>
      )}
      {open && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="More payment methods"
          onKeyDown={onMenuKey}
          className="absolute right-0 top-full z-40 mt-0.5 min-w-[150px] rounded-xs border border-qb-border-strong bg-white py-0.5 shadow-popover"
        >
          {MORE.map((m) => (
            <button
              key={m.id}
              type="button"
              role="menuitemradio"
              aria-checked={value === m.id}
              onClick={() => pickMore(m)}
              className="flex w-full items-center gap-1 whitespace-nowrap px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover focus:bg-qb-row-hover focus:outline-none"
            >
              <span className="w-3 text-qb-green-active">{value === m.id ? '✓' : ''}</span>
              {m.name}
            </button>
          ))}
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              setAdding(true);
            }}
            className="flex w-full items-center gap-1 whitespace-nowrap border-t border-qb-border px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover focus:bg-qb-row-hover focus:outline-none"
          >
            <span className="w-3" />＋ Add new…
          </button>
        </div>
      )}
      {adding && (
        <NewPaymentMethodDialog
          onClose={() => setAdding(false)}
          onCreated={(m) => {
            setAdding(false);
            onChange(m);
          }}
        />
      )}
    </div>
  );
}
