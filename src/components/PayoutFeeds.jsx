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
/** How often a feed is fetched while LocalBooks is open; '' is only by hand. */
const AUTO = [
  ['', 'Only when I click Get payouts'],
  ['1', 'Every hour'],
  ['6', 'Every 6 hours'],
  ['12', 'Every 12 hours'],
  ['24', 'Once a day'],
];
const PROCESSOR = { STRIPE: 'Stripe', PAYPAL: 'PayPal', SHOPIFY: 'Shopify Payments' };

/**
 * Connect (or change) a processor account: its read-only credentials (a Stripe restricted key, or a
 * PayPal app's client id and secret) and the accounts each part posts to.
 */
function FeedForm({ accounts, feed, processor = feed?.processor ?? 'STRIPE', onDone, onCancel }) {
  const paypal = processor === 'PAYPAL';
  const shopify = processor === 'SHOPIFY';
  const [form, setForm] = useState(() => ({
    name: feed?.name ?? PROCESSOR[processor],
    key: '',
    clientId: '',
    secret: '',
    shop: '',
    token: '',
    sandbox: false,
    ...Object.fromEntries(ROLES.map(([k]) => [k, feed?.[k] ?? null])),
  }));
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const credentialsReady = paypal ? form.clientId.trim() && form.secret.trim() : shopify ? form.shop.trim() && form.token.trim() : form.key.trim();
  const ready = form.name.trim() && ROLES.every(([k]) => form[k]) && (feed || credentialsReady);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const input = { name: form.name, ...Object.fromEntries(ROLES.map(([k]) => [k, form[k]])) };
      onDone(
        feed
          ? await api.updatePayoutFeed(feed.id, input)
          : paypal
            ? await api.connectPaypalFeed({ clientId: form.clientId, secret: form.secret, sandbox: form.sandbox }, input)
            : shopify
              ? await api.connectShopifyFeed(form.shop, form.token, input)
              : await api.connectStripeFeed(form.key, input),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-2 border-t border-qb-border px-2 py-2 text-sm">
      {!feed && paypal && (
        <p className="text-xs text-qb-ink-muted">
          At developer.paypal.com, create a REST app for your business account with <span className="font-semibold">Transaction search</span> turned on, and paste its client id and
          secret here. LocalBooks only reads your transactions with them, keeps them in this computer’s credential store, calls PayPal only when you get payouts, and sends
          nothing anywhere else.
        </p>
      )}
      {!feed && shopify && (
        <p className="text-xs text-qb-ink-muted">
          In your Shopify admin, under Settings › Apps and sales channels › Develop apps, create an app, give its Admin API only the{' '}
          <span className="font-semibold">read_shopify_payments_payouts</span> and <span className="font-semibold">read_shopify_payments_accounts</span> scopes, install it, and paste
          its Admin API access token with your store’s address. LocalBooks keeps the token in this computer’s credential store, calls only your store’s Admin API when you get
          payouts, and sends nothing anywhere else.
        </p>
      )}
      {!feed && !paypal && !shopify && (
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
        {!feed && paypal && (
          <>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Client id</span>
              <input className="qb-input font-mono" autoComplete="off" spellCheck={false} value={form.clientId} onChange={(e) => setForm({ ...form, clientId: e.target.value })} />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Secret</span>
              <input className="qb-input font-mono" type="password" autoComplete="off" spellCheck={false} value={form.secret} onChange={(e) => setForm({ ...form, secret: e.target.value })} />
            </label>
            <label className="flex items-center gap-1 self-end text-xs">
              <input type="checkbox" checked={form.sandbox} onChange={(e) => setForm({ ...form, sandbox: e.target.checked })} />
              Sandbox app (for trying it out)
            </label>
          </>
        )}
        {!feed && shopify && (
          <>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Store address</span>
              <input className="qb-input" autoComplete="off" spellCheck={false} placeholder="my-store.myshopify.com" value={form.shop} onChange={(e) => setForm({ ...form, shop: e.target.value })} />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="qb-label">Admin API access token</span>
              <input className="qb-input font-mono" type="password" autoComplete="off" spellCheck={false} value={form.token} onChange={(e) => setForm({ ...form, token: e.target.value })} />
            </label>
          </>
        )}
        {!feed && !paypal && !shopify && (
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
          {feed ? 'Save' : busy ? (paypal || shopify ? 'Checking the app…' : 'Checking the key…') : 'Connect'}
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
        <span className="text-xs text-qb-ink-muted">{PROCESSOR[feed.processor] ?? feed.processor}{feed.accountRef === 'SANDBOX' ? ' (sandbox)' : feed.processor === 'SHOPIFY' ? ` · ${feed.accountRef}` : ''} · last fetched {feed.lastFetchedAt ? formatDate(feed.lastFetchedAt.slice(0, 10)) : 'never'}</span>
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
      <div className="flex flex-wrap items-center gap-2 border-b border-qb-border px-2 py-1 text-xs">
        <label className="flex items-center gap-1">
          <span className="text-qb-ink-muted">Fetch</span>
          <select
            className="qb-input h-row"
            value={feed.autoFetchHours ? String(feed.autoFetchHours) : ''}
            disabled={!isAdmin || busy}
            title={isAdmin ? 'While LocalBooks is open and someone who can change the books is signed in. Payouts still wait for you to record them.' : 'An Admin chooses this'}
            onChange={(e) => act(() => api.setPayoutAutoFetch(feed.id, e.target.value ? Number(e.target.value) : null))}
          >
            {AUTO.map(([v, label]) => (
              <option key={v} value={v}>{label}</option>
            ))}
          </select>
        </label>
        {feed.autoFetchHours && feed.lastAutoResult && <span className="text-qb-ink-muted">Last automatic fetch: {feed.lastAutoResult}</span>}
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
 * Banking › Payout Feeds (`payout_feeds.rs`, design in docs/plans/payout-feeds.md): Stripe, PayPal and Shopify Payments payouts
 * as deposits of their gross sales, less fees, refunds and chargebacks.
 */
export default function PayoutFeeds({ accounts, isAdmin = true }) {
  const { data: feeds, error, reload } = useLedgerQuery(api.listPayoutFeeds);
  // The processor being connected, or null.
  const [connecting, setConnecting] = useState(null);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto">
      <section className="qb-panel">
        <div className="flex h-row-lg items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Payout Feeds</h2>
          <span className="text-xs text-qb-ink-muted">A processor’s payout becomes a deposit of the gross sales, less its fees, refunds and chargebacks</span>
          {isAdmin && !connecting && (
            <span className="ml-auto flex gap-1">
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setConnecting('STRIPE')}>Connect Stripe…</button>
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setConnecting('PAYPAL')}>Connect PayPal…</button>
              <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setConnecting('SHOPIFY')}>Connect Shopify…</button>
            </span>
          )}
        </div>
        {error && <Notice kind="error" text={error} />}
        {connecting && <FeedForm key={connecting} accounts={accounts} processor={connecting} onDone={() => (setConnecting(null), reload())} onCancel={() => setConnecting(null)} />}
        {feeds && feeds.length === 0 && !connecting && (
          <p className="px-2 py-1.5 text-sm text-qb-ink-muted">Off. Nothing is fetched until an Admin connects a Stripe account, a PayPal app or a Shopify store with read-only credentials.</p>
        )}
      </section>
      {(feeds ?? []).map((f) => (
        <FeedPanel key={f.id} feed={f} accounts={accounts} isAdmin={isAdmin} onChanged={reload} />
      ))}
    </div>
  );
}
