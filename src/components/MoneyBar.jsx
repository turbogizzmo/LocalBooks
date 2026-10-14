import { formatCents } from '../lib/money.js';

/**
 * Money bar: a few labelled totals, each with a status icon, label and count so
 * the color stripe is never the only carrier of meaning.
 */
export default function MoneyBar({ items }) {
  return (
    <div className="qb-panel grid flex-none gap-px overflow-hidden bg-qb-border" style={{ gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))` }}>
      {items.map((item) => (
        <div key={item.label} className="bg-white px-2 py-1">
          <div className="flex items-baseline gap-1 text-xs text-qb-ink-muted">
            <span aria-hidden="true" style={{ color: item.color }}>{item.icon}</span>
            <span className="font-semibold uppercase tracking-wide">{item.label}</span>
            {item.count !== undefined && <span>· {item.count}</span>}
          </div>
          <div className="num text-left text-xl font-semibold">{formatCents(item.cents)}</div>
          <div className="mt-0.5 h-1 rounded-full" style={{ backgroundColor: item.color }} />
        </div>
      ))}
    </div>
  );
}
