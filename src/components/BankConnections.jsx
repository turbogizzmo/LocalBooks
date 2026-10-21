import { useEffect, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { api, errorText } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const isFeedAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
const BRIDGE_URL = 'https://bridge.simplefin.org';

const when = (iso) => (iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : 'never');

/** One line describing a sync, for the Bank Feeds notice. */
export function syncSummaryText(s) {
  const extras = [
    s.duplicates ? `${s.duplicates} already imported` : null,
    s.pending ? `${s.pending} pending (next sync)` : null,
    s.beforeStart ? `${s.beforeStart} before the start date` : null,
    s.skippedZero ? `${s.skippedZero} zero-amount skipped` : null,
    s.problems.length ? `${s.problems.length} unreadable: ${s.problems.join('; ')}` : null,
  ].filter(Boolean);
  const head = `Synced ${s.accounts} account${s.accounts === 1 ? '' : 's'}: ${s.imported} new transaction${s.imported === 1 ? '' : 's'} for review`;
  const messages = s.messages.length ? ` SimpleFIN says: ${s.messages.join(' · ')}` : '';
  return `${head}${extras.length ? ` (${extras.join(', ')})` : ''}.${messages}`;
}

function TokenField({ label, busyLabel, onSubmit, autoFocus = false }) {
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);
  const submit = async () => {
    setBusy(true);
    try {
      await onSubmit(token);
      setToken('');
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-start gap-2">
      <textarea
        ref={ref}
        className="qb-input h-[52px] min-w-0 flex-1 resize-none py-1 font-mono text-xs"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder="Paste the setup token from SimpleFIN Bridge"
        spellCheck={false}
        autoComplete="off"
        aria-label="SimpleFIN setup token"
      />
      <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || !token.trim()} onClick={submit}>
        {busy ? busyLabel : label}
      </button>
    </div>
  );
}

function LinkRow({ account, accounts, linkedIds, onChanged, setNotice }) {
  const [bankAccountId, setBankAccountId] = useState(null);
  const [syncFrom, setSyncFrom] = useState('');
  const [busy, setBusy] = useState(false);
  const { homeCurrency } = useCompany();
  const linked = account.bankAccountId !== null;
  const foreign = homeCurrency && account.currency && account.currency !== homeCurrency;
  const today = todayIso();

  // Start where Bank Feeds leaves off for the chosen account, so nothing is imported twice.
  useEffect(() => {
    if (bankAccountId === null) return;
    let stale = false;
    api.suggestedBankStart(bankAccountId).then((d) => !stale && setSyncFrom(d), () => {});
    return () => {
      stale = true;
    };
  }, [bankAccountId]);

  const run = async (fn, done) => {
    setBusy(true);
    try {
      await fn();
      setNotice({ kind: 'success', text: done });
      await onChanged();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <tr>
      <td className="qb-grid-cell">
        <div className="font-semibold">{account.name}</div>
        <div className="text-xs text-qb-ink-muted">{account.institution ?? '—'}</div>
      </td>
      <td className="qb-grid-cell num">
        {account.balanceCents !== null ? formatCents(account.balanceCents) : '—'}
        <div className="text-xs text-qb-ink-muted">
          {account.currency}
          {account.balanceDate ? ` · ${formatDate(account.balanceDate)}` : ''}
        </div>
      </td>
      <td className="qb-grid-cell">
        {linked ? (
          <div className="flex items-center gap-2">
            <div className="min-w-0 flex-1">
              <div className="truncate">→ {accountLabel(accounts, account.bankAccountId)}</div>
              <div className="text-xs text-qb-ink-muted">
                From {formatDate(account.syncFrom)} · last synced {when(account.lastSyncedAt)}
              </div>
            </div>
            <button
              type="button"
              className="qb-btn qb-btn-secondary h-[22px] px-2 text-xs"
              disabled={busy}
              onClick={() => run(() => api.unlinkBankAccount(account.id), `${account.name} is no longer linked. Lines already imported stay in Bank Feeds.`)}
            >
              Unlink
            </button>
          </div>
        ) : foreign ? (
          <span className="block whitespace-normal text-xs text-qb-ink-muted">
            In {account.currency}; LocalBooks bank accounts are kept in {homeCurrency}. Import this account’s statements as files.
          </span>
        ) : (
          <div className="flex flex-wrap items-center gap-1.5">
            <AccountSelect
              accounts={accounts}
              filter={(a) => isFeedAccount(a) && !linkedIds.has(a.id)}
              value={bankAccountId}
              onChange={setBankAccountId}
              placeholder="Link to…"
              className="qb-input h-row w-[200px] text-sm"
              aria-label={`LocalBooks account for ${account.name}`}
            />
            {bankAccountId !== null && (
              <>
                <label className="flex items-center gap-1 text-xs text-qb-ink-muted">
                  from
                  <input
                    type="date"
                    className="qb-input h-row text-sm"
                    value={syncFrom}
                    min={addDays(today, -90)}
                    max={today}
                    onChange={(e) => setSyncFrom(e.target.value)}
                  />
                </label>
                <button
                  type="button"
                  className="qb-btn qb-btn-primary h-[22px] px-2 text-xs"
                  disabled={busy || !syncFrom}
                  onClick={() =>
                    run(
                      () => api.linkBankAccount(account.id, bankAccountId, syncFrom),
                      `${account.name} now feeds ${accountLabel(accounts, bankAccountId)} from ${formatDate(syncFrom)}. Choose Sync now to fetch transactions.`,
                    )
                  }
                >
                  Link
                </button>
              </>
            )}
          </div>
        )}
      </td>
    </tr>
  );
}

/**
 * SimpleFIN bank connections: paste a setup token, link each bank account to a LocalBooks account,
 * then sync. Synced transactions land in the Bank Feeds review queue like imported statements.
 */
export default function BankConnections({ accounts, onClose, onSynced }) {
  const [connections, setConnections] = useState(null);
  const [notice, setNotice] = useState(null);
  const [busyId, setBusyId] = useState(null);

  const reload = async () => {
    try {
      setConnections(await api.listBankConnections());
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
      setConnections([]);
    }
  };
  useEffect(() => {
    reload();
  }, []);
  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const linkedIds = new Set((connections ?? []).flatMap((c) => c.accounts.map((a) => a.bankAccountId).filter((id) => id !== null)));

  const connect = async (token) => {
    try {
      const view = await api.connectBank(token);
      setNotice({ kind: 'success', text: `Connected to ${view.serverHost}: ${view.accounts.length} account${view.accounts.length === 1 ? '' : 's'} found. Link each one you want to a LocalBooks account.` });
      await reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const act = async (id, fn) => {
    setBusyId(id);
    try {
      await fn();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusyId(null);
      await reload();
    }
  };

  const sync = (c) =>
    act(c.id, async () => {
      const summary = await api.syncBankConnection(c.id);
      setNotice({ kind: 'success', text: syncSummaryText(summary) });
      onSynced?.(summary);
    });

  const remove = (c) => {
    if (!window.confirm(`Remove the connection to ${c.serverHost}? LocalBooks forgets its access on this computer. Transactions already imported stay in Bank Feeds.`)) return;
    act(c.id, async () => {
      await api.disconnectBank(c.id);
      setNotice({ kind: 'info', text: 'Connection removed. To stop SimpleFIN sharing the data entirely, also disable the app connection in SimpleFIN Bridge.' });
    });
  };

  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center bg-black/30 p-6" role="dialog" aria-modal="true" aria-label="Bank connections">
      <div className="qb-panel flex max-h-full w-full max-w-[860px] flex-col shadow-lg">
        <div className="flex items-center gap-2 border-b border-qb-border px-3 py-2">
          <h2 className="text-md font-semibold">Bank connections</h2>
          <span className="text-xs text-qb-ink-muted">Automatic bank feeds through SimpleFIN</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row-lg px-3" onClick={onClose}>
            Close
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <section className="border-b border-qb-border px-3 py-2 text-xs text-qb-ink-muted">
            <p className="mb-1">
              SimpleFIN Bridge connects to your banks and shares <strong>read-only</strong> transactions with LocalBooks. It is a separate
              service with its own subscription (about $15 a year, paid to SimpleFIN). LocalBooks never sees your bank sign-in.
            </p>
            <ol className="ml-4 list-decimal">
              <li>
                At <span className="select-all font-mono text-qb-ink">{BRIDGE_URL}</span>, create an account and connect your banks.
              </li>
              <li>In Bridge, create a setup token for a new app connection and copy it.</li>
              <li>Paste it below. A token works once; the access it unlocks is kept in this computer’s keychain, not in the company file.</li>
            </ol>
          </section>

          <section className="border-b border-qb-border px-3 py-2">
            <TokenField label="Connect" busyLabel="Connecting…" onSubmit={connect} autoFocus={connections?.length === 0} />
          </section>

          {notice && (
            <div className="px-3 pt-2">
              <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />
            </div>
          )}

          {connections === null && <p className="px-3 py-2 text-sm text-qb-ink-muted">Loading…</p>}
          {(connections ?? []).map((c) => (
            <section key={c.id} className="m-3 rounded-xs border border-qb-border">
              <div className="flex flex-wrap items-center gap-2 border-b border-qb-border bg-qb-canvas px-2 py-1.5">
                <span className="text-sm font-semibold">{c.serverHost}</span>
                <span className="text-xs text-qb-ink-muted">Last synced {when(c.lastSyncedAt)}</span>
                <div className="ml-auto flex gap-1.5">
                  <button
                    type="button"
                    className="qb-btn qb-btn-primary h-[22px] px-2 text-xs"
                    disabled={busyId !== null || !c.hasAccess || !c.accounts.some((a) => a.bankAccountId !== null)}
                    onClick={() => sync(c)}
                  >
                    {busyId === c.id ? 'Working…' : 'Sync now'}
                  </button>
                  <button
                    type="button"
                    className="qb-btn qb-btn-secondary h-[22px] px-2 text-xs"
                    disabled={busyId !== null || !c.hasAccess}
                    onClick={() => act(c.id, () => api.refreshBankAccounts(c.id))}
                    title="Re-read the account list and balances"
                  >
                    Refresh accounts
                  </button>
                  <button type="button" className="qb-btn qb-btn-secondary h-[22px] px-2 text-xs" disabled={busyId !== null} onClick={() => remove(c)}>
                    Remove
                  </button>
                </div>
              </div>

              {c.messages.length > 0 && (
                <ul className="border-b border-qb-border bg-[#fff8e6] px-3 py-1 text-xs">
                  {c.messages.map((m) => (
                    <li key={m}>{errorText(m)}</li>
                  ))}
                </ul>
              )}

              {!c.hasAccess && (
                <div className="border-b border-qb-border px-3 py-2 text-xs">
                  <p className="mb-1 text-qb-ink-muted">
                    This computer has no saved access for this connection (the company file may have been set up on another computer).
                    Create a new setup token in SimpleFIN Bridge and paste it here; the account links are kept.
                  </p>
                  <TokenField label="Reconnect" busyLabel="Reconnecting…" onSubmit={(t) => act(c.id, () => api.reconnectBank(c.id, t))} />
                </div>
              )}

              <table className="qb-grid">
                <colgroup>
                  <col className="w-[34%]" />
                  <col className="w-[18%]" />
                  <col />
                </colgroup>
                <thead>
                  <tr>
                    <th className="qb-grid-head">SimpleFIN account</th>
                    <th className="qb-grid-head text-right">Balance</th>
                    <th className="qb-grid-head">Feeds into</th>
                  </tr>
                </thead>
                <tbody>
                  {c.accounts.length === 0 && (
                    <tr>
                      <td colSpan={3} className="qb-grid-cell text-qb-ink-muted">
                        No accounts yet. Connect a bank in SimpleFIN Bridge, then choose Refresh accounts.
                      </td>
                    </tr>
                  )}
                  {c.accounts.map((a) => (
                    <LinkRow key={a.id} account={a} accounts={accounts} linkedIds={linkedIds} onChanged={reload} setNotice={setNotice} />
                  ))}
                </tbody>
              </table>
            </section>
          ))}
        </div>

        <div className="border-t border-qb-border px-3 py-1.5 text-xs text-qb-ink-muted">
          Synced transactions wait in Bank Feeds for you to categorize and approve; nothing posts on its own. SimpleFIN allows about 24
          updates a day, so LocalBooks syncs only when you ask, at most every 10 minutes, and reaches back up to 90 days.
        </div>
      </div>
    </div>
  );
}
