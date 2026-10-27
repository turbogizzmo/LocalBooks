import { useEffect, useRef, useState } from 'react';
import Notice from './Notice.jsx';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate, tableExports } from './ReportParts.jsx';
import { fiscalMonthLabel, usePeriod } from './MoreReports.jsx';
import { barPath, niceStep, useSize } from './AssetLiabilityChart.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatCompactCents } from '../lib/money.js';
import { formatDate, monthLabel, todayIso } from '../lib/dates.js';

/**
 * The graph reports (`graphs.rs`) in the Reports menu's Graphs group: Income and Expense, Net
 * Worth, A/R and A/P aging, Sales, and Budget vs. Actual. Every number comes from the matching text
 * report, so clicking a bar or slice opens that report (or the account's register) for the same
 * dates. Each graph has a table view; the charts are plain SVG drawn like the Dashboard's.
 */

/** Reports menu entries: [menu id, title]. */
export const GRAPH_REPORTS = [
  ['graph-ie', 'Income and Expense Graph'],
  ['graph-nw', 'Net Worth Graph'],
  ['graph-ar', 'Accounts Receivable Graph'],
  ['graph-ap', 'Accounts Payable Graph'],
  ['graph-sales', 'Sales Graph'],
  ['graph-bva', 'Budget vs. Actual Graph'],
];

export const isGraphReport = (id) => GRAPH_REPORTS.some(([g]) => g === id);

// Chart colours are explicit (not CSS classes) so a saved picture or a printout looks the same.
// The two series colours are the Dashboard's validated pair (≥ 3:1 on light and dark surfaces,
// ΔE 26.7 under protanopia); slices take a fixed categorical order, with grey for "Other".
const INK = '#393a3d';
const MUTED = '#6b6c72';
const GRID = '#e4e6ea';
const AXIS = '#babec5';
const HOVER = '#f4f5f8';
const GREEN = '#2ca01c';
const BLUE = '#0077c5';
const SLICES = ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#6250d6'];
const OTHER = '#8d9096';
const FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", "Helvetica Neue", Helvetica, Arial, sans-serif';

const sliceColor = (slice, i) => (slice.other ? OTHER : SLICES[i % SLICES.length]);
const pct = (part, whole) => (whole ? `${((part / whole) * 100).toFixed(1)}%` : '');
const truncate = (text, max) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

/** Activates a focusable chart mark with Enter or Space, as a button would. */
const keyActivate = (fn) => (e) => {
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    fn();
  }
};

/** "Jan" within one calendar year, "Jan 25" when the range spans years. */
function monthLabeller(months) {
  const years = new Set(months.map((m) => m.slice(0, 4)));
  return (ym) => (years.size > 1 ? `${monthLabel(ym)} ${ym.slice(2, 4)}` : monthLabel(ym));
}

function Legend({ items, x, y }) {
  let cx = x;
  return items.map((item) => {
    const at = cx;
    cx += 26 + item.label.length * 6.4;
    return (
      <g key={item.label} transform={`translate(${at},${y})`}>
        {item.line ? (
          <>
            <line x1="0" x2="14" y1="0" y2="0" stroke={item.color} strokeWidth="2" />
            <circle cx="7" cy="0" r="3.5" fill={item.color} stroke="#fff" strokeWidth="1.5" />
          </>
        ) : (
          <rect x="0" y="-5" width="10" height="10" rx="2" fill={item.color} />
        )}
        <text x={item.line ? 19 : 15} y="0" dy="0.32em" fontSize="12" fill={INK}>
          {item.label}
        </text>
      </g>
    );
  });
}

/**
 * Grouped columns by category (month or aging bucket), with an optional line on the same axis.
 * `series` are { label, color, values }; `line` is { label, color, values }; `onSelect(i)` opens
 * the category's report.
 */
function BarChart({ width, height = 260, categories, series, line = null, onSelect, label, selectHint }) {
  const [hover, setHover] = useState(null);
  const margin = { top: 30, right: 12, bottom: 22, left: 60 };
  const plotW = Math.max(0, width - margin.left - margin.right);
  const plotH = height - margin.top - margin.bottom;
  const values = [...series.flatMap((s) => s.values), ...(line?.values ?? [])];
  let lo = Math.min(0, ...values);
  let hi = Math.max(0, ...values);
  if (hi === lo) hi = lo + 100_000;
  const step = niceStep((hi - lo) / 4);
  const yMin = Math.floor(lo / step) * step;
  const yMax = Math.ceil(hi / step) * step;
  const y = (v) => margin.top + ((yMax - v) / (yMax - yMin)) * plotH;
  const ticks = [];
  for (let t = yMin; t <= yMax + step / 2; t += step) ticks.push(t);

  const n = categories.length || 1;
  const groupW = plotW / n;
  const gap = 2;
  const barW = Math.max(2, Math.min(28, (groupW - 10 - gap * (series.length - 1)) / series.length));
  const innerW = barW * series.length + gap * (series.length - 1);
  const everyLabel = groupW >= 38;
  const y0 = y(0);
  const cx = (i) => margin.left + groupW * (i + 0.5);
  const describe = (i) =>
    `${categories[i].title}: ${series.map((s) => `${s.label} ${formatCents(s.values[i])}`).join(', ')}${line ? `, ${line.label} ${formatCents(line.values[i])}` : ''}`;
  const tipLeft = hover === null ? 0 : Math.min(Math.max(cx(hover) - 100, 0), Math.max(0, width - 200));

  return (
    <div className="relative" onMouseLeave={() => setHover(null)}>
      <svg data-graph width={width} height={height} viewBox={`0 0 ${width} ${height}`} xmlns="http://www.w3.org/2000/svg" fontFamily={FONT} role="group" aria-label={label}>
        <Legend items={[...series, ...(line ? [{ ...line, line: true }] : [])]} x={margin.left} y={12} />
        {ticks.map((t) => (
          <g key={t}>
            <line x1={margin.left} x2={width - margin.right} y1={y(t)} y2={y(t)} stroke={t === 0 ? AXIS : GRID} strokeWidth="1" shapeRendering="crispEdges" />
            <text x={margin.left - 6} y={y(t)} dy="0.32em" textAnchor="end" fontSize="11" fill={MUTED}>
              {formatCompactCents(t)}
            </text>
          </g>
        ))}
        {categories.map((cat, i) => {
          const gx = margin.left + i * groupW;
          const x0 = gx + (groupW - innerW) / 2;
          return (
            <g key={cat.key}>
              {hover === i && <rect x={gx} y={margin.top} width={groupW} height={plotH} fill={HOVER} />}
              {series.map((s, j) => {
                const d = barPath(x0 + j * (barW + gap), barW, y0, y(s.values[i]));
                return d && <path key={s.label} d={d} fill={s.color} />;
              })}
              {(everyLabel || i % 2 === 0 || i === n - 1) && (
                <text x={gx + groupW / 2} y={height - 6} textAnchor="middle" fontSize="11" fill={MUTED}>
                  {cat.label}
                </text>
              )}
            </g>
          );
        })}
        {line && (
          <>
            <path d={line.values.map((v, i) => `${i ? 'L' : 'M'}${cx(i)},${y(v)}`).join('')} fill="none" stroke={line.color} strokeWidth="2" strokeLinejoin="round" />
            {line.values.map((v, i) => (
              <circle key={categories[i].key} cx={cx(i)} cy={y(v)} r="4" fill={line.color} stroke="#fff" strokeWidth="2" />
            ))}
          </>
        )}
        {categories.map((cat, i) => (
          <rect
            key={cat.key}
            x={margin.left + i * groupW}
            y={margin.top}
            width={groupW}
            height={plotH + margin.bottom}
            fill="transparent"
            role="button"
            tabIndex={0}
            aria-label={`${describe(i)}. ${selectHint}`}
            className="cursor-pointer outline-none focus-visible:stroke-qb-info"
            strokeWidth="2"
            onMouseEnter={() => setHover(i)}
            onFocus={() => setHover(i)}
            onBlur={() => setHover(null)}
            onClick={() => onSelect(i)}
            onKeyDown={keyActivate(() => onSelect(i))}
          />
        ))}
      </svg>
      {hover !== null && categories[hover] && (
        <div className="pointer-events-none absolute top-6 z-10 w-[200px] rounded-xs border border-qb-border bg-white px-2 py-1 text-sm shadow-popover" style={{ left: tipLeft }}>
          <div className="mb-0.5 font-semibold">{categories[hover].title}</div>
          {[...series, ...(line ? [line] : [])].map((s) => (
            <div key={s.label} className="flex items-center gap-1.5">
              <span className="h-2 w-2 flex-none rounded-[2px]" style={{ backgroundColor: s.color }} />
              <span className="flex-1 text-qb-ink-muted">{s.label}</span>
              <span className="num">{formatCents(s.values[hover])}</span>
            </div>
          ))}
          <div className="mt-0.5 border-t border-qb-border pt-0.5 text-xs text-qb-ink-muted">{selectHint}</div>
        </div>
      )}
    </div>
  );
}

function arcPath(cx, cy, r, inner, a0, a1) {
  const p = (radius, a) => `${cx + radius * Math.sin(a)},${cy - radius * Math.cos(a)}`;
  const large = a1 - a0 > Math.PI ? 1 : 0;
  return `M${p(r, a0)}A${r},${r} 0 ${large} 1 ${p(r, a1)}L${p(inner, a1)}A${inner},${inner} 0 ${large} 0 ${p(inner, a0)}Z`;
}

/**
 * A donut of the positive slices with a legend (name, amount, share) beside it, or beneath it when
 * narrow. Negative amounts (a refund larger than the income) are listed in the table view only.
 */
function PieChart({ width, slices, onSelect, label, selectHint }) {
  const [hover, setHover] = useState(null);
  const shown = slices.filter((s) => s.amountCents > 0);
  const total = shown.reduce((t, s) => t + s.amountCents, 0);
  const size = 200;
  const beside = width >= 520;
  const rowH = 22;
  const legendX = beside ? size + 32 : 8;
  const legendY = beside ? Math.max(12, size / 2 - (shown.length * rowH) / 2 + 6) : size + 24;
  const height = beside ? Math.max(size + 16, legendY + shown.length * rowH) : legendY + shown.length * rowH;
  const legendW = Math.max(200, Math.min(460, width - legendX - 8));
  const r = size / 2 - 4;
  const [cx, cy] = [beside ? size / 2 + 8 : width / 2, size / 2 + 8];
  let angle = 0;
  const arcs = shown.map((s, i) => {
    const a0 = angle;
    angle += (s.amountCents / total) * Math.PI * 2;
    return { slice: s, i, a0, a1: angle };
  });
  const maxChars = Math.max(10, Math.floor((legendW - 160) / 6.4));

  if (total <= 0) return <p className="px-2 py-6 text-center text-sm text-qb-ink-muted">Nothing to show for this period.</p>;
  return (
    <svg data-graph width={width} height={height} viewBox={`0 0 ${width} ${height}`} xmlns="http://www.w3.org/2000/svg" fontFamily={FONT} role="group" aria-label={label} onMouseLeave={() => setHover(null)}>
      {arcs.map(({ slice, i, a0, a1 }) => {
        const describe = `${slice.label}: ${formatCents(slice.amountCents)}, ${pct(slice.amountCents, total)}. ${selectHint}`;
        const common = {
          fill: sliceColor(slice, i),
          stroke: '#fff',
          strokeWidth: 2,
          role: 'button',
          tabIndex: 0,
          'aria-label': describe,
          className: 'cursor-pointer outline-none',
          opacity: hover === null || hover === i ? 1 : 0.55,
          onMouseEnter: () => setHover(i),
          onFocus: () => setHover(i),
          onBlur: () => setHover(null),
          onClick: () => onSelect(slice),
          onKeyDown: keyActivate(() => onSelect(slice)),
        };
        return arcs.length === 1 ? (
          <g key={slice.label}>
            <circle cx={cx} cy={cy} r={(r + r * 0.58) / 2} fill="none" stroke={sliceColor(slice, i)} strokeWidth={r * 0.42} />
            <circle cx={cx} cy={cy} r={r} {...common} fillOpacity="0" stroke="none" />
          </g>
        ) : (
          <path key={slice.label} d={arcPath(cx, cy, r, r * 0.58, a0, a1)} {...common} />
        );
      })}
      <text x={cx} y={cy - 7} textAnchor="middle" fontSize="11" fill={MUTED}>
        {hover === null ? 'Total' : truncate(shown[hover].label, 22)}
      </text>
      <text x={cx} y={cy + 10} textAnchor="middle" fontSize="14" fontWeight="600" fill={INK}>
        {formatCents(hover === null ? total : shown[hover].amountCents)}
      </text>
      {shown.map((s, i) => (
        <g
          key={s.label}
          transform={`translate(${legendX},${legendY + i * rowH})`}
          className="cursor-pointer"
          onMouseEnter={() => setHover(i)}
          onClick={() => onSelect(s)}
          aria-hidden="true"
        >
          <rect x="0" y="-11" width={legendW} height={rowH - 2} fill={hover === i ? HOVER : 'transparent'} />
          <rect x="4" y="-5" width="10" height="10" rx="2" fill={sliceColor(s, i)} />
          <text x="20" y="0" dy="0.32em" fontSize="12" fill={INK}>
            {truncate(s.label, maxChars)}
          </text>
          <text x={legendW - 52} y="0" dy="0.32em" textAnchor="end" fontSize="12" fill={INK}>
            {formatCents(s.amountCents)}
          </text>
          <text x={legendW - 4} y="0" dy="0.32em" textAnchor="end" fontSize="12" fill={MUTED}>
            {pct(s.amountCents, total)}
          </text>
        </g>
      ))}
    </svg>
  );
}

/** Horizontal bars, largest first: the top customers or vendors by balance. */
function RankChart({ width, items, color, onSelect, label, selectHint }) {
  const [hover, setHover] = useState(null);
  const rowH = 26;
  const labelW = Math.min(200, Math.max(110, width * 0.32));
  const valueW = 100;
  const plotW = Math.max(40, width - labelW - valueW - 16);
  const max = Math.max(1, ...items.map((s) => s.amountCents));
  const height = items.length * rowH + 8;
  if (items.length === 0) return <p className="px-2 py-6 text-center text-sm text-qb-ink-muted">No open balances.</p>;
  return (
    <svg data-graph width={width} height={height} viewBox={`0 0 ${width} ${height}`} xmlns="http://www.w3.org/2000/svg" fontFamily={FONT} role="group" aria-label={label} onMouseLeave={() => setHover(null)}>
      {items.map((s, i) => {
        const w = Math.max(1, (s.amountCents / max) * plotW);
        const top = 4 + i * rowH;
        return (
          <g
            key={s.label}
            role="button"
            tabIndex={0}
            aria-label={`${s.label}: ${formatCents(s.amountCents)}. ${selectHint}`}
            className="cursor-pointer outline-none"
            onMouseEnter={() => setHover(i)}
            onFocus={() => setHover(i)}
            onBlur={() => setHover(null)}
            onClick={() => onSelect(s)}
            onKeyDown={keyActivate(() => onSelect(s))}
          >
            <rect x="0" y={top} width={width} height={rowH} fill={hover === i ? HOVER : 'transparent'} />
            <text x={labelW - 8} y={top + rowH / 2} dy="0.32em" textAnchor="end" fontSize="12" fill={INK}>
              {truncate(s.label, Math.floor(labelW / 6.8))}
            </text>
            <path d={`M${labelW},${top + 5}H${labelW + w - Math.min(4, w)}Q${labelW + w},${top + 5} ${labelW + w},${top + 5 + Math.min(4, w)}V${top + rowH - 5 - Math.min(4, w)}Q${labelW + w},${top + rowH - 5} ${labelW + w - Math.min(4, w)},${top + rowH - 5}H${labelW}Z`} fill={s.other ? OTHER : color} />
            <text x={labelW + w + 6} y={top + rowH / 2} dy="0.32em" fontSize="12" fill={INK}>
              {formatCents(s.amountCents)}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

/** One data table of a graph, for the table view and the printout. */
function DataTable({ table, print = false }) {
  const money = (c) => table.columns[c].kind === 'money';
  return (
    <table className={print ? 'doc-table' : 'qb-grid w-full'}>
      {table.caption && <caption className={print ? 'doc-subtitle' : 'px-2 py-1 text-left text-sm font-semibold'}>{table.caption}</caption>}
      <thead>
        <tr>
          {table.columns.map((col, c) => (
            <th key={col.label} className={`${print ? '' : 'qb-grid-head'} ${money(c) ? 'text-right' : 'text-left'}`}>
              {col.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {table.rows.map((row, r) => (
          <tr key={r} className={print ? '' : `qb-grid-row ${row.kind === 'total' ? 'font-semibold' : ''}`}>
            {row.cells.map((v, c) => (
              <td key={c} className={`${print ? '' : 'qb-grid-cell'} ${money(c) ? 'num text-right' : ''}`}>
                {v === null || v === undefined ? '' : money(c) ? formatCents(v) : v}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Draws the sheet's title and its SVG charts onto a canvas and saves it as a PNG through the Save dialog. */
async function savePicture(node, title, subtitle, filename) {
  const svgs = [...node.querySelectorAll('svg[data-graph]')];
  if (svgs.length === 0) throw new Error('GRAPH_EMPTY: switch to the chart view to save a picture');
  const box = node.getBoundingClientRect();
  const firstTop = Math.min(...svgs.map((s) => s.getBoundingClientRect().top));
  const header = 64;
  const bottom = Math.max(...svgs.map((s) => s.getBoundingClientRect().bottom));
  const [w, h, scale] = [Math.ceil(box.width), Math.ceil(header + bottom - firstTop + 16), 2];
  const canvas = document.createElement('canvas');
  canvas.width = w * scale;
  canvas.height = h * scale;
  const ctx = canvas.getContext('2d');
  ctx.scale(scale, scale);
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, w, h);
  ctx.fillStyle = INK;
  ctx.font = `600 18px ${FONT}`;
  ctx.fillText(title, 24, 30);
  ctx.fillStyle = MUTED;
  ctx.font = `13px ${FONT}`;
  ctx.fillText(subtitle, 24, 50);
  for (const svg of svgs) {
    const rect = svg.getBoundingClientRect();
    // The picture shows the chart at rest: no hover band, no dimmed slices.
    const copy = svg.cloneNode(true);
    copy.querySelectorAll(`[fill="${HOVER}"]`).forEach((n) => n.setAttribute('fill', 'transparent'));
    copy.querySelectorAll('[opacity]').forEach((n) => n.removeAttribute('opacity'));
    const xml = new XMLSerializer().serializeToString(copy);
    const img = new Image();
    img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(xml)}`;
    await img.decode();
    ctx.drawImage(img, rect.left - box.left, header + rect.top - firstTop, rect.width, rect.height);
  }
  const base64 = canvas.toDataURL('image/png').split(',')[1];
  return api.savePng(base64, filename);
}

/**
 * The frame every graph shares: its controls, Chart/Table switch and Export menu, the on-screen
 * sheet, and the printout. `charts(width)` draws the charts at a width; `tables` are the data.
 */
function GraphFrame({ title, subtitle, controls, error, empty, charts, tables, filename, footer }) {
  const [view, setView] = useState('chart');
  const [notice, setNotice] = useState(null);
  const [ref, size] = useSize();
  const sheet = useRef(null);
  const company = useCompany();
  const ready = Boolean(tables);
  // Later tables go under a heading, in the first table's columns: a cell whose kind differs there
  // (a "% of total" beside a money column) is left out, so every exported amount stays a number.
  const exportRows = () => {
    const columns = tables[0].columns;
    return tables.flatMap((t, i) => [
      ...(i > 0 ? [{ kind: 'section', depth: 0, cells: [t.caption, ...columns.slice(1).map(() => null)] }] : []),
      ...t.rows.map((row) => ({ kind: row.kind ?? 'detail', depth: 0, cells: columns.map((col, c) => (t.columns[c]?.kind === col.kind ? row.cells[c] : null)) })),
    ]);
  };
  const exports = ready
    ? tableExports({ company: company.name, title, subtitle, columns: tables[0].columns, rows: exportRows, filename, footer, onNotice: setNotice })
    : null;

  return (
    <>
      <div className="qb-panel no-print flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        {controls}
        <div className="ml-auto flex items-center gap-2 self-center">
          <div className="flex overflow-hidden rounded-xs border border-qb-border-strong text-xs" role="group" aria-label="View">
            {['chart', 'table'].map((v) => (
              <button key={v} type="button" aria-pressed={view === v} onClick={() => setView(v)} className={`px-2 py-0.5 capitalize ${view === v ? 'bg-qb-nav text-white' : 'bg-white hover:bg-qb-row-hover'}`}>
                {v}
              </button>
            ))}
          </div>
          {ready && (
            <ExportMenu
              onNotice={setNotice}
              onCsv={exports.onCsv}
              onXlsx={exports.onXlsx}
              onPng={async () => {
                if (view !== 'chart') throw new Error('GRAPH_EMPTY: switch to the chart view to save a picture');
                const path = await savePicture(sheet.current, title, subtitle, `${filename}.png`);
                if (path) setNotice({ kind: 'success', text: `Saved ${path}` });
              }}
            />
          )}
        </div>
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {empty && <Notice kind="info" text={empty} />}
      <article ref={sheet} className="mx-auto w-full max-w-[1100px] bg-white px-6 py-4 shadow-panel">
        <header className="mb-3 border-b border-qb-border pb-2">
          <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
          <h2 className="text-xl font-semibold">{title}</h2>
          <div className="text-sm text-qb-ink-muted">{subtitle}</div>
        </header>
        <div ref={ref} className="min-w-0">
          {ready && (view === 'chart' ? size.width > 0 && charts(size.width) : tables.map((t) => <DataTable key={t.caption ?? 'main'} table={t} />))}
        </div>
        {footer && <footer className="mt-3 border-t border-qb-border pt-1 text-xs text-qb-ink-muted">{footer}</footer>}
      </article>
      {ready && (
        <PrintPortal>
          <article className="doc">
            <DocHeader company={company} title={title} subtitle={subtitle} />
            {charts(690, true)}
            {tables.map((t) => (
              <DataTable key={t.caption ?? 'main'} table={t} print />
            ))}
            {footer && <p className="doc-note">{footer}</p>}
          </article>
        </PrintPortal>
      )}
    </>
  );
}

function Section({ title, children }) {
  return (
    <section className="mt-3 first:mt-0">
      <h3 className="mb-1 text-sm font-semibold text-qb-ink">{title}</h3>
      {children}
    </section>
  );
}

const rangeSubtitle = (start, end) => `${longDate(start)} – ${longDate(end)}`;
const usePeriodGraph = (fetch, deps) => {
  const q = useLedgerQuery(fetch);
  useEffect(() => {
    q.reload();
  }, [...deps, q.reload]); // eslint-disable-line react-hooks/exhaustive-deps
  return q;
};

function IncomeExpenseGraph({ navigate, onOpenReport }) {
  const p = usePeriod('fiscalYear');
  const [basis, setBasis] = useState('ACCRUAL');
  const [pie, setPie] = useState('expense');
  const { data, error } = usePeriodGraph(() => (p.valid ? api.getIncomeExpenseGraph(p.start, p.end, basis) : Promise.resolve(null)), [p.start, p.end, basis]);
  const label = data ? monthLabeller(data.months.map((m) => m.month)) : null;
  const slices = data ? (pie === 'expense' ? data.expenseSlices : data.incomeSlices) : [];
  const openSlice = (s) =>
    s.accountId == null
      ? onOpenReport('pl', { startDate: data.startDate, endDate: data.endDate, basis })
      : navigate?.('accounting/register', 'open', { accountId: s.accountId, startDate: data.startDate, endDate: data.endDate, includeSubaccounts: true });
  const basisText = basis === 'CASH' ? 'Cash basis' : 'Accrual basis';
  return (
    <GraphFrame
      title="Income and Expense"
      subtitle={data ? `${rangeSubtitle(data.startDate, data.endDate)} · ${basisText}` : ''}
      filename={`Income-and-Expense-Graph_${p.start}_to_${p.end}`}
      error={error}
      controls={
        <>
          {p.controls}
          <label>
            <span className="qb-label">Basis</span>
            <select className="qb-input w-[100px]" value={basis} onChange={(e) => setBasis(e.target.value)}>
              <option value="ACCRUAL">Accrual</option>
              <option value="CASH">Cash</option>
            </select>
          </label>
          <label>
            <span className="qb-label">Pie shows</span>
            <select className="qb-input w-[150px]" value={pie} onChange={(e) => setPie(e.target.value)}>
              <option value="expense">Expense by account</option>
              <option value="income">Income by account</option>
            </select>
          </label>
        </>
      }
      footer={`${basisText} · the same totals as Profit and Loss · expenses include cost of goods sold and other expenses`}
      tables={
        data && [
          {
            columns: [{ label: 'Month', kind: 'text' }, { label: 'Income', kind: 'money' }, { label: 'Expense', kind: 'money' }, { label: 'Net income', kind: 'money' }],
            rows: [
              ...data.months.map((m) => ({ cells: [monthLabel(m.month, true), m.incomeCents, m.expenseCents, m.netIncomeCents] })),
              { kind: 'total', cells: ['Total', data.totalIncomeCents, data.totalExpenseCents, data.netIncomeCents] },
            ],
          },
          {
            caption: pie === 'expense' ? 'Expense by account' : 'Income by account',
            columns: [{ label: 'Account', kind: 'text' }, { label: 'Amount', kind: 'money' }, { label: '% of total', kind: 'text' }],
            rows: slices.map((s) => ({ cells: [s.label, s.amountCents, pct(s.amountCents, pie === 'expense' ? data.totalExpenseCents : data.totalIncomeCents)] })),
          },
        ]
      }
      charts={(width) => (
        <>
          <Section title="By month">
            <BarChart
              width={width}
              categories={data.months.map((m) => ({ key: m.month, label: label(m.month), title: monthLabel(m.month, true) }))}
              series={[
                { label: 'Income', color: GREEN, values: data.months.map((m) => m.incomeCents) },
                { label: 'Expense', color: BLUE, values: data.months.map((m) => m.expenseCents) },
              ]}
              label={`Income and expense by month, ${rangeSubtitle(data.startDate, data.endDate)}`}
              selectHint="Opens Profit and Loss for the month"
              onSelect={(i) => onOpenReport('pl', { startDate: data.months[i].startDate, endDate: data.months[i].endDate, basis })}
            />
          </Section>
          <Section title={pie === 'expense' ? `Expense by account · ${formatCents(data.totalExpenseCents)}` : `Income by account · ${formatCents(data.totalIncomeCents)}`}>
            <PieChart width={width} slices={slices} label={pie === 'expense' ? 'Expense by account' : 'Income by account'} selectHint="Opens the account register" onSelect={openSlice} />
          </Section>
        </>
      )}
    />
  );
}

function NetWorthGraph({ onOpenReport }) {
  const p = usePeriod('fiscalYear');
  const { data, error } = usePeriodGraph(() => (p.valid ? api.getNetWorthGraph(p.start, p.end) : Promise.resolve(null)), [p.start, p.end]);
  const label = data ? monthLabeller(data.months.map((m) => m.month)) : null;
  return (
    <GraphFrame
      title="Net Worth"
      subtitle={data ? `Month ends, ${rangeSubtitle(data.startDate, data.endDate)}` : ''}
      filename={`Net-Worth-Graph_${p.start}_to_${p.end}`}
      error={error}
      controls={p.controls}
      footer="Total assets and total liabilities from the Balance Sheet at each month end · net worth is assets less liabilities"
      tables={
        data && [
          {
            columns: [{ label: 'As of', kind: 'text' }, { label: 'Assets', kind: 'money' }, { label: 'Liabilities', kind: 'money' }, { label: 'Net worth', kind: 'money' }],
            rows: data.months.map((m) => ({ cells: [formatDate(m.asOf), m.assetsCents, m.liabilitiesCents, m.netWorthCents] })),
          },
        ]
      }
      charts={(width) => (
        <BarChart
          width={width}
          height={300}
          categories={data.months.map((m) => ({ key: m.month, label: label(m.month), title: `As of ${longDate(m.asOf)}` }))}
          series={[
            { label: 'Assets', color: GREEN, values: data.months.map((m) => m.assetsCents) },
            { label: 'Liabilities', color: BLUE, values: data.months.map((m) => m.liabilitiesCents) },
          ]}
          line={{ label: 'Net worth', color: INK, values: data.months.map((m) => m.netWorthCents) }}
          label={`Assets, liabilities and net worth at each month end, ${rangeSubtitle(data.startDate, data.endDate)}`}
          selectHint="Opens the Balance Sheet as of this date"
          onSelect={(i) => onOpenReport('bs', { asOf: data.months[i].asOf })}
        />
      )}
    />
  );
}

function AgingGraph({ payable, onOpenReport }) {
  const [asOf, setAsOf] = useState(todayIso());
  const { data, error } = usePeriodGraph(() => (asOf ? api.getAgingGraph(payable, asOf) : Promise.resolve(null)), [asOf]);
  const who = payable ? 'Vendor' : 'Customer';
  const report = payable ? 'ap' : 'ar';
  const reportName = payable ? 'A/P Aging Summary' : 'A/R Aging Summary';
  const open = () => onOpenReport(report, { asOf: data.asOf });
  return (
    <GraphFrame
      title={payable ? 'Accounts Payable' : 'Accounts Receivable'}
      subtitle={data ? `As of ${longDate(data.asOf)} · ${formatCents(data.totalCents)} ${payable ? 'owed to vendors' : 'owed by customers'}` : ''}
      filename={`${payable ? 'Accounts-Payable' : 'Accounts-Receivable'}-Graph_${asOf}`}
      error={error}
      controls={
        <label>
          <span className="qb-label">As of</span>
          <input type="date" className="qb-input w-[132px]" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
        </label>
      }
      footer={`The same balances as the ${reportName} · days past due · credit not yet used is subtracted in the column for its age`}
      tables={
        data && [
          {
            columns: [{ label: 'Days past due', kind: 'text' }, { label: 'Amount', kind: 'money' }],
            rows: [...data.buckets.map((b) => ({ cells: [b.label, b.amountCents] })), { kind: 'total', cells: ['Total', data.totalCents] }],
          },
          {
            caption: payable ? 'Vendors by balance' : 'Customers by balance',
            columns: [{ label: who, kind: 'text' }, { label: 'Balance', kind: 'money' }],
            rows: data.parties.map((s) => ({ cells: [s.label, s.amountCents] })),
          },
        ]
      }
      charts={(width) => (
        <>
          <Section title="By days past due">
            <BarChart
              width={width}
              height={230}
              categories={data.buckets.map((b) => ({ key: b.label, label: b.label, title: b.label === 'Current' ? 'Current (not yet due)' : `${b.label} days past due` }))}
              series={[{ label: payable ? 'Owed to vendors' : 'Owed by customers', color: BLUE, values: data.buckets.map((b) => b.amountCents) }]}
              label={`${payable ? 'Payables' : 'Receivables'} by days past due as of ${longDate(data.asOf)}`}
              selectHint={`Opens the ${reportName}`}
              onSelect={open}
            />
          </Section>
          <Section title={payable ? 'Vendors with the largest balances' : 'Customers with the largest balances'}>
            <RankChart width={width} items={data.parties} color={GREEN} label={payable ? 'Vendors by balance' : 'Customers by balance'} selectHint={`Opens the ${reportName}`} onSelect={open} />
          </Section>
        </>
      )}
    />
  );
}

const SALES_BY = {
  customer: { report: 'sales-customer', title: 'Customer', name: 'Sales by Customer Summary' },
  item: { report: 'sales-product', title: 'Product/Service', name: 'Sales by Product/Service Summary' },
  rep: { report: 'sales-rep', title: 'Rep', name: 'Sales by Rep Summary' },
};

function SalesGraph({ onOpenReport }) {
  const p = usePeriod('fiscalYear');
  const [by, setBy] = useState('customer');
  const cfg = SALES_BY[by];
  const { data, error } = usePeriodGraph(() => (p.valid ? api.getSalesGraph(by, p.start, p.end) : Promise.resolve(null)), [p.start, p.end, by]);
  const label = data ? monthLabeller(data.months.map((m) => m.month)) : null;
  return (
    <GraphFrame
      title="Sales"
      subtitle={data ? `${rangeSubtitle(data.startDate, data.endDate)} · by ${cfg.title.toLowerCase()}` : ''}
      filename={`Sales-Graph_${p.start}_to_${p.end}`}
      error={error}
      controls={
        <>
          {p.controls}
          <label>
            <span className="qb-label">By</span>
            <select className="qb-input w-[150px]" value={by} onChange={(e) => setBy(e.target.value)}>
              <option value="customer">Customer</option>
              <option value="item">Product/Service</option>
              <option value="rep">Rep</option>
            </select>
          </label>
        </>
      }
      footer={`The same totals as the ${cfg.name}`}
      tables={
        data && [
          {
            columns: [{ label: 'Month', kind: 'text' }, { label: 'Sales', kind: 'money' }],
            rows: [...data.months.map((m) => ({ cells: [monthLabel(m.month, true), m.amountCents] })), { kind: 'total', cells: ['Total', data.totalCents] }],
          },
          {
            caption: `Sales by ${cfg.title.toLowerCase()}`,
            columns: [{ label: cfg.title, kind: 'text' }, { label: 'Sales', kind: 'money' }, { label: '% of total', kind: 'text' }],
            rows: data.top.map((s) => ({ cells: [s.label, s.amountCents, pct(s.amountCents, data.totalCents)] })),
          },
        ]
      }
      charts={(width) => (
        <>
          <Section title={`By month · ${formatCents(data.totalCents)}`}>
            <BarChart
              width={width}
              categories={data.months.map((m) => ({ key: m.month, label: label(m.month), title: monthLabel(m.month, true) }))}
              series={[{ label: 'Sales', color: GREEN, values: data.months.map((m) => m.amountCents) }]}
              label={`Sales by month, ${rangeSubtitle(data.startDate, data.endDate)}`}
              selectHint={`Opens the ${cfg.name} for the month`}
              onSelect={(i) => onOpenReport(cfg.report, { startDate: data.months[i].startDate, endDate: data.months[i].endDate })}
            />
          </Section>
          <Section title={`Top by ${cfg.title.toLowerCase()}`}>
            <PieChart
              width={width}
              slices={data.top}
              label={`Sales by ${cfg.title.toLowerCase()}`}
              selectHint={`Opens the ${cfg.name}`}
              onSelect={() => onOpenReport(cfg.report, { startDate: data.startDate, endDate: data.endDate })}
            />
          </Section>
        </>
      )}
    />
  );
}

const MEASURES = {
  netIncomeCents: 'Net income',
  incomeCents: 'Income',
  expenseCents: 'Expenses',
};

function BudgetGraph({ onOpenReport }) {
  const budgets = useLedgerQuery(api.listBudgets);
  const [budgetId, setBudgetId] = useState(null);
  const [measure, setMeasure] = useState('netIncomeCents');
  const list = budgets.data ?? [];
  const budget = list.find((b) => b.id === budgetId) ?? list[0];
  const { data, error } = usePeriodGraph(() => (budget ? api.getBudgetGraph(budget.id, 1, 12) : Promise.resolve(null)), [budget?.id]);
  const name = MEASURES[measure];
  const title = 'Budget vs. Actual';
  return (
    <GraphFrame
      title={title}
      subtitle={budget && data ? `${budget.name} · ${name} · ${rangeSubtitle(data.months[0].startDate, data.months[data.months.length - 1].endDate)}` : ''}
      filename={`Budget-vs-Actual-Graph_${budget?.name ?? ''}`.replace(/[^\w.-]+/g, '-')}
      error={error}
      empty={budgets.data && list.length === 0 ? 'There are no budgets yet. Create one under Accounting › Budgets, then come back to compare it with actual income and expenses.' : null}
      controls={
        <>
          <label>
            <span className="qb-label">Budget</span>
            <select className="qb-input w-[220px]" value={budget?.id ?? ''} onChange={(e) => setBudgetId(Number(e.target.value))}>
              {list.map((b) => (
                <option key={b.id} value={b.id}>{b.name} (FY from {formatDate(b.fiscalYearStart)})</option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">Show</span>
            <select className="qb-input w-[120px]" value={measure} onChange={(e) => setMeasure(e.target.value)}>
              {Object.entries(MEASURES).map(([k, v]) => (
                <option key={k} value={k}>{v}</option>
              ))}
            </select>
          </label>
        </>
      }
      footer="Accrual basis · the same amounts as Profit and Loss Budget vs. Actual for each month"
      tables={
        data &&
        budget && [
          {
            columns: [{ label: 'Month', kind: 'text' }, { label: `Actual ${name.toLowerCase()}`, kind: 'money' }, { label: `Budget ${name.toLowerCase()}`, kind: 'money' }, { label: 'Over budget', kind: 'money' }],
            rows: [
              ...data.months.map((m) => ({ cells: [fiscalMonthLabel(budget.fiscalYearStart, m.period), m.actual[measure], m.budget[measure], m.actual[measure] - m.budget[measure]] })),
              { kind: 'total', cells: ['Total', data.actual[measure], data.budget[measure], data.actual[measure] - data.budget[measure]] },
            ],
          },
        ]
      }
      charts={(width) => (
        <BarChart
          width={width}
          height={300}
          categories={data.months.map((m) => ({ key: m.month, label: monthLabel(m.month), title: fiscalMonthLabel(budget.fiscalYearStart, m.period) }))}
          series={[
            { label: `Actual ${name.toLowerCase()}`, color: GREEN, values: data.months.map((m) => m.actual[measure]) },
            { label: `Budget ${name.toLowerCase()}`, color: BLUE, values: data.months.map((m) => m.budget[measure]) },
          ]}
          label={`${name}, actual against budget by month, ${budget.name}`}
          selectHint="Opens Profit and Loss Budget vs. Actual for the month"
          onSelect={(i) => onOpenReport('bva', { budgetId: budget.id, fromPeriod: data.months[i].period, toPeriod: data.months[i].period })}
        />
      )}
    />
  );
}

export default function GraphReport({ report, navigate, onOpenReport }) {
  if (report === 'graph-ie') return <IncomeExpenseGraph navigate={navigate} onOpenReport={onOpenReport} />;
  if (report === 'graph-nw') return <NetWorthGraph onOpenReport={onOpenReport} />;
  if (report === 'graph-ar') return <AgingGraph key="ar" payable={false} onOpenReport={onOpenReport} />;
  if (report === 'graph-ap') return <AgingGraph key="ap" payable onOpenReport={onOpenReport} />;
  if (report === 'graph-sales') return <SalesGraph onOpenReport={onOpenReport} />;
  if (report === 'graph-bva') return <BudgetGraph onOpenReport={onOpenReport} />;
  return null;
}
