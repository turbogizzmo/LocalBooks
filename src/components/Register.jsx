import { Fragment, useContext, useEffect, useRef, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import Notice from './Notice.jsx';
import RegisterEntry, { takesEntries } from './RegisterEntry.jsx';
import TransactionHistory from './TransactionHistory.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate, tableExports } from './ReportParts.jsx';
import { EntryLines, SOURCE_LABEL } from './JournalEntries.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents, parseMoneyToCents } from '../lib/money.js';
import { formatDate, periodRange } from '../lib/dates.js';
import { quickReport, reportShortcut, transactionJournal } from '../lib/quickReport.js';
import { useWindowContent, WindowManagerContext } from '../lib/windows.js';
import { opensOwnForm, transactionTarget } from '../lib/openTransaction.js';
import { SortTh, useSort } from './SortHead.jsx';
import DateBox from './DateBox.jsx';
import { setRegisterColor, useMyPreferences } from '../lib/myPreferences.js';
import { REGISTER_COLORS, registerColor, registerTint } from '../lib/textSize.js';

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

/** The register's date filter: all dates, this month, this year, or a range typed in From and To. */
const DATE_FILTERS = [
  ['all', 'All'],
  ['thisMonth', 'This month'],
  ['thisYear', 'This year'],
  ['custom', 'Custom'],
];
const filterFor = (start, end) => {
  if (!start && !end) return 'all';
  for (const id of ['thisMonth', 'thisYear']) {
    const [s, e] = periodRange(id);
    if (s === start && e === end) return id;
  }
  return 'custom';
};

/** What Go To looks in. */
const GO_TO_FIELDS = [
  ['number', 'Number'],
  ['payee', 'Payee'],
  ['memo', 'Memo'],
  ['amount', 'Amount'],
];

/** The register's own layout choice (one line or two per transaction), kept on this computer. */
const LAYOUT_KEY = 'localbooks.register.twoLine';
function readTwoLine() {
  try {
    return window.localStorage.getItem(LAYOUT_KEY) === '1';
  } catch {
    return false;
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
  // The row last clicked or found by Go To: what QuickReport's History and Journal act on.
  const [selected, setSelected] = useState(null);
  const [notice, setNotice] = useState(null);
  const [twoLine, setTwoLineState] = useState(readTwoLine);
  const setTwoLine = (on) => {
    setTwoLineState(on);
    try {
      window.localStorage.setItem(LAYOUT_KEY, on ? '1' : '0');
    } catch {
      // Storage unavailable: the choice holds while the window is open.
    }
  };
  const [goTo, setGoTo] = useState(null); // { field, text, message }
  const [history, setHistory] = useState(null); // journal entry id
  const scrollRef = useRef(null);
  // A row just recorded is scrolled into view once the register has reloaded with it.
  const scrollToEntry = useRef(null);
  const { open: openChild } = useContext(WindowManagerContext);
  // The register's color (a My Preference), kept per company file and account.
  const { data: storage } = useLedgerQuery(api.getStorageStatus);
  // Until the company file's path is known there is no key to keep a color under.
  const companyPath = storage?.path ?? null;
  const registerColors = useMyPreferences().registerColors;
  const color = companyPath === null ? '' : registerColor(registerColors, companyPath, accountId);
  const tint = registerTint(color);

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
    setSelected(null);
  }, [accountId, start, end, subaccounts, reload]);
  useEffect(() => {
    const id = scrollToEntry.current;
    const row = id !== null && scrollRef.current?.querySelector(`[data-entry="${id}"]`);
    if (row) {
      row.scrollIntoView({ block: 'nearest' });
      scrollToEntry.current = null;
    }
  }, [data]);
  const dateFilter = filterFor(start, end);
  const chooseDates = (id) => {
    if (id === 'custom') return;
    const [s, e] = id === 'all' ? ['', ''] : periodRange(id);
    setStart(s);
    setEnd(e);
  };

  const account = accounts.find((a) => a.id === accountId);
  // The window is named after its register ("Checking Register") and reopens on it with the desktop.
  useWindowContent(
    account ? { name: `${account.name} Register`, action: 'open', data: { accountId, startDate: start, endDate: end, includeSubaccounts: subaccounts } } : null,
  );
  const hasChildren = accounts.some((a) => a.parentId === accountId);
  // In a register, the "increase" column is the account's normal side.
  const [increaseHead, decreaseHead] = amountHeads(account?.accountTypeName);
  const increase = (r) => (data?.isDebitNormal ? r.debitCents : r.creditCents);
  const decrease = (r) => (data?.isDebitNormal ? r.creditCents : r.debitCents);
  // A bank register reads like a checkbook: payments, then deposits.
  const paymentsFirst = account?.accountTypeName === 'Bank';
  const [firstHead, secondHead] = paymentsFirst ? [decreaseHead, increaseHead] : [increaseHead, decreaseHead];
  const [firstAmount, secondAmount] = paymentsFirst ? [decrease, increase] : [increase, decrease];

  const company = useCompany();
  const exportRange = start && end ? `${longDate(start)} – ${longDate(end)}` : start ? `From ${longDate(start)}` : end ? `Through ${longDate(end)}` : 'All dates';
  const exports = tableExports({
    company: company.name,
    title: data ? `${data.accountName}${data.includeSubaccounts ? ' (with subaccounts)' : ''} Register` : 'Register',
    subtitle: exportRange,
    columns: [
      { label: 'Date', kind: 'date' },
      { label: 'Type', kind: 'text' },
      { label: 'No.', kind: 'text' },
      { label: 'Payee / name', kind: 'text' },
      { label: 'Memo', kind: 'text' },
      { label: 'Account', kind: 'text' },
      { label: 'Class', kind: 'text' },
      { label: 'Cleared', kind: 'text' },
      { label: firstHead, kind: 'money' },
      { label: secondHead, kind: 'money' },
      { label: 'Balance', kind: 'money' },
    ],
    rows: () => [
      ...(data.startDate ? [{ kind: 'section', cells: [data.startDate, '', '', '', 'Balance forward', '', '', '', null, null, data.openingBalanceCents] }] : []),
      ...data.rows.map((r) => ({
        cells: [
          r.entryDate,
          r.isReversal ? 'Void' : (SOURCE_LABEL[r.sourceType] ?? r.sourceType),
          r.num ?? '',
          r.partyName ?? '',
          `${r.isVoided ? 'VOIDED ' : ''}${r.memo ?? ''}`,
          data.includeSubaccounts && r.accountName !== data.accountName ? `${r.accountName} · ${r.split ?? ''}` : (r.split ?? ''),
          r.className ?? '',
          r.reconciled ? 'R' : r.cleared ? 'C' : '',
          firstAmount(r) || null,
          secondAmount(r) || null,
          r.balanceCents,
        ],
      })),
      { kind: 'total', cells: ['', '', '', '', 'Ending balance', '', '', '', null, null, data.endingBalanceCents] },
    ],
    filename: `${(data?.accountName ?? 'Register').replace(/[^A-Za-z0-9]+/g, '-')}-Register${start ? `_${start}` : ''}${end ? `_to_${end}` : ''}`,
    onNotice: setNotice,
  });

  // Click a heading to sort; the register starts (and a third click returns) in date order. The
  // balance is always the running balance in date order, so it is dimmed while sorted otherwise.
  const typeLabel = (r) => (r.isReversal ? 'Void' : (SOURCE_LABEL[r.sourceType] ?? r.sourceType));
  const sorter = useSort(
    data?.rows,
    {
      date: (r) => r.entryDate,
      type: typeLabel,
      number: (r) => (r.num && /^\d+$/.test(r.num) ? Number(r.num) : r.num),
      payee: (r) => r.partyName,
      memo: (r) => r.memo,
      account: (r) => r.split,
      cleared: (r) => (r.reconciled ? 2 : r.cleared ? 1 : 0),
      first: (r) => firstAmount(r) || null,
      second: (r) => secondAmount(r) || null,
    },
    { initial: null, storageKey: 'register' },
  );
  const inDateOrder = !sorter.sort || (sorter.sort.key === 'date' && sorter.sort.dir === 'asc');
  const sortTh = (col, label, className = '') => (
    <SortTh sorter={sorter} col={col} className={className}>
      {label}
    </SortTh>
  );
  const clearedTh = (
    <SortTh sorter={sorter} col="cleared" className="text-center" title="Cleared (C) or reconciled (R) · click to sort">
      ✓
    </SortTh>
  );
  const balanceTh = <th className="qb-grid-head text-right" title="Running balance in date order">Balance</th>;
  const columnCount = twoLine ? 9 : 12;

  // Double-click (or the row menu) opens the transaction itself: the bill, check or invoice form
  // where LocalBooks has one, otherwise the journal entry.
  const openTransaction = (r) => {
    const [view, action, data] = transactionTarget(r);
    openChild(view, action, data);
  };
  const openLabel = (r) => (opensOwnForm(r) ? `Open ${(SOURCE_LABEL[r.sourceType] ?? 'transaction').toLowerCase()}` : 'Open in journal');

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };

  // QuickReport (Ctrl+Q): this account's transactions for the register's dates. Transaction
  // History (Ctrl+H) and Journal (Ctrl+Y): the selected row's transaction.
  const selectedRow = data?.rows.find((r) => r.journalEntryId === selected) ?? null;
  const rowLabel = (r) => `${SOURCE_LABEL[r.sourceType] ?? r.sourceType} #${r.num ?? r.entryNumber}`;
  const openReport = ([view, action, target]) => openChild(view, action, target, { forceNew: true });
  const runQuickReport = () => account && openReport(quickReport({ kind: 'account', id: account.id, name: account.name, startDate: start, endDate: end }));
  const runJournal = (r) => openReport(transactionJournal({ journalEntryId: r.journalEntryId, label: rowLabel(r) }));
  const pickFirst = () => setNotice({ kind: 'info', text: 'Click a transaction in the register first.' });
  const onKeyDown = (e) => {
    const key = reportShortcut(e);
    if (!key || !data) return;
    e.preventDefault();
    if (key === 'quickReport') runQuickReport();
    else if (!selectedRow) pickFirst();
    else if (key === 'history') setHistory(selectedRow.journalEntryId);
    else runJournal(selectedRow);
  };

  // Go To: the next row (after the selected one, wrapping round) whose number, payee, memo or
  // amount matches, in the order the register is shown.
  const findNext = (backwards = false) => {
    const needle = goTo.text.trim().toLowerCase();
    if (!needle) return;
    const cents = goTo.field === 'amount' ? parseMoneyToCents(goTo.text) : null;
    const hit = (r) => {
      switch (goTo.field) {
        case 'number':
          return (r.num ?? '').toLowerCase() === needle.replace(/^#/, '') || String(r.entryNumber) === needle.replace(/^#/, '');
        case 'payee':
          return (r.partyName ?? '').toLowerCase().includes(needle);
        case 'memo':
          return (r.memo ?? '').toLowerCase().includes(needle);
        default:
          return cents !== null && (r.debitCents === cents || r.creditCents === cents);
      }
    };
    const rows = sorter.sorted;
    const from = rows.findIndex((r) => r.journalEntryId === selected);
    for (let step = 1; step <= rows.length; step += 1) {
      const i = (((from + (backwards ? -step : step)) % rows.length) + rows.length) % rows.length;
      if (hit(rows[i])) {
        setSelected(rows[i].journalEntryId);
        setGoTo((g) => ({ ...g, message: null }));
        requestAnimationFrame(() => scrollRef.current?.querySelector(`[data-entry="${rows[i].journalEntryId}"]`)?.scrollIntoView({ block: 'nearest' }));
        return;
      }
    }
    setGoTo((g) => ({ ...g, message: 'No transaction in this register matches.' }));
  };
  const canEnter = takesEntries(account, subaccounts) && data;

  return (
    // Ctrl+Q, Ctrl+H and Ctrl+Y work anywhere in the register's window.
    <div className="flex h-full flex-col gap-2 p-2 outline-none" tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1" style={tint ? { backgroundColor: tint } : undefined}>
        <label>
          <span className="qb-label">Account</span>
          <AccountSelect accounts={accounts} value={accountId} onChange={setAccountId} placeholder="Choose an account…" className="qb-input w-[300px]" />
        </label>
        <label>
          <span className="qb-label">Dates</span>
          <select className="qb-input w-[120px]" value={dateFilter} onChange={(e) => chooseDates(e.target.value)}>
            {DATE_FILTERS.map(([id, label]) => (
              <option key={id} value={id} disabled={id === 'custom'}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">From</span>
          <DateBox className="qb-input w-[132px]" value={start} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">To</span>
          <DateBox className="qb-input w-[132px]" value={end} onChange={(e) => setEnd(e.target.value)} />
        </label>
        {(hasChildren || subaccounts) && (
          <label className="flex items-center gap-1 self-center pt-3 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={subaccounts} onChange={(e) => setSubaccounts(e.target.checked)} />
            Include subaccounts
          </label>
        )}
        {accountId !== null && (
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row-lg self-end px-2"
            title="Keep this register open in a window of its own"
            onClick={() => openChild('accounting/register', 'open', { accountId, startDate: start, endDate: end, includeSubaccounts: subaccounts }, { forceNew: true })}
          >
            Open in new window
          </button>
        )}
        {data && (
          <>
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg self-end px-2" onClick={() => setGoTo((g) => (g ? null : { field: 'payee', text: '', message: null }))} aria-expanded={Boolean(goTo)}>
              Go To…
            </button>
            <button type="button" className="qb-btn qb-btn-secondary h-row-lg self-end px-2" onClick={runQuickReport} title="This account's transactions as a report (Ctrl+Q)">
              QuickReport
            </button>
            <label className="flex items-center gap-1 self-center pt-3 text-sm" title="Show each transaction on one line, or on two with the account and memo below">
              <input type="checkbox" className="accent-qb-green" checked={twoLine} onChange={(e) => setTwoLine(e.target.checked)} />
              2-line
            </label>
            <label title="A color for this account's register, so it's easy to tell apart (kept on this computer)">
              <span className="qb-label">Color</span>
              <select className="qb-input w-[96px]" value={color} onChange={(e) => companyPath !== null && setRegisterColor(companyPath, accountId, e.target.value)} disabled={companyPath === null} aria-label="Register color">
                {REGISTER_COLORS.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label}
                  </option>
                ))}
              </select>
            </label>
            <div className="self-end">
              <ExportMenu printable={false} onNotice={setNotice} {...exports} />
            </div>
          </>
        )}
        {data && (
          <div className="ml-auto text-right">
            <div className="qb-label">Ending balance</div>
            <div className="num text-2xl font-semibold leading-none">{formatCents(data.endingBalanceCents)}</div>
          </div>
        )}
      </div>
      {goTo && data && (
        <form
          className="qb-panel flex flex-none flex-wrap items-end gap-2 px-2 py-1"
          aria-label="Go To"
          onSubmit={(e) => {
            e.preventDefault();
            findNext(false);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation();
              setGoTo(null);
            }
          }}
        >
          <label>
            <span className="qb-label">Which field</span>
            <select className="qb-input w-[110px]" value={goTo.field} onChange={(e) => setGoTo({ ...goTo, field: e.target.value, message: null })}>
              {GO_TO_FIELDS.map(([id, label]) => (
                <option key={id} value={id}>{label}</option>
              ))}
            </select>
          </label>
          <label>
            <span className="qb-label">Search for</span>
            <input className="qb-input w-[200px]" autoFocus value={goTo.text} onChange={(e) => setGoTo({ ...goTo, text: e.target.value, message: null })} />
          </label>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => findNext(true)} disabled={!goTo.text.trim()}>
            Back
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row" disabled={!goTo.text.trim()}>
            Next
          </button>
          <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setGoTo(null)}>
            Close
          </button>
          {goTo.message && <span className="self-center text-sm text-qb-ink-muted">{goTo.message}</span>}
        </form>
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {history !== null && <TransactionHistory journalEntryId={history} onClose={() => setHistory(null)} onOpen={openChild} />}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">{data ? `${data.accountName}${data.includeSubaccounts ? ' (with subaccounts)' : ''}` : 'Register'}</h2>
          {data && (
            <span className="text-xs text-qb-ink-muted">
              {data.accountTypeName} · click a row to see its lines, double-click to open it · Ctrl+Q QuickReport · Ctrl+H history · Ctrl+Y journal
            </span>
          )}
          {data && !inDateOrder && (
            <span className="ml-auto text-xs text-qb-ink-muted">Sorted: balances are still in date order</span>
          )}
        </div>
        <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          {accountId === null ? (
            <div className="p-2 text-sm text-qb-ink-muted">Choose an account to see its register.</div>
          ) : (
            <table className="qb-grid" style={tint ? { backgroundColor: tint } : undefined}>
              {twoLine ? (
                <colgroup>
                  <col className="w-[110px]" />
                  <col className="w-[72px]" />
                  <col className="w-[240px]" />
                  <col />
                  <col className="w-[30px]" />
                  <col className="w-[110px]" />
                  <col className="w-[110px]" />
                  <col className="w-[120px]" />
                  <col className="w-[30px]" />
                </colgroup>
              ) : (
                <colgroup>
                  <col className="w-[92px]" />
                  <col className="w-[110px]" />
                  <col className="w-[64px]" />
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
              )}
              <thead>
                {twoLine ? (
                  <>
                    <tr>
                      {sortTh('date', 'Date')}
                      {sortTh('number', 'No.')}
                      {sortTh('payee', 'Payee / name')}
                      <th className="qb-grid-head" />
                      {clearedTh}
                      {sortTh('first', firstHead, 'text-right')}
                      {sortTh('second', secondHead, 'text-right')}
                      {balanceTh}
                      <th className="qb-grid-head" aria-label="Actions" />
                    </tr>
                    <tr>
                      {sortTh('type', 'Type')}
                      <th className="qb-grid-head" />
                      {sortTh('account', 'Account · class')}
                      {sortTh('memo', 'Memo')}
                      <th className="qb-grid-head" colSpan={5} />
                    </tr>
                  </>
                ) : (
                  <tr>
                    {sortTh('date', 'Date')}
                    {sortTh('type', 'Type')}
                    {sortTh('number', 'No.')}
                    {sortTh('payee', 'Payee / name')}
                    {sortTh('memo', 'Memo')}
                    {sortTh('account', 'Account')}
                    <th className="qb-grid-head">Class</th>
                    {clearedTh}
                    {sortTh('first', firstHead, 'text-right')}
                    {sortTh('second', secondHead, 'text-right')}
                    {balanceTh}
                    <th className="qb-grid-head" aria-label="Actions" />
                  </tr>
                )}
              </thead>
              <tbody>
                {data && (data.startDate || data.rows.length === 0) && (
                  <tr className="bg-qb-canvas">
                    <td className="qb-grid-cell" colSpan={columnCount - 2}>
                      {data.startDate ? `Balance forward as of ${formatDate(data.startDate)}` : 'No transactions yet.'}
                    </td>
                    <td className="qb-grid-cell num font-semibold">{formatCents(data.openingBalanceCents)}</td>
                    <td />
                  </tr>
                )}
                {sorter.sorted.map((r) => {
                  const active = expanded === r.journalEntryId || selected === r.journalEntryId;
                  const rowProps = {
                    'data-entry': r.journalEntryId,
                    className: `qb-grid-row cursor-pointer ${active ? 'is-active' : ''} ${r.isVoided || r.isReversal ? 'text-qb-ink-faint' : ''}`,
                    onClick: () => {
                      setSelected(r.journalEntryId);
                      setExpanded(expanded === r.journalEntryId ? null : r.journalEntryId);
                    },
                    onDoubleClick: () => openTransaction(r),
                    title: `Double-click: ${openLabel(r).toLowerCase()}`,
                  };
                  const otherAccount = (
                    <>
                      {data.includeSubaccounts && r.accountName !== data.accountName ? <span className="text-qb-ink-muted">{r.accountName} · </span> : null}
                      {r.split ?? ''}
                    </>
                  );
                  const memoCell = (
                    <td className="qb-grid-cell text-qb-ink-muted" title={r.memo ?? ''}>
                      {r.isVoided && <span className="mr-1 text-2xs font-semibold uppercase text-qb-negative">Voided</span>}
                      {r.memo ?? ''}
                    </td>
                  );
                  const amounts = (
                    <>
                      <td className="qb-grid-cell text-center font-semibold">{r.reconciled ? 'R' : r.cleared ? 'C' : ''}</td>
                      <td className="qb-grid-cell num">{firstAmount(r) ? formatCents(firstAmount(r)) : ''}</td>
                      <td className="qb-grid-cell num">{secondAmount(r) ? formatCents(secondAmount(r)) : ''}</td>
                      <td className={`qb-grid-cell num font-semibold ${r.balanceCents < 0 ? 'text-qb-negative' : ''} ${inDateOrder ? '' : 'opacity-50'}`}>{formatCents(r.balanceCents)}</td>
                      <td className="qb-grid-cell cell-tight text-center" onClick={(ev) => ev.stopPropagation()}>
                        {!r.isReversal && (
                          <RowMenu
                            items={[
                              { label: `${openLabel(r)}…`, onSelect: () => openTransaction(r) },
                              { label: 'Transaction History…', onSelect: () => setHistory(r.journalEntryId) },
                              { label: 'Transaction Journal', onSelect: () => runJournal(r) },
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
                    </>
                  );
                  return (
                    <Fragment key={r.lineId}>
                      {twoLine ? (
                        <>
                          <tr {...rowProps} className={`${rowProps.className} [&>td]:border-b-0`}>
                            <td className="qb-grid-cell">{formatDate(r.entryDate)}</td>
                            <td className="qb-grid-cell" title={`Entry #${r.entryNumber}`}>{r.num ?? ''}</td>
                            <td className="qb-grid-cell font-medium" title={r.partyName ?? ''}>{r.partyName ?? ''}</td>
                            <td className="qb-grid-cell" />
                            {amounts}
                          </tr>
                          <tr {...rowProps} data-entry={undefined}>
                            <td className="qb-grid-cell text-qb-ink-muted">{typeLabel(r)}</td>
                            <td className="qb-grid-cell" />
                            <td className="qb-grid-cell" title={r.split ?? ''}>
                              {otherAccount}
                              {r.className ? <span className="text-qb-ink-muted"> · {r.className}</span> : null}
                            </td>
                            {memoCell}
                            <td className="qb-grid-cell" colSpan={5} />
                          </tr>
                        </>
                      ) : (
                        <tr {...rowProps}>
                          <td className="qb-grid-cell">{formatDate(r.entryDate)}</td>
                          <td className="qb-grid-cell">{typeLabel(r)}</td>
                          <td className="qb-grid-cell" title={`Entry #${r.entryNumber}`}>{r.num ?? ''}</td>
                          <td className="qb-grid-cell" title={r.partyName ?? ''}>{r.partyName ?? ''}</td>
                          {memoCell}
                          <td className="qb-grid-cell" title={r.split ?? ''}>{otherAccount}</td>
                          <td className="qb-grid-cell text-qb-ink-muted">{r.className ?? ''}</td>
                          {amounts}
                        </tr>
                      )}
                      {expanded === r.journalEntryId && (
                        <tr className="bg-qb-canvas">
                          <td colSpan={columnCount} className="border-b border-qb-border">
                            <EntryLines entryId={r.journalEntryId} />
                            <div className="flex gap-3 px-2 pb-1">
                              <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => openTransaction(r)}>
                                {openLabel(r)} →
                              </button>
                              <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => setHistory(r.journalEntryId)}>
                                Transaction History (Ctrl+H)
                              </button>
                              <button type="button" className="text-sm text-qb-link hover:underline" onClick={() => runJournal(r)}>
                                Transaction Journal (Ctrl+Y)
                              </button>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
        {canEnter && (
          <RegisterEntry
            key={account.id}
            account={account}
            accounts={accounts}
            heads={[firstHead, secondHead]}
            onRecorded={(posted, text) => {
              setSelected(posted.journalEntryId);
              setNotice({ kind: 'success', text });
              scrollToEntry.current = posted.journalEntryId;
            }}
          />
        )}
      </section>
    </div>
  );
}
