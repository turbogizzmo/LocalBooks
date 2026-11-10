import { useState } from 'react';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { api, useLedgerQuery } from '../lib/api.js';
import { useOpenFromList } from '../lib/windows.js';
import { itemDetail, openTarget, segmentItem } from '../lib/moneyBars.js';

/**
 * Money bar: a few labelled totals, each with a status icon, label and count so the color stripe
 * is never the only carrier of meaning. With `side` (`SALES` or `BILLS`) it adds the segments from
 * `money_bars.rs`; a segment with rows is a button that lists its transactions under the bar, and
 * each row opens the window that holds it.
 */
export default function MoneyBar({ items, side }) {
  const segments = useLedgerQuery(() => (side ? api.getMoneyBarSegments(side) : Promise.resolve([]))).data ?? [];
  const cells = [...items, ...segments.map(segmentItem)];
  const [shown, setShown] = useState(null);
  const openFromList = useOpenFromList();
  const listed = cells.find((c) => c.label === shown && c.list);
  return (
    <div className="flex flex-none flex-col gap-1">
      <div className="qb-panel grid gap-px overflow-hidden bg-qb-border" style={{ gridTemplateColumns: `repeat(${cells.length}, minmax(0, 1fr))` }}>
        {cells.map((item) => {
          const body = (
            <>
              <div className="flex items-baseline gap-1 text-xs text-qb-ink-muted">
                <span aria-hidden="true" style={{ color: item.color }}>{item.icon}</span>
                <span className="font-semibold uppercase tracking-wide">{item.label}</span>
                {item.count !== undefined && <span>· {item.count}</span>}
              </div>
              <div className="num text-left text-xl font-semibold">{formatCents(item.cents)}</div>
              <div className="mt-0.5 h-1 rounded-full" style={{ backgroundColor: item.color }} />
            </>
          );
          return item.list ? (
            <button
              key={item.label}
              type="button"
              className={`px-2 py-1 text-left hover:bg-qb-row-hover ${shown === item.label ? 'bg-qb-row-selected' : 'bg-white'}`}
              aria-expanded={shown === item.label}
              aria-label={`${item.label}: ${formatCents(item.cents)}, ${item.count}. Show the transactions`}
              onClick={() => setShown(shown === item.label ? null : item.label)}
            >
              {body}
            </button>
          ) : (
            <div key={item.label} className="bg-white px-2 py-1">
              {body}
            </div>
          );
        })}
      </div>
      {listed && (
        <div className="qb-panel max-h-48 overflow-auto" role="region" aria-label={`${listed.label} transactions`}>
          <table className="w-full text-sm">
            <tbody>
              {listed.list.map((row) => {
                const target = openTarget(row);
                return (
                  <tr
                    key={`${row.kind}-${row.id}`}
                    className="cursor-pointer hover:bg-qb-row-hover"
                    tabIndex={0}
                    onClick={() => target && openFromList(...target)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && target) openFromList(...target);
                    }}
                  >
                    <td className="w-24 px-2 py-0.5 text-qb-ink-muted">{row.date ? formatDate(row.date) : ''}</td>
                    <td className="px-2 py-0.5">{row.name}</td>
                    <td className="truncate px-2 py-0.5 text-qb-ink-muted">{itemDetail(row)}</td>
                    <td className="num px-2 py-0.5 text-right">{formatCents(row.amountCents)}</td>
                  </tr>
                );
              })}
              {listed.list.length === 0 && (
                <tr>
                  <td className="px-2 py-1 text-qb-ink-muted">Nothing here.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
