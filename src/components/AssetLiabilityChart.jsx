import { useEffect, useRef, useState } from 'react';
import { formatCents, formatCompactCents } from '../lib/money.js';
import { monthLabel } from '../lib/dates.js';

// Validated pair (light surface): ΔE 26.7 under protanopia, contrast >= 3:1.
const SERIES = [
  { key: 'assetsCents', label: 'Assets', color: '#2ca01c' },
  { key: 'liabilitiesCents', label: 'Liabilities', color: '#0077c5' },
];

const MIN_HEIGHT = 180;
const MARGIN = { top: 8, right: 8, bottom: 20, left: 52 };
const BAR_GAP = 2;
const GRID = '#e4e6ea';

export function niceStep(raw) {
  const power = 10 ** Math.floor(Math.log10(raw));
  const m = raw / power;
  return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * power;
}

/** Column with a 4px rounded data end and a square baseline end; mirrors for negatives. */
export function barPath(x, width, y0, y1) {
  const h = Math.abs(y1 - y0);
  if (h < 0.5) return null;
  const r = Math.min(4, width / 2, h);
  if (y1 < y0) {
    return `M${x},${y0}V${y1 + r}Q${x},${y1} ${x + r},${y1}H${x + width - r}Q${x + width},${y1} ${x + width},${y1 + r}V${y0}Z`;
  }
  return `M${x},${y0}V${y1 - r}Q${x},${y1} ${x + r},${y1}H${x + width - r}Q${x + width},${y1} ${x + width},${y1 - r}V${y0}Z`;
}

export function useSize() {
  const ref = useRef(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const observer = new ResizeObserver(([entry]) =>
      setSize({ width: Math.floor(entry.contentRect.width), height: Math.floor(entry.contentRect.height) }),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, size];
}

function Chart({ series, width, height }) {
  const [hover, setHover] = useState(null);
  const chartHeight = Math.max(MIN_HEIGHT, height);
  const plotW = Math.max(0, width - MARGIN.left - MARGIN.right);
  const plotH = chartHeight - MARGIN.top - MARGIN.bottom;

  const values = series.flatMap((p) => SERIES.map((s) => p[s.key]));
  let lo = Math.min(0, ...values);
  let hi = Math.max(0, ...values);
  if (hi === lo) hi = lo + 100_000;
  const step = niceStep((hi - lo) / 4);
  const yMin = Math.floor(lo / step) * step;
  const yMax = Math.ceil(hi / step) * step;
  const y = (v) => MARGIN.top + ((yMax - v) / (yMax - yMin)) * plotH;
  const ticks = [];
  for (let t = yMin; t <= yMax + step / 2; t += step) ticks.push(t);

  const groupW = plotW / series.length;
  const barW = Math.max(2, Math.min(14, (groupW - 10 - BAR_GAP) / 2));
  const innerW = barW * 2 + BAR_GAP;
  const showEveryLabel = groupW >= 34;
  const y0 = y(0);

  const hovered = hover === null ? null : series[hover];
  const tooltipLeft =
    hover === null ? 0 : Math.min(Math.max(MARGIN.left + groupW * (hover + 0.5) - 90, 0), Math.max(0, width - 180));

  return (
    <div className="relative" onMouseLeave={() => setHover(null)}>
      <svg width={width} height={chartHeight} role="img" aria-label="Month-end assets and liabilities, last 12 months">
        {ticks.map((t) => (
          <g key={t}>
            <line x1={MARGIN.left} x2={width - MARGIN.right} y1={y(t)} y2={y(t)} stroke={t === 0 ? '#babec5' : GRID} strokeWidth="1" shapeRendering="crispEdges" />
            <text x={MARGIN.left - 6} y={y(t)} dy="0.32em" textAnchor="end" className="fill-qb-ink-muted text-2xs">
              {formatCompactCents(t)}
            </text>
          </g>
        ))}

        {series.map((point, i) => {
          const gx = MARGIN.left + i * groupW;
          const x0 = gx + (groupW - innerW) / 2;
          return (
            <g key={point.month}>
              {hover === i && <rect x={gx} y={MARGIN.top} width={groupW} height={plotH} fill="#f4f5f8" />}
              {SERIES.map((s, j) => {
                const d = barPath(x0 + j * (barW + BAR_GAP), barW, y0, y(point[s.key]));
                return d && <path key={s.key} d={d} fill={s.color} />;
              })}
              {(showEveryLabel || i % 2 === series.length % 2 || i === series.length - 1) && (
                <text x={gx + groupW / 2} y={chartHeight - 6} textAnchor="middle" className="fill-qb-ink-muted text-2xs">
                  {monthLabel(point.month)}
                </text>
              )}
              <rect
                x={gx}
                y={MARGIN.top}
                width={groupW}
                height={plotH + MARGIN.bottom}
                fill="transparent"
                onMouseEnter={() => setHover(i)}
              />
            </g>
          );
        })}
      </svg>

      {hovered && (
        <div
          className="pointer-events-none absolute top-1 z-10 w-[180px] rounded-xs border border-qb-border bg-white px-2 py-1 text-sm shadow-popover"
          style={{ left: tooltipLeft }}
        >
          <div className="mb-0.5 font-semibold">{monthLabel(hovered.month, true)}</div>
          {SERIES.map((s) => (
            <div key={s.key} className="flex items-center gap-1.5">
              <span className="h-2 w-2 flex-none rounded-[2px]" style={{ backgroundColor: s.color }} />
              <span className="flex-1 text-qb-ink-muted">{s.label}</span>
              <span className="num">{formatCents(hovered[s.key])}</span>
            </div>
          ))}
          <div className="mt-0.5 flex border-t border-qb-border pt-0.5">
            <span className="flex-1 text-qb-ink-muted">Net</span>
            <span className="num font-semibold">{formatCents(hovered.assetsCents - hovered.liabilitiesCents)}</span>
          </div>
        </div>
      )}
    </div>
  );
}

function SeriesTable({ series }) {
  return (
    <table className="qb-grid">
      <thead>
        <tr>
          <th className="qb-grid-head">Month end</th>
          <th className="qb-grid-head text-right">Assets</th>
          <th className="qb-grid-head text-right">Liabilities</th>
          <th className="qb-grid-head text-right">Net</th>
        </tr>
      </thead>
      <tbody>
        {series.map((p) => (
          <tr key={p.month} className="qb-grid-row">
            <td className="qb-grid-cell">{monthLabel(p.month, true)}</td>
            <td className="qb-grid-cell num">{formatCents(p.assetsCents)}</td>
            <td className="qb-grid-cell num">{formatCents(p.liabilitiesCents)}</td>
            <td className="qb-grid-cell num">{formatCents(p.assetsCents - p.liabilitiesCents)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export default function AssetLiabilityChart({ series }) {
  const [view, setView] = useState('chart');
  const [ref, size] = useSize();
  const latest = series[series.length - 1];

  return (
    <section className="qb-panel flex min-h-0 flex-col">
      <div className="flex h-row-lg flex-none items-center gap-3 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Assets vs. Liabilities</h2>
        <span className="text-xs text-qb-ink-muted">Month-end, last 12 months</span>
        <div className="ml-auto flex overflow-hidden rounded-xs border border-qb-border-strong text-xs" role="group" aria-label="View">
          {['chart', 'table'].map((v) => (
            <button
              key={v}
              type="button"
              aria-pressed={view === v}
              onClick={() => setView(v)}
              className={`px-2 py-0.5 capitalize ${view === v ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}
            >
              {v}
            </button>
          ))}
        </div>
      </div>

      <div className="flex flex-none items-center gap-4 px-2 py-1 text-sm">
        {SERIES.map((s) => (
          <span key={s.key} className="flex items-center gap-1.5">
            <span className="h-2.5 w-2.5 rounded-[2px]" style={{ backgroundColor: s.color }} />
            <span className="text-qb-ink-muted">{s.label}</span>
            <span className="num font-semibold">{latest ? formatCents(latest[s.key]) : '—'}</span>
          </span>
        ))}
      </div>

      <div ref={ref} className="min-h-0 flex-1 overflow-auto px-1 pb-1 scrollbar-thin">
        {view === 'chart' ? (
          size.width > 0 && <Chart series={series} width={size.width} height={size.height - 4} />
        ) : (
          <SeriesTable series={series} />
        )}
      </div>
    </section>
  );
}
