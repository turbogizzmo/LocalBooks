import { useEffect, useRef, useState } from 'react';
import { Icon } from '../lib/icons.js';
import { PAYMENT_METHODS } from '../lib/api.js';

// The methods with a tile of their own; the rest of PAYMENT_METHODS sit under More.
const TILES = [
  { id: 'CASH', label: 'Cash', icon: 'PayCash' },
  { id: 'CHECK', label: 'Check', icon: 'PayCheck' },
  { id: 'CREDIT_CARD', label: 'Card', icon: 'PayCard', hint: 'Credit or debit card' },
  { id: 'ACH', label: 'Bank / ACH', icon: 'PayBank', hint: 'ACH, e-check or bank transfer' },
];
const MORE = PAYMENT_METHODS.filter((m) => !TILES.some((t) => t.id === m.id));

const TILE =
  'flex h-[58px] w-[66px] flex-col items-center justify-center gap-1 rounded-sm border text-2xs font-semibold uppercase tracking-wide ' +
  'focus-visible:outline-none focus-visible:shadow-focus';
const tileState = (pressed) =>
  pressed
    ? 'border-qb-green bg-qb-green-tint text-qb-green-active ring-1 ring-inset ring-qb-green'
    : 'border-qb-border-strong bg-qb-surface text-qb-ink-muted hover:border-qb-ink-faint hover:text-qb-ink';

/**
 * How a payment was received, as a row of large tiles: Cash, Check, Card, Bank / ACH, and More ▾
 * for the other stored methods (Wire, Other). The chosen tile is highlighted and `aria-pressed`.
 * `value` and `onChange` use the PAYMENT_METHODS ids, so nothing new is stored.
 */
export default function PaymentMethodTiles({ value, onChange, label = 'Payment method' }) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef(null);
  const moreRef = useRef(null);
  const menuRef = useRef(null);
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

  const pickMore = (id) => {
    onChange(id);
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
      {TILES.map((t) => (
        <button
          key={t.id}
          type="button"
          className={`${TILE} ${tileState(value === t.id)}`}
          aria-pressed={value === t.id}
          title={t.hint}
          onClick={() => onChange(t.id)}
        >
          <Icon name={t.icon} size={22} />
          {t.label}
        </button>
      ))}
      {MORE.length > 0 && (
        <button
          ref={moreRef}
          type="button"
          className={`${TILE} ${tileState(Boolean(moreChosen))}`}
          aria-pressed={Boolean(moreChosen)}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={moreChosen ? `More payment methods: ${moreChosen.label} chosen` : 'More payment methods'}
          onClick={() => setOpen((o) => !o)}
        >
          <Icon name="PayMore" size={22} />
          <span className="flex items-center gap-0.5">
            {moreChosen ? moreChosen.label : 'More'}
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
              onClick={() => pickMore(m.id)}
              className="flex w-full items-center gap-1 whitespace-nowrap px-2 py-0.5 text-left text-sm hover:bg-qb-row-hover focus:bg-qb-row-hover focus:outline-none"
            >
              <span className="w-3 text-qb-green-active">{value === m.id ? '✓' : ''}</span>
              {m.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
