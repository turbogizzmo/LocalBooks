import { useEffect, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

const ORIGIN_TEXT = {
  LEARNED: 'Suggested from your past approvals',
  BUILT_IN: 'Suggested by vendor match',
  DEFAULT: 'No match found: uncategorized',
};

const isFeedAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';

export default function BankFeed({ accounts }) {
  const feedAccounts = useMemo(() => accounts.filter((a) => a.isActive && isFeedAccount(a)), [accounts]);
  const [bankAccountId, setBankAccountId] = useState(null);
  const { data, error } = useLedgerQuery(api.listBankTransactions);
  const [selectedId, setSelectedId] = useState(null);
  const [overrides, setOverrides] = useState({});
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [flashId, setFlashId] = useState(null);
  const listRef = useRef(null);

  useEffect(() => {
    if (bankAccountId === null && feedAccounts.length > 0) setBankAccountId(feedAccounts[0].id);
  }, [bankAccountId, feedAccounts]);

  const pending = useMemo(
    () => (data ?? []).filter((t) => t.bankAccountId === bankAccountId),
    [data, bankAccountId],
  );
  const pendingByAccount = useMemo(() => {
    const counts = new Map();
    for (const t of data ?? []) counts.set(t.bankAccountId, (counts.get(t.bankAccountId) ?? 0) + 1);
    return counts;
  }, [data]);

  // Keep a valid selection as the queue changes.
  useEffect(() => {
    if (pending.length === 0) setSelectedId(null);
    else if (!pending.some((t) => t.id === selectedId)) setSelectedId(pending[0].id);
  }, [pending, selectedId]);

  const selectedIndex = pending.findIndex((t) => t.id === selectedId);
  const selected = selectedIndex >= 0 ? pending[selectedIndex] : null;
  const categoryId = selected ? (overrides[selected.id] ?? selected.suggestion?.accountId ?? null) : null;

  const importCsv = async () => {
    if (bankAccountId === null) return;
    try {
      const path = await open({ multiple: false, directory: false, filters: [{ name: 'Bank CSV', extensions: ['csv', 'CSV'] }] });
      if (!path) return;
      setBusy(true);
      const r = await api.importBankCsv(path, bankAccountId);
      const extras = [
        r.duplicates ? `${r.duplicates} already imported` : null,
        r.skippedZero ? `${r.skippedZero} zero-amount skipped` : null,
      ].filter(Boolean);
      setNotice({
        kind: 'success',
        text: `Imported ${r.imported} transaction${r.imported === 1 ? '' : 's'} into ${accountLabel(accounts, bankAccountId)}${extras.length ? ` (${extras.join(', ')})` : ''}.`,
      });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const advanceFrom = (index) => {
    const next = pending[index + 1] ?? pending[index - 1] ?? null;
    setSelectedId(next ? next.id : null);
  };

  const approve = async () => {
    if (!selected || categoryId === null) return;
    setBusy(true);
    try {
      await api.approveBankTransaction(selected.id, categoryId);
      setFlashId(selected.id);
      setNotice({
        kind: 'success',
        text: `Approved ${formatCents(Math.abs(selected.amountCents))} "${selected.description}" → ${accountLabel(accounts, categoryId)}.`,
      });
      advanceFrom(selectedIndex);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  const exclude = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await api.excludeBankTransaction(selected.id);
      setNotice({ kind: 'info', text: `Excluded "${selected.description}" from the review queue.` });
      advanceFrom(selectedIndex);
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (flashId === null) return undefined;
    const t = setTimeout(() => setFlashId(null), 900);
    return () => clearTimeout(t);
  }, [flashId]);

  const onListKey = (e) => {
    if (pending.length === 0) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const delta = e.key === 'ArrowDown' ? 1 : -1;
      const next = Math.min(pending.length - 1, Math.max(0, selectedIndex + delta));
      setSelectedId(pending[next].id);
      listRef.current?.querySelector(`[data-id="${pending[next].id}"]`)?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter' && !busy) {
      e.preventDefault();
      approve();
    }
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-center gap-2 px-2 py-1">
        <label className="flex items-center gap-1.5">
          <span className="text-sm font-semibold text-qb-ink-muted">Account</span>
          <AccountSelect
            accounts={accounts}
            filter={isFeedAccount}
            value={bankAccountId}
            onChange={setBankAccountId}
            className="qb-input w-[240px]"
          />
        </label>
        <button type="button" className="qb-btn qb-btn-secondary" onClick={importCsv} disabled={busy || bankAccountId === null}>
          Import CSV…
        </button>
        <span className="text-xs text-qb-ink-muted">Columns: Date, Description, Amount (negative = money out) or Debit/Credit</span>
        <div className="ml-auto flex gap-3 text-sm">
          {feedAccounts.map((a) => (
            <span key={a.id} className={a.id === bankAccountId ? 'font-semibold' : 'text-qb-ink-muted'}>
              {a.fullName}: {pendingByAccount.get(a.id) ?? 0} for review
            </span>
          ))}
        </div>
      </div>

      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load bank lines: ${error}`} />}

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,3fr)_minmax(320px,2fr)] gap-2">
        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">For review</h2>
            <span className="ml-2 text-xs text-qb-ink-muted">{pending.length} imported line{pending.length === 1 ? '' : 's'} · ↑↓ to move · Enter to approve</span>
          </div>
          <div ref={listRef} tabIndex={0} onKeyDown={onListKey} className="min-h-0 flex-1 overflow-auto outline-none scrollbar-thin focus-visible:shadow-focus">
            <table className="qb-grid">
              <colgroup>
                <col className="w-[92px]" />
                <col />
                <col className="w-[104px]" />
                <col className="w-[104px]" />
                <col className="w-[180px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head">Date</th>
                  <th className="qb-grid-head">Description</th>
                  <th className="qb-grid-head text-right">Spent</th>
                  <th className="qb-grid-head text-right">Received</th>
                  <th className="qb-grid-head">Suggested category</th>
                </tr>
              </thead>
              <tbody>
                {pending.length === 0 && (
                  <tr>
                    <td colSpan={5} className="qb-grid-cell text-qb-ink-muted">
                      Nothing to review. Import a CSV export from your bank to get started.
                    </td>
                  </tr>
                )}
                {pending.map((t) => {
                  const chosen = overrides[t.id] ?? t.suggestion?.accountId ?? null;
                  return (
                    <tr
                      key={t.id}
                      data-id={t.id}
                      aria-selected={t.id === selectedId}
                      className="qb-grid-row"
                      onMouseDown={() => setSelectedId(t.id)}
                    >
                      <td className="qb-grid-cell">{formatDate(t.date)}</td>
                      <td className="qb-grid-cell" title={t.description}>{t.description}</td>
                      <td className="qb-grid-cell num">{t.amountCents < 0 ? formatCents(-t.amountCents) : ''}</td>
                      <td className="qb-grid-cell num">{t.amountCents > 0 ? formatCents(t.amountCents) : ''}</td>
                      <td className={`qb-grid-cell ${t.suggestion?.origin === 'DEFAULT' ? 'text-qb-ink-faint' : ''}`}>
                        {chosen !== null ? accountLabel(accounts, chosen) : ''}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <section className="qb-panel flex min-h-0 flex-col">
          <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
            <h2 className="text-md font-semibold">Categorize</h2>
          </div>
          {selected ? (
            <div className={`flex flex-col gap-2 p-2 transition-colors duration-75 ${flashId ? 'bg-qb-green-tint' : ''}`}>
              <dl className="grid grid-cols-[88px_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-sm">
                <dt className="text-qb-ink-muted">Date</dt>
                <dd>{formatDate(selected.date)}</dd>
                <dt className="text-qb-ink-muted">Description</dt>
                <dd className="break-words font-semibold">{selected.description}</dd>
                <dt className="text-qb-ink-muted">{selected.amountCents < 0 ? 'Spent' : 'Received'}</dt>
                <dd className="num text-left text-xl font-semibold">{formatCents(Math.abs(selected.amountCents))}</dd>
                <dt className="text-qb-ink-muted">Account</dt>
                <dd>{accountLabel(accounts, selected.bankAccountId)}</dd>
              </dl>

              <label>
                <span className="qb-label">{selected.amountCents < 0 ? 'Expense category' : 'Income category'}</span>
                <AccountSelect
                  accounts={accounts}
                  filter={(a) => a.id !== selected.bankAccountId}
                  value={categoryId}
                  onChange={(id) => setOverrides((o) => ({ ...o, [selected.id]: id }))}
                  placeholder="Select category"
                />
                {selected.suggestion && overrides[selected.id] === undefined && (
                  <span className="mt-0.5 block text-xs text-qb-ink-muted">{ORIGIN_TEXT[selected.suggestion.origin]}</span>
                )}
              </label>

              <div className="rounded-xs border border-qb-border bg-qb-canvas px-2 py-1 text-xs">
                <div className="mb-0.5 font-semibold text-qb-ink-muted">Journal entry preview</div>
                {categoryId !== null &&
                  (selected.amountCents < 0
                    ? [['Dr', categoryId], ['Cr', selected.bankAccountId]]
                    : [['Dr', selected.bankAccountId], ['Cr', categoryId]]
                  ).map(([side, id]) => (
                    <div key={side} className="flex gap-2">
                      <span className={`w-5 font-semibold ${side === 'Cr' ? 'pl-2' : ''}`}>{side}</span>
                      <span className="flex-1 truncate">{accountLabel(accounts, id)}</span>
                      <span className="num">{formatCents(Math.abs(selected.amountCents))}</span>
                    </div>
                  ))}
              </div>

              <div className="flex gap-2">
                <button type="button" className="qb-btn qb-btn-primary flex-1" onClick={approve} disabled={busy || categoryId === null}>
                  {busy ? 'Posting…' : 'Approve'}
                </button>
                <button type="button" className="qb-btn qb-btn-secondary" onClick={exclude} disabled={busy}>
                  Exclude
                </button>
              </div>
            </div>
          ) : (
            <div className="p-2 text-sm text-qb-ink-muted">Select a transaction on the left to categorize it.</div>
          )}
        </section>
      </div>
    </div>
  );
}
