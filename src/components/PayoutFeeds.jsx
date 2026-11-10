import { Fragment, useEffect, useState } from 'react';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';

const ROLES = [
  ['bankAccountId', 'Payouts arrive in', (a) => a.accountType === 'BANK'],
  ['salesAccountId', 'Sales (gross)', (a) => !['BANK'].includes(a.accountType)],
  ['feeAccountId', 'Processor fees', (a) => a.accountType !== 'BANK'],
  ['refundAccountId', 'Refunds', (a) => a.accountType !== 'BANK'],
  ['chargebackAccountId', 'Chargebacks', (a) => a.accountType !== 'BANK'],
];
const STATUS = { NEW: 'New', RECORDED: 'Recorded', IGNORED: 'Ignored' };

/** Connect (or change) a Stripe account: the restricted key and the accounts each part posts to. */
function FeedForm({ accounts, feed, onDone, onCancel }) {
  const [form, setForm] = useState(() => ({
    name: feed?.name ?? 'Stripe',
    key: '',
    ...Object.fromEntries(ROLES.map(([k]) => [k, feed?.[k] ?? null])),
  }));
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const ready = form.name.trim() && ROLES.every(([k]) => form[k]) && (feed || form.key.trim());
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const input = { name: form.name, ...Object.fromEntries(ROLES.map(([k]) => [k, form[k]])) };
      onDone(feed ? await api.updatePayoutFeed(feed.id, input) : await api.connectStripeFeed(form.key, input));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-2 border-t border-qb-border px-2 py-2 text-sm">
      {!feed && (
        <p className="text-xs text-qb-ink-muted">
          In your Stripe Dashboard, create a <span className="font-semibold">restricted key</span> with read access to Balance and Payouts, and paste it here. LocalBooks keeps it in this
          computer’s credential store, calls Stripe only when you click Get payouts, and sends nothing anywhere else.
        </p>
      )}
      <div className="grid grid-cols-2 gap-2">
        <label className="flex flex-col gap-0.5">
          <span className="qb-label">Name</span>
          <input className="qb-input" value={form.name} maxLength={100} onChange={(e) => setForm({ ...form, name: e.target.value })} />
        </label>
        {!feed && (
          <label className="flex flex-col gap-0.5">
            <span className="qb-label">Restricted key (rk_live_…)</span>
            <input className="qb-input font-mono" type="password" autoComplete="off" spellCheck={false} value={form.key} onChange={(e) => setForm({ ...form, key: e.target.value })} />
          </label>
        )}
        {ROLES.map(([k, label, filter]) => (
          <label key={k} className="flex flex-col gap-0.5">
            <span className="qb-label">{label}</span>
            <AccountSelect accounts={accounts} filter={filter} value={form[k]} placeholder="Choose an account…" onChange={(id) => setForm({ ...form, [k]: id })} />
          </label>
        ))}
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      <div className="flex gap-2">
        <button type="button" className="qb-btn qb-btn-primary h-row" disabled={!ready || busy} onClick={save}>
          {feed ? 'Save' : busy ? 'Checking the key…' : 'Connect'}
        </button>
        <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

/** A payout's sales, refunds and disputes. */
function PayoutDetails({ payout }) {
  let items = [];
  try {
    items = JSON.parse(payout.detailsJson);
  } catch {
    items = [];
  }
  return (
    <ul className="px-6 pb-1 text-xs text-qb-ink-muted">
      {items.map((it, i) => (
        <li key={i} className="flex gap-2">
          <span className="w-24">{it.kind === 'CHARGE' ? 'Sale' : it.kind === 'REFUND' ? 'Refund' : 'Chargeback'}</span>
          <span className="flex-1 truncate">{it.description}</span>
          <span className="num">{formatCents(it.kind === 'CHARGE' ? it.amountCents : -it.amountCents)}</span>
        </li>
      ))}
    </ul>
  );
}

function FeedPanel({ feed, accounts, isAdmin, onChanged }) {
  const { data: payouts, reload } = useLedgerQuery(() => api.listPayouts(feed.id));
  useEffect(() => {
    reload();
  }, [feed.id, reload]);
  const [editing, setEditing] = useState(false);
  const [open, setOpen] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busy, setBusy] = useState(false);
  const act = async (fn, ok) => {
    setBusy(true);
    setNotice(null);
    try {
      const r = await fn();
      if (ok) setNotice({ kind: 'success', text: ok(r) });
      reload();
      onChanged?.();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };
  const fetchNow = () =>
    act(
      () => api.fetchPayouts(feed.id),
      (r) => `${r.newPayouts} new payout${r.newPayouts === 1 ? '' : 's'}.${r.refused.length ? ` Not kept: ${r.refused.join('; ')}.` : ''}`,
    );
  return (
    <section className="qb-panel flex flex-col">
      <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
        <h2 className="text-md font-semibold">{feed.name}</h2>
        <span className="text-xs text-qb-ink-muted">Stripe · last fetched {feed.lastFetchedAt ? formatDate(feed.lastFetchedAt.slice(0, 10)) : 'never'}</span>
        <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" disabled={busy} onClick={fetchNow}>
          {busy ? 'Working…' : 'Get payouts'}
        </button>
        {isAdmin && (
          <>
            <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => setEditing(!editing)}>Accounts…</button>
            <button type="button" className="qb-btn qb-btn-secondary h-row" disabled={busy} onClick={() => act(() => api.disconnectPayoutFeed(feed.id))}>Disconnect</button>
          </>
        )}
      </div>
      {editing && <FeedForm accounts={accounts} feed={feed} onDone={() => (setEditing(false), onChanged?.())} onCancel={() => setEditing(false)} />}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      <table className="qb-grid">
        <thead>
          <tr>
            <th className="qb-grid-head w-[100px]">Arrived</th>
            <th className="qb-grid-head">Payout</th>
            <th className="qb-grid-head w-[110px] text-right">Sales</th>
            <th className="qb-grid-head w-[100px] text-right">Fees</th>
            <th className="qb-grid-head w-[100px] text-right">Refunds</th>
            <th className="qb-grid-head w-[110px] text-right">Chargebacks</th>
            <th className="qb-grid-head w-[110px] text-right">To the bank</th>
            <th className="qb-grid-head w-[90px]">Status</th>
            <th className="qb-grid-head w-[190px]" aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {(payouts ?? []).map((p) => (
            <Fragment key={p.id}>
              <tr className="qb-grid-row cursor-pointer" onClick={() => setOpen(open === p.id ? null : p.id)} title="Show its sales, refunds and disputes">
                <td className="qb-grid-cell tabular-nums">{formatDate(p.arrivalDate)}</td>
                <td className="qb-grid-cell font-mono text-xs">{p.processorPayoutId}</td>
                <td className="qb-grid-cell num">{formatCents(p.grossCents)}</td>
                <td className="qb-grid-cell num">{formatCents(-p.feeCents)}</td>
                <td className="qb-grid-cell num">{formatCents(-p.refundCents)}</td>
                <td className="qb-grid-cell num">{formatCents(-p.chargebackCents)}</td>
                <td className="qb-grid-cell num font-semibold">{formatCents(p.netCents)}</td>
                <td className="qb-grid-cell">{STATUS[p.status]}</td>
                <td className="qb-grid-cell cell-tight" onClick={(e) => e.stopPropagation()}>
                  {p.status === 'NEW' && (
                    <div className="flex gap-1">
                      <button type="button" className="qb-btn qb-btn-primary h-[20px] flex-1" disabled={busy} onClick={() => act(() => api.recordPayout(p.id), () => `Payout ${p.processorPayoutId} recorded as a deposit of ${formatCents(p.netCents)}.`)}>
                        Record deposit
                      </button>
                      <button type="button" className="qb-btn qb-btn-secondary h-[20px] flex-1" disabled={busy} onClick={() => act(() => api.setPayoutIgnored(p.id, true))}>Ignore</button>
                    </div>
                  )}
                  {p.status === 'IGNORED' && (
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full" disabled={busy} onClick={() => act(() => api.setPayoutIgnored(p.id, false))}>Bring back</button>
                  )}
                </td>
              </tr>
              {open === p.id && (
                <tr>
                  <td colSpan={9}>
                    <PayoutDetails payout={p} />
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
          {payouts && payouts.length === 0 && (
            <tr><td className="qb-grid-cell text-qb-ink-muted" colSpan={9}>No payouts yet. Click Get payouts to fetch the last 90 days.</td></tr>
          )}
        </tbody>
      </table>
    </section>
  );
}

/**
 * Banking › Payout Feeds (`payout_feeds.rs`, design in docs/plans/payout-feeds.md): Stripe payouts
 * as deposits of their gross sales, less fees, refunds and chargebacks.
 */
export default function PayoutFeeds({ accounts, isAdmin = true }) {
  const { data: feeds, error, reload } = useLedgerQuery(api.listPayoutFeeds);
  const [connecting, setConnecting] = useState(false);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto">
      <section className="qb-panel">
        <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Payout Feeds</h2>
          <span className="text-xs text-qb-ink-muted">A processor’s payout becomes a deposit of the gross sales, less its fees, refunds and chargebacks</span>
          {isAdmin && !connecting && (
            <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => setConnecting(true)}>Connect Stripe…</button>
          )}
        </div>
        {error && <Notice kind="error" text={error} />}
        {connecting && <FeedForm accounts={accounts} onDone={() => (setConnecting(false), reload())} onCancel={() => setConnecting(false)} />}
        {feeds && feeds.length === 0 && !connecting && (
          <p className="px-2 py-1.5 text-sm text-qb-ink-muted">Off. Nothing is fetched until an Admin connects a Stripe account with a read-only key.</p>
        )}
      </section>
      {(feeds ?? []).map((f) => (
        <FeedPanel key={f.id} feed={f} accounts={accounts} isAdmin={isAdmin} onChanged={reload} />
      ))}
    </div>
  );
}
