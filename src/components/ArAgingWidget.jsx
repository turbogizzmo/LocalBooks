import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

// Status palette (good → critical); every bucket also carries an icon and text label.
const BUCKET_STYLE = {
  current: { color: '#0ca30c', icon: '✓' },
  days1To30: { color: '#fab219', icon: '!' },
  days31To60: { color: '#ec835a', icon: '!!' },
  days61Plus: { color: '#d03b3b', icon: '⚠' },
};

export default function ArAgingWidget() {
  const { data, error } = useLedgerQuery(api.getArAging);

  return (
    <div className="qb-panel col-span-2 px-2 py-1">
      <div className="flex items-baseline justify-between">
        <span className="text-xs font-semibold uppercase tracking-wide text-qb-ink-muted">A/R aging · open invoices</span>
        <span className="num text-md font-semibold">{data ? formatCents(data.totalCents) : '—'}</span>
      </div>
      {error && <div className="text-xs text-qb-negative">{error}</div>}
      {data && (
        <>
          <div className="my-1 flex h-2.5 gap-[2px] overflow-hidden rounded-[4px] bg-qb-canvas" role="img" aria-label="Open receivables by age">
            {data.totalCents > 0 &&
              data.buckets
                .filter((b) => b.amountCents > 0)
                .map((b) => (
                  <div
                    key={b.key}
                    title={`${b.label}: ${formatCents(b.amountCents)} (${b.invoiceCount})`}
                    style={{ flexGrow: b.amountCents, flexBasis: 0, minWidth: 4, backgroundColor: BUCKET_STYLE[b.key].color }}
                  />
                ))}
          </div>
          <div className="grid grid-cols-4 gap-1 text-xs">
            {data.buckets.map((b) => (
              <div key={b.key} className="min-w-0">
                <div className="flex items-center gap-1 text-qb-ink-muted">
                  <span className="h-2 w-2 flex-none rounded-[2px]" style={{ backgroundColor: BUCKET_STYLE[b.key].color }} />
                  <span aria-hidden="true" className="font-semibold">{BUCKET_STYLE[b.key].icon}</span>
                  <span className="truncate">{b.label}</span>
                </div>
                <div className="num text-left font-semibold">{formatCents(b.amountCents)}</div>
                <div className="text-2xs text-qb-ink-faint">{b.invoiceCount} invoice{b.invoiceCount === 1 ? '' : 's'}</div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
