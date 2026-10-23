import { Fragment, useEffect, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { EntryLines, SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { formatDate } from '../lib/dates.js';

/** Headings for the increase and decrease columns, by account type, as in classic desktop registers. */
function amountHeads(typeName) {
  switch (typeName) {
    case 'Bank':
      return ['Deposit', 'Payment'];
    case 'Credit Card':
    case 'Accounts Receivable':
      return ['Charge', 'Payment'];
    case 'Accounts Payable':
      return ['Billed', 'Paid'];
    default:
      return ['Increase', 'Decrease'];
  }
}

/**
 * An account register: every posted line of one account in date order with a running balance.
 * Reached from Accounting › Account Registers, a double-click in the Chart of Accounts, or a click
 * on an account in a report (which opens it for the report's period).
 */
export default function Register({ accounts, intent }) {
  const [accountId, setAccountId] = useState(intent?.data?.accountId ?? null);
  const [start, setStart] = useState(intent?.data?.startDate ?? '');
  const [end, setEnd] = useState(intent?.data?.endDate ?? '');
  const [subaccounts, setSubaccounts] = useState(intent?.data?.includeSubaccounts ?? false);
  const [expanded, setExpanded] = useState(null);
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (intent?.action !== 'open' || !intent.data) return;
    setAccountId(intent.data.accountId ?? null);
    setStart(intent.data.startDate ?? '');
    setEnd(intent.data.endDate ?? '');
    setSubaccounts(intent.data.includeSubaccounts ?? false);
  }, [intent]);

  const { data, error, reload } = useLedgerQuery(() =>
    accountId === null ? Promise.resolve(null) : api.getAccountRegister(accountId, start || null, end || null, subaccounts),
  );
  useEffect(() => {
    reload();
    setExpanded(null);
  }, [accountId, start, end, subaccounts, reload]);

  const account = accounts.find((a) => a.id === accountId);
  const hasChildren = accounts.some((a) => a.parentId === accountId);
  // In a register, the "increase" column is the account's normal side.
  const [increaseHead, decreaseHead] = amountHeads(account?.accountTypeName);
  const increase = (r) => (data?.isDebitNormal ? r.debitCents : r.creditCents);
  const decrease = (r) => (data?.isDebitNormal ? r.creditCents : r.debitCents);
  // A bank register reads like a checkbook: payments, then deposits.
  const paymentsFirst = account?.accountTypeName === 'Bank';
  const [firstHead, secondHead] = paymentsFirst ? [decreaseHead, increaseHead] : [increaseHead, decreaseHead];
  const [firstAmount, secondAmount] = paymentsFirst ? [decrease, increase] : [increase, decrease];

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1">
        <label>
          <span className="qb-label">Account</span>
          <AccountSelect accounts={accounts} value={accountId} onChange={setAccountId} placeholder="Choose an account…" className="qb-input w-[300px]" />
        </label>
        <label>
          <span className="qb-label">From</span>
          <input type="date" className="qb-input w-[132px]" value={start} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">To</span>
          <input type="date" className="qb-input w-[132px]" value={end} onChange={(e) => setEnd(e.target.value)} />
        </label>
        {(hasChildren || subaccounts) && (
          <label className="flex items-center gap-1 self-center pt-3 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={subaccounts} onChange={(e) => setSubaccounts(e.target.checked)} />
            Include subaccounts
          </label>
        )}
        {data && (
          <div className="ml-auto text-right">
            <div className="qb-label">Ending balance</div>
            <div className="num text-2xl font-semibold leading-none">{formatCents(data.endingBalanceCents)}</div>
          </div>
        )}
      </div>
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">{data ? `${data.accountName}${data.includeSubaccounts ? ' (with subaccounts)' : ''}` : 'Register'}</h2>
          {data && <span className="text-xs text-qb-ink-muted">{data.accountTypeName} · click a row to see the whole transaction</span>}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {accountId === null ? (
            <div className="p-2 text-sm text-qb-ink-muted">Choose an account to see its register.</div>
          ) : (
            <table className="qb-grid">
              <colgroup>
                <col className="w-[92px]" />
                <col className="w-[110px]" />
                <col className="w-[56px]" />
                <col className="w-[170px]" />
                <col />
                <col className="w-[190px]" />
                <col className="w-[110px]" />
                <col className="w-[30px]" />
                <col className="w-[110px]" />
                <col className="w-[110px]" />
                <col className="w-[120px]" />
                <col className="w-[30px]" />
              </colgroup>
              <thead>
                <tr>
                  <th className="qb-grid-head">Date</th>
                  <th className="qb-grid-head">Type</th>
                  <th className="qb-grid-head">No.</th>
                  <th className="qb-grid-head">Payee / name</th>
                  <th className="qb-grid-head">Memo</th>
                  <th className="qb-grid-head">Account</th>
                  <th className="qb-grid-head">Class</th>
                  <th className="qb-grid-head text-center" title="Cleared (C) or reconciled (R)">✓</th>
                  <th className="qb-grid-head text-right">{firstHead}</th>
                  <th className="qb-grid-head text-right">{secondHead}</th>
                  <th className="qb-grid-head text-right">Balance</th>
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {data && (data.startDate || data.rows.length === 0) && (
                  <tr className="bg-qb-canvas">
                    <td className="qb-grid-cell" colSpan={10}>
                      {data.startDate ? `Balance forward as of ${formatDate(data.startDate)}` : 'No transactions yet.'}
                    </td>
                    <td className="qb-grid-cell num font-semibold">{formatCents(data.openingBalanceCents)}</td>
                    <td />
                  </tr>
                )}
                {(data?.rows ?? []).map((r) => (
                  <Fragment key={r.lineId}>
                    <tr
                      className={`qb-grid-row cursor-pointer ${expanded === r.journalEntryId ? 'is-active' : ''} ${r.isVoided || r.isReversal ? 'text-qb-ink-faint' : ''}`}
                      onClick={() => setExpanded(expanded === r.journalEntryId ? null : r.journalEntryId)}
                    >
                      <td className="qb-grid-cell">{formatDate(r.entryDate)}</td>
                      <td className="qb-grid-cell">{r.isReversal ? 'Void' : (SOURCE_LABEL[r.sourceType] ?? r.sourceType)}</td>
                      <td className="qb-grid-cell">{r.entryNumber}</td>
                      <td className="qb-grid-cell" title={r.partyName ?? ''}>{r.partyName ?? ''}</td>
                      <td className="qb-grid-cell text-qb-ink-muted" title={r.memo ?? ''}>
                        {r.isVoided && <span className="mr-1 text-2xs font-semibold uppercase text-qb-negative">Voided</span>}
                        {r.memo ?? ''}
                      </td>
                      <td className="qb-grid-cell" title={r.split ?? ''}>
                        {data.includeSubaccounts && r.accountName !== data.accountName ? <span className="text-qb-ink-muted">{r.accountName} · </span> : null}
                        {r.split ?? ''}
                      </td>
                      <td className="qb-grid-cell text-qb-ink-muted">{r.className ?? ''}</td>
                      <td className="qb-grid-cell text-center font-semibold">{r.reconciled ? 'R' : r.cleared ? 'C' : ''}</td>
                      <td className="qb-grid-cell num">{firstAmount(r) ? formatCents(firstAmount(r)) : ''}</td>
                      <td className="qb-grid-cell num">{secondAmount(r) ? formatCents(secondAmount(r)) : ''}</td>
                      <td className={`qb-grid-cell num font-semibold ${r.balanceCents < 0 ? 'text-qb-negative' : ''}`}>{formatCents(r.balanceCents)}</td>
                      <td className="qb-grid-cell cell-tight text-center" onClick={(ev) => ev.stopPropagation()}>
                        {!r.isReversal && (
                          <RowMenu
                            items={[
                              voidItem({
                                journalEntryId: r.journalEntryId,
                                voided: r.isVoided,
                                description: `Entry #${r.entryNumber} (${SOURCE_LABEL[r.sourceType] ?? r.sourceType}).`,
                                onVoid: voidEntry,
                              }),
                            ]}
                          />
                        )}
                      </td>
                    </tr>
                    {expanded === r.journalEntryId && (
                      <tr className="bg-qb-canvas">
                        <td colSpan={12} className="border-b border-qb-border">
                          <EntryLines entryId={r.journalEntryId} />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </div>
  );
}
