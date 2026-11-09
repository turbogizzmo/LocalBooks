import { useEffect, useState } from 'react';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import { commentNumbers, cellKey } from '../lib/reportComments.js';
import { DocHeader, PrintPortal, useCompany } from './PrintDocuments.jsx';
import Notice from './Notice.jsx';

const RIGHT = new Set(['money', 'quantity', 'number']);

/** A kept cell as text, by its column's kind (quantities are kept scaled by 10,000). */
export function cellText(kind, v) {
  if (v === null || v === undefined) return '';
  if (kind === 'money') return formatCents(v);
  if (kind === 'quantity') return (v / 10000).toLocaleString('en-US', { maximumFractionDigits: 4 });
  if (kind === 'date') return formatDate(v);
  return String(v);
}

/**
 * Commenting on a report's figures (`report_comments.rs`). `snapshot()` gives the report as shown,
 * in the engine's shape ({ columns, rows }). The first comment keeps that copy under Reports ›
 * Commented Reports; later ones in the same view attach to it.
 *
 * Returns `toggle` (the Comment button), `cell(row, col, figure)` (the figure's number, if
 * commented, and props that open the comment box while commenting) and `editor` (the box itself).
 */
export function useFigureComments({ title, subtitle, snapshot }) {
  const [active, setActive] = useState(false);
  const [kept, setKept] = useState(null);
  const [editing, setEditing] = useState(null);
  const [body, setBody] = useState('');
  const [error, setError] = useState(null);
  // Another report, or other dates, starts a new copy.
  useEffect(() => {
    setKept(null);
    setEditing(null);
  }, [title, subtitle]);
  const numbers = commentNumbers(kept?.comments ?? []);

  const save = async () => {
    try {
      const input = kept
        ? { reportId: kept.id, row: editing.row, col: editing.col, figure: editing.figure, body }
        : { title, subtitle, snapshot: snapshot(), row: editing.row, col: editing.col, figure: editing.figure, body };
      setKept(await api.commentOnReportFigure(input));
      setEditing(null);
      setBody('');
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  };

  const toggle = (
    <button
      type="button"
      className={`no-print qb-btn h-row text-xs ${active ? 'qb-btn-primary' : 'qb-btn-secondary'}`}
      aria-pressed={active}
      onClick={() => {
        setActive(!active);
        setEditing(null);
      }}
      title="Click a figure to comment on it"
    >
      Comment
    </button>
  );

  const cell = (row, col, figure) => {
    const marker = numbers.get(cellKey(row, col)) ?? null;
    if (!active || figure === '' || figure === null || figure === undefined) return { marker, props: {} };
    return {
      marker,
      props: {
        onClick: (e) => {
          e.stopPropagation();
          setEditing({ row, col, figure });
          setBody('');
        },
        title: 'Comment on this figure',
        'data-commentable': 'true',
      },
    };
  };

  const editor = (active || kept) && (
    <div className="no-print mb-2 border border-qb-border bg-qb-canvas px-2 py-1.5 text-sm">
      {active && !editing && <p className="text-xs text-qb-ink-muted">Click a figure to comment on it.</p>}
      {editing && (
        <div className="flex items-end gap-2">
          <label className="flex-1">
            <span className="qb-label">Comment on {editing.figure}</span>
            <input className="qb-input w-full" autoFocus maxLength={2000} value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && body.trim() && save()} />
          </label>
          <button type="button" className="qb-btn qb-btn-primary h-row" disabled={!body.trim()} onClick={save}>Save</button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setEditing(null)}>Cancel</button>
        </div>
      )}
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {kept && (
        <p className="mt-1 text-xs text-qb-ink-muted">
          {kept.comments.length} comment{kept.comments.length === 1 ? '' : 's'} kept with this report under Reports › Commented Reports, where it opens and prints with them.
        </p>
      )}
    </div>
  );

  return { toggle, cell, editor };
}

/** The number beside a commented figure. */
export function Marker({ n }) {
  return n ? <sup className="ml-0.5 font-semibold text-qb-info">[{n}]</sup> : null;
}

/** The comments under a report, numbered as their figures are. */
function CommentList({ comments, onDelete = null }) {
  const numbers = commentNumbers(comments);
  const sorted = [...comments].sort((a, b) => a.rowIndex - b.rowIndex || a.colIndex - b.colIndex || a.id - b.id);
  return (
    <ol className="list-none space-y-1 text-sm">
      {sorted.map((c) => (
        <li key={c.id} className="flex gap-2">
          <span className="w-8 flex-none font-semibold">[{numbers.get(cellKey(c.rowIndex, c.colIndex))}]</span>
          <span className="flex-1">
            <span className="text-qb-ink-muted">
              {c.lineLabel ? `${c.lineLabel}, ` : ''}
              {c.figure}:{' '}
            </span>
            {c.body}
            <span className="text-xs text-qb-ink-muted">
              {' '}
              · {c.author}, {c.createdAt.slice(0, 10)}
            </span>
          </span>
          {onDelete && (
            <button type="button" className="no-print qb-btn qb-btn-secondary h-[20px] px-2 text-xs" onClick={() => onDelete(c)}>
              Remove
            </button>
          )}
        </li>
      ))}
    </ol>
  );
}

/** A kept report with its comments: on screen (click a figure to add one) and in print. */
function CommentedReportView({ id, onBack }) {
  const company = useCompany();
  const { data: report, error, reload } = useLedgerQuery(() => api.getCommentedReport(id));
  const [notice, setNotice] = useState(null);
  const [adding, setAdding] = useState(null);
  const [body, setBody] = useState('');
  if (error) return <Notice kind="error" text={error} />;
  if (!report) return null;
  const { columns, rows } = report.snapshot;
  const numbers = commentNumbers(report.comments);
  const add = async () => {
    try {
      await api.commentOnReportFigure({ reportId: report.id, row: adding.row, col: adding.col, figure: adding.figure, body });
      setAdding(null);
      setBody('');
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const remove = async (c) => {
    try {
      await api.deleteReportComment(c.id);
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  const screenRow = (kind) => (kind === 'section' ? 'font-semibold' : kind === 'detail' || kind === 'account' ? '' : 'font-semibold border-t border-qb-ink');
  const printRow = (kind) => (kind === 'section' ? 'doc-line-section' : kind === 'detail' || kind === 'account' ? '' : 'doc-line-total');
  const tableBody = (print) =>
    rows.map((r, i) => (
      <tr key={i} className={print ? printRow(r.kind) : screenRow(r.kind)}>
        {columns.map((c, j) => {
          const text = cellText(c.kind, r.cells[j]);
          const n = numbers.get(cellKey(i, j));
          const commentable = !print && j > 0 && text !== '';
          const align = print ? (RIGHT.has(c.kind) ? 'doc-num' : '') : `px-1.5 py-0.5 ${RIGHT.has(c.kind) ? 'num text-right' : ''} ${commentable ? 'cursor-cell hover:bg-qb-row-hover' : ''}`;
          return (
            <td
              key={j}
              className={align}
              style={j === 0 && r.depth ? { paddingLeft: `${6 + r.depth * 12}px` } : undefined}
              onClick={commentable ? () => (setAdding({ row: i, col: j, figure: text }), setBody('')) : undefined}
              title={commentable ? 'Comment on this figure' : undefined}
            >
              {text}
              {n ? <sup>[{n}]</sup> : null}
            </td>
          );
        })}
      </tr>
    ));
  return (
    <>
      <PrintPortal>
        <article className="doc">
          <DocHeader company={company} title={report.title} subtitle={report.subtitle} />
          <table className="doc-table">
            <thead>
              <tr>
                {columns.map((c, i) => (
                  <th key={i} className={RIGHT.has(c.kind) ? 'doc-num' : ''}>{c.label}</th>
                ))}
              </tr>
            </thead>
            <tbody>{tableBody(true)}</tbody>
          </table>
          <h3 style={{ marginTop: '12pt' }}>Comments</h3>
          <CommentList comments={report.comments} />
          <p style={{ fontSize: '8pt', marginTop: '8pt' }}>
            The report as it was when first commented on, {report.createdAt.slice(0, 10)}, by {report.createdBy}.
          </p>
        </article>
      </PrintPortal>
      <article className="mx-auto w-full max-w-[1100px] bg-white px-6 py-4 shadow-panel">
        <div className="no-print mb-2 flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={onBack}>
            ‹ Commented Reports
          </button>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => printDocument(`${report.title} with comments`)}>
            Print…
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => api.deleteCommentedReport(report.id).then(onBack, (err) => setNotice({ kind: 'error', text: errorText(err) }))}>
            Delete
          </button>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        <header className="mb-3 border-b-[3px] border-double border-qb-ink pb-2">
          <div className="text-xs uppercase tracking-wide text-qb-ink-muted">{company.name}</div>
          <h2 className="text-xl font-semibold">{report.title}</h2>
          <div className="text-sm text-qb-ink-muted">{report.subtitle}</div>
          <div className="text-xs text-qb-ink-muted">
            As it was when first commented on, {report.createdAt.slice(0, 10)}, by {report.createdBy}. Click a figure to add a comment.
          </div>
        </header>
        {adding && (
          <div className="no-print mb-2 flex items-end gap-2 border border-qb-border bg-qb-canvas px-2 py-1.5 text-sm">
            <label className="flex-1">
              <span className="qb-label">Comment on {adding.figure}</span>
              <input className="qb-input w-full" autoFocus maxLength={2000} value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && body.trim() && add()} />
            </label>
            <button type="button" className="qb-btn qb-btn-primary h-row" disabled={!body.trim()} onClick={add}>
              Save
            </button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setAdding(null)}>
              Cancel
            </button>
          </div>
        )}
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b border-qb-border-strong">
              {columns.map((c, i) => (
                <th key={i} className={`px-1.5 py-0.5 text-xs font-semibold uppercase tracking-wide text-qb-ink-muted ${RIGHT.has(c.kind) ? 'text-right' : 'text-left'}`}>
                  {c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>{tableBody(false)}</tbody>
        </table>
        <section className="mt-4 border-t border-qb-border pt-2">
          <h3 className="mb-1 text-md font-semibold">Comments</h3>
          {report.comments.length ? <CommentList comments={report.comments} onDelete={remove} /> : <p className="text-sm text-qb-ink-muted">No comments left. Click a figure to add one, or delete this report.</p>}
        </section>
      </article>
    </>
  );
}

/** Reports › Commented Reports: every report kept with comments, most recently commented first. */
export default function CommentedReports() {
  const { data, error, reload } = useLedgerQuery(api.listCommentedReports);
  const [open, setOpen] = useState(null);
  if (open) {
    return (
      <CommentedReportView
        id={open}
        onBack={() => {
          setOpen(null);
          reload();
        }}
      />
    );
  }
  return (
    <section className="qb-panel mx-auto w-full max-w-[1100px]">
      <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Commented Reports</h2>
        <span className="text-xs text-qb-ink-muted">Reports kept as they were when someone commented on a figure</span>
      </div>
      {error && <Notice kind="error" text={error} />}
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head">Report</th>
            <th className="qb-grid-head">Dates</th>
            <th className="qb-grid-head w-[110px] text-right">Comments</th>
            <th className="qb-grid-head w-[150px]">Last comment</th>
            <th className="qb-grid-head w-[150px]">Kept by</th>
          </tr>
        </thead>
        <tbody>
          {(data ?? []).map((r) => (
            <tr key={r.id} className="qb-grid-row cursor-pointer" onClick={() => setOpen(r.id)} title="Open with its comments">
              <td className="qb-grid-cell">{r.title}</td>
              <td className="qb-grid-cell text-qb-ink-muted">{r.subtitle}</td>
              <td className="qb-grid-cell num">{r.commentCount}</td>
              <td className="qb-grid-cell tabular-nums">{(r.lastCommentAt ?? r.createdAt).slice(0, 10)}</td>
              <td className="qb-grid-cell">{r.createdBy}</td>
            </tr>
          ))}
          {data && data.length === 0 && (
            <tr>
              <td className="qb-grid-cell text-qb-ink-muted" colSpan={5}>
                No commented reports yet. Open a report, click Comment, then click a figure.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}
