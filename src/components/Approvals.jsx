import { useEffect, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import Notice from './Notice.jsx';

const KIND = { BILL: 'Bill', PURCHASE_ORDER: 'Purchase order' };
const STATUS = { PENDING: 'Waiting', APPROVED: 'Approved', REJECTED: 'Rejected' };
const FILTERS = [
  ['PENDING', 'Waiting'],
  ['APPROVED', 'Approved'],
  ['REJECTED', 'Rejected'],
  ['', 'All'],
];

/**
 * Expenses › Approvals (`approvals.rs`, design in docs/plans/approvals.md): bills and purchase
 * orders held by an approval rule. Admins approve (which records the bill or order) or reject with
 * a note (kept, nothing recorded), and set the rules.
 */
export default function Approvals({ isAdmin = true }) {
  const [filter, setFilter] = useState('PENDING');
  const { data: requests, error, reload } = useLedgerQuery(() => api.listApprovalRequests(filter || null));
  useEffect(() => {
    reload();
  }, [filter, reload]);
  const [rejecting, setRejecting] = useState(null);
  const [note, setNote] = useState('');
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);

  const act = async (fn, ok) => {
    setBusy(true);
    setNotice(null);
    try {
      const r = await fn();
      setNotice({ kind: 'success', text: ok(r) });
      reload();
      return true;
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };
  const approve = (r) => act(() => api.approveRequest(r.id), () => `${KIND[r.kind]} from ${r.vendorName} approved and ${r.kind === 'BILL' ? 'posted' : 'recorded'}: ${formatCents(r.totalCents)}.`);
  const reject = () => {
    const r = rejecting;
    // Keep the box and the note open if rejecting failed, so it can be tried again.
    act(() => api.rejectRequest(r.id, note), () => `${KIND[r.kind]} from ${r.vendorName} rejected. It is kept with your note; nothing was recorded.`).then((done) => {
      if (!done) return;
      setRejecting(null);
      setNote('');
    });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <section className="qb-panel flex min-h-0 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Approvals</h2>
          <span className="text-xs text-qb-ink-muted">Bills and purchase orders held by a rule are recorded only when approved</span>
          <div role="radiogroup" aria-label="Show" className="ml-auto flex gap-3 text-sm">
            {FILTERS.map(([id, label]) => (
              <label key={id} className="flex items-center gap-1">
                <input type="radio" name="approval-filter" checked={filter === id} onChange={() => setFilter(id)} />
                {label}
              </label>
            ))}
          </div>
        </div>
        {error && <Notice kind="error" text={error} />}
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        <div className="min-h-0 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <th className="qb-grid-head w-[110px]">Date</th>
                <th className="qb-grid-head w-[120px]">Type</th>
                <th className="qb-grid-head">Vendor</th>
                <th className="qb-grid-head w-[120px] text-right">Total</th>
                <th className="qb-grid-head w-[150px]">Asked by</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head w-[170px]" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {(requests ?? []).map((r) => (
                <tr key={r.id} className="qb-grid-row">
                  <td className="qb-grid-cell tabular-nums">{formatDate(r.documentDate)}</td>
                  <td className="qb-grid-cell">{KIND[r.kind]}{r.reference ? ` ${r.reference}` : ''}</td>
                  <td className="qb-grid-cell">{r.vendorName}</td>
                  <td className="qb-grid-cell num">{formatCents(r.totalCents)}</td>
                  <td className="qb-grid-cell">{r.requestedBy}</td>
                  <td className="qb-grid-cell">
                    {STATUS[r.status]}
                    {r.decidedBy ? ` by ${r.decidedBy}` : ''}
                    {r.note ? <span className="text-qb-ink-muted"> · {r.note}</span> : null}
                  </td>
                  <td className="qb-grid-cell cell-tight">
                    {r.status === 'PENDING' && isAdmin && (
                      <div className="flex gap-1">
                        <button type="button" className="qb-btn qb-btn-primary h-[20px] flex-1" disabled={busy} onClick={() => approve(r)}>Approve</button>
                        <button type="button" className="qb-btn qb-btn-secondary h-[20px] flex-1" disabled={busy} onClick={() => (setRejecting(r), setNote(''))}>Reject…</button>
                      </div>
                    )}
                  </td>
                </tr>
              ))}
              {requests && requests.length === 0 && (
                <tr><td className="qb-grid-cell text-qb-ink-muted" colSpan={7}>{filter === 'PENDING' ? 'Nothing is waiting for approval.' : 'None.'}</td></tr>
              )}
            </tbody>
          </table>
        </div>
        {rejecting && (
          <div className="flex items-end gap-2 border-t border-qb-border bg-qb-canvas px-2 py-2 text-sm" role="dialog" aria-label="Reject">
            <label className="flex-1">
              <span className="qb-label">Why is the {KIND[rejecting.kind].toLowerCase()} from {rejecting.vendorName} rejected?</span>
              <input className="qb-input w-full" autoFocus maxLength={1000} value={note} onChange={(e) => setNote(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && note.trim() && reject()} />
            </label>
            <button type="button" className="qb-btn qb-btn-primary h-row" disabled={busy || !note.trim()} onClick={reject}>Reject</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setRejecting(null)}>Cancel</button>
          </div>
        )}
        {!isAdmin && <p className="border-t border-qb-border px-2 py-1 text-xs text-qb-ink-muted">An Admin approves or rejects.</p>}
      </section>
      <ApprovalRules isAdmin={isAdmin} />
    </div>
  );
}

/** The rules that hold bills and purchase orders for approval. */
function ApprovalRules({ isAdmin }) {
  const { data: rules, reload } = useLedgerQuery(api.listApprovalRules);
  const { data: vendors } = useLedgerQuery(api.listVendors);
  const [form, setForm] = useState({ kind: 'BILL', amount: '', vendorId: '' });
  const [error, setError] = useState(null);
  const add = async () => {
    const cents = form.amount.trim() === '' ? 0 : parseMoneyToCents(form.amount, { impliedDecimal: false });
    if (cents == null || cents < 0) return setError('Enter an amount of zero or more.');
    try {
      await api.addApprovalRule({ kind: form.kind, minTotalCents: cents, vendorId: form.vendorId ? Number(form.vendorId) : null });
      setForm({ kind: form.kind, amount: '', vendorId: '' });
      setError(null);
      reload();
    } catch (err) {
      setError(errorText(err));
    }
    return undefined;
  };
  const describe = (r) =>
    `${KIND[r.kind]}s${r.vendorName ? ` from ${r.vendorName}` : ''}${r.minTotalCents > 0 ? ` of ${formatCents(r.minTotalCents)} or more` : r.vendorName ? '' : ', every one'}`;
  return (
    <section className="qb-panel flex-none">
      <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">Rules</h2>
        <span className="text-xs text-qb-ink-muted">A bill or purchase order matching any rule waits here for approval</span>
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      <ul className="text-sm">
        {(rules ?? []).map((r) => (
          <li key={r.id} className="flex items-center gap-2 border-b border-qb-border px-2 py-0.5">
            <span>Approve {describe(r)}</span>
            {isAdmin && (
              <button type="button" className="qb-btn qb-btn-secondary ml-auto h-[20px] px-2" onClick={() => api.deleteApprovalRule(r.id).then(reload, (err) => setError(errorText(err)))}>
                Remove
              </button>
            )}
          </li>
        ))}
        {rules && rules.length === 0 && <li className="px-2 py-1 text-qb-ink-muted">No rules: bills and purchase orders are recorded when they are saved.</li>}
      </ul>
      {isAdmin && (
        <div className="flex flex-wrap items-end gap-2 px-2 py-2 text-sm">
          <label>
            <span className="qb-label">Approve</span>
            <select className="qb-input w-[160px]" value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value })}>
              <option value="BILL">Bills</option>
              <option value="PURCHASE_ORDER">Purchase orders</option>
            </select>
          </label>
          <label>
            <span className="qb-label">Of at least</span>
            <input className="qb-input w-[120px]" inputMode="decimal" placeholder="0.00 (all)" value={form.amount} onChange={(e) => setForm({ ...form, amount: e.target.value })} />
          </label>
          <label>
            <span className="qb-label">From</span>
            <select className="qb-input w-[220px]" value={form.vendorId} onChange={(e) => setForm({ ...form, vendorId: e.target.value })}>
              <option value="">Any vendor</option>
              {(vendors ?? []).filter((v) => v.isActive !== false).map((v) => (
                <option key={v.id} value={v.id}>{v.name}</option>
              ))}
            </select>
          </label>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={add}>Add rule</button>
        </div>
      )}
    </section>
  );
}
