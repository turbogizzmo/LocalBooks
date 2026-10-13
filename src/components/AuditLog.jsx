import { useState } from 'react';
import Notice from './Notice.jsx';
import { SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';

const ACTION_STYLE = {
  POST: { label: 'Posted', className: 'text-qb-ink' },
  VOID: { label: 'Voided', className: 'font-semibold text-qb-negative' },
  LOCK_RECONCILIATION: { label: 'Reconciled', className: 'font-semibold text-qb-green-active' },
};

const TIMESTAMP = new Intl.DateTimeFormat('en-US', { dateStyle: 'short', timeStyle: 'medium' });

/** Read-only, database-enforced history of every post, void and reconciliation lock. */
export default function AuditLog() {
  const { data, error } = useLedgerQuery(api.listAuditLog);
  const [action, setAction] = useState('ALL');
  const rows = (data ?? []).filter((a) => action === 'ALL' || a.action === action);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {error && <Notice kind="error" text={`Could not load the audit log: ${error}`} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Audit log</h2>
          <select className="qb-input h-row w-[160px] text-sm" value={action} onChange={(e) => setAction(e.target.value)} aria-label="Action">
            <option value="ALL">All events</option>
            <option value="POST">Posted</option>
            <option value="VOID">Voided</option>
            <option value="LOCK_RECONCILIATION">Reconciliations</option>
          </select>
          <span className="ml-auto text-xs text-qb-ink-muted">Recorded by the database itself · cannot be edited or deleted</span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[170px]" />
              <col className="w-[96px]" />
              <col className="w-[120px]" />
              <col className="w-[64px]" />
              <col />
              <col className="w-[120px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">When</th>
                <th className="qb-grid-head">Event</th>
                <th className="qb-grid-head">Source</th>
                <th className="qb-grid-head">Entry</th>
                <th className="qb-grid-head">Detail</th>
                <th className="qb-grid-head text-right">Amount</th>
              </tr>
            </thead>
            <tbody>
              {data && rows.length === 0 && (
                <tr>
                  <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">Nothing recorded yet.</td>
                </tr>
              )}
              {rows.map((a) => (
                <tr key={a.id} className="qb-grid-row">
                  <td className="qb-grid-cell text-qb-ink-muted">{a.occurredAt ? TIMESTAMP.format(new Date(a.occurredAt)) : ''}</td>
                  <td className={`qb-grid-cell ${ACTION_STYLE[a.action]?.className ?? ''}`}>{ACTION_STYLE[a.action]?.label ?? a.action}</td>
                  <td className="qb-grid-cell">{a.sourceType ? (SOURCE_LABEL[a.sourceType] ?? a.sourceType) : ''}</td>
                  <td className="qb-grid-cell">{a.entryNumber ?? ''}</td>
                  <td className="qb-grid-cell" title={a.detail}>{a.detail}</td>
                  <td className="qb-grid-cell num">{a.amountCents !== null ? formatCents(a.amountCents) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
