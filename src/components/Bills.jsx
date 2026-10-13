import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import MoneyBar from './MoneyBar.jsx';
import Notice from './Notice.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, summarizeSplits } from './SplitGrid.jsx';
import { StatusBadge } from './Invoices.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';

const COLS = 10;
export const isCategory = (a) =>
  !a.isSystem && (a.classification === 'EXPENSE' || ['FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_ASSET'].includes(a.accountType));
const isPaymentAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
const isOpen = (b) => !b.isVoided && (b.status === 'UNPAID' || b.status === 'PARTIAL');

function NewBill({ accounts, vendors, defaultCategory, vendorRef, onPosted }) {
  const today = todayIso();
  const [vendor, setVendor] = useState('');
  const [billNumber, setBillNumber] = useState('');
  const [billDate, setBillDate] = useState(today);
  const [dueDate, setDueDate] = useState(addDays(today, 30));
  const [total, setTotal] = useState('');
  const [rows, setRows] = useState(() => [blankSplit(null)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    if (defaultCategory === null) return;
    setRows((rs) => rs.map((r) => (r.accountId === null ? { ...r, accountId: defaultCategory } : r)));
  }, [defaultCategory]);

  const totalCents = parseMoneyToCents(total);
  const { lines, invalid, total: allocated } = summarizeSplits(rows);
  const unallocated = totalCents === null ? null : totalCents - allocated;

  // With a single split row, the bill total flows into it until the user allocates manually.
  const fillSingleSplit = () => {
    if (rows.length === 1 && rows[0].amount.trim() === '' && totalCents !== null && totalCents > 0) {
      setRows([{ ...rows[0], amount: centsToInput(totalCents) }]);
    }
  };

  const post = async () => {
    if (!vendor.trim()) return setError('Enter a vendor.');
    if (totalCents === null || totalCents <= 0) return setError('Enter the bill total.');
    if (invalid > 0) return setError(`${invalid} split row(s) need a category and an amount above zero.`);
    if (lines.length === 0) return setError('Allocate the bill to at least one category.');
    if (unallocated !== 0) return setError(`Split rows must add up to the bill total; ${formatCents(unallocated)} is unallocated.`);
    if (dueDate < billDate) return setError('Due date cannot be before the bill date.');
    setBusy(true);
    try {
      const bill = await api.postVendorBill({
        vendorName: vendor.trim(),
        billNumber: billNumber.trim() || null,
        billDate,
        dueDate,
        memo: null,
        totalCents,
        lines: lines.map((l) => ({ accountId: l.accountId, amountCents: l.amountCents, memo: l.memo })),
      });
      onPosted(`Bill from ${bill.vendorName} posted: Cr Accounts Payable ${formatCents(bill.totalCents)} / Dr ${lines.length} split${lines.length === 1 ? '' : 's'}.`);
      setVendor('');
      setBillNumber('');
      setTotal('');
      setRows([blankSplit(defaultCategory)]);
      setError(null);
      vendorRef.current?.focus();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New bill</h2>
        <label>
          <span className="qb-label">Vendor</span>
          <input ref={vendorRef} className="qb-input w-[200px]" value={vendor} onChange={(e) => setVendor(e.target.value)} list="bill-vendors" placeholder="Vendor" />
          <datalist id="bill-vendors">
            {vendors.map((v) => (
              <option key={v.id} value={v.name} />
            ))}
          </datalist>
        </label>
        <label>
          <span className="qb-label">Bill no.</span>
          <input className="qb-input w-[100px]" value={billNumber} onChange={(e) => setBillNumber(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Bill date</span>
          <input
            type="date"
            className="qb-input w-[132px]"
            value={billDate}
            onChange={(e) => {
              setBillDate(e.target.value);
              setDueDate(addDays(e.target.value, 30));
            }}
          />
        </label>
        <label>
          <span className="qb-label">Due date</span>
          <input type="date" className="qb-input w-[132px]" value={dueDate} onChange={(e) => setDueDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Bill total</span>
          <input
            className="qb-input num w-[120px]"
            value={total}
            onChange={(e) => setTotal(e.target.value)}
            onBlur={fillSingleSplit}
            placeholder="0.00"
            inputMode="decimal"
            aria-invalid={total.trim() !== '' && (totalCents === null || totalCents <= 0)}
          />
        </label>
        <div className="ml-auto flex items-center gap-3 self-center">
          <span className={`text-sm ${unallocated === 0 ? 'text-qb-green-active' : unallocated === null ? 'text-qb-ink-muted' : 'text-qb-negative'}`}>
            {unallocated === null ? 'Enter the bill total' : unallocated === 0 ? '✓ Fully allocated' : `Unallocated ${formatCents(unallocated)}`}
          </span>
          <button type="button" className="qb-btn qb-btn-primary" onClick={post} disabled={busy}>
            {busy ? 'Posting…' : 'Post Bill'}
          </button>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} />
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
    </section>
  );
}

function PayBills({ bills, accounts, onDone, onCancel }) {
  const open = bills.filter(isOpen).sort((a, b) => a.dueDate.localeCompare(b.dueDate));
  const defaultAccount = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const [accountId, setAccountId] = useState(defaultAccount);
  const [date, setDate] = useState(todayIso());
  const [selected, setSelected] = useState({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const firstRef = useRef(null);

  useEffect(() => firstRef.current?.focus(), []);

  const toggle = (bill, on) => setSelected((s) => ({ ...s, [bill.id]: on ? centsToInput(bill.balanceCents) : undefined }));
  const picked = open.filter((b) => selected[b.id] !== undefined);
  const cents = (b) => parseMoneyToCents(selected[b.id] ?? '');
  const bad = picked.filter((b) => cents(b) === null || cents(b) <= 0 || cents(b) > b.balanceCents);
  const total = picked.reduce((s, b) => s + Math.max(0, cents(b) ?? 0), 0);

  const pay = async () => {
    if (picked.length === 0) return setError('Select at least one bill.');
    if (bad.length > 0) return setError('Each payment must be above zero and no more than the open balance.');
    if (picked.some((b) => date < b.billDate)) return setError('The payment date is earlier than one of the bill dates.');
    setBusy(true);
    try {
      const count = await api.payVendorBills(
        picked.map((b) => ({ billId: b.id, paymentAccountId: accountId, paymentDate: date, amountCents: cents(b), reference: null })),
      );
      onDone(`Paid ${count} bill${count === 1 ? '' : 's'} totalling ${formatCents(total)}.`);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Pay bills</h2>
        <label>
          <span className="qb-label">Payment account</span>
          <AccountSelect accounts={accounts} filter={isPaymentAccount} value={accountId} onChange={setAccountId} className="qb-input w-[200px]" />
        </label>
        <label>
          <span className="qb-label">Payment date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <div className="ml-auto text-right">
          <div className="qb-label">Total payment</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
        </div>
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[32px]" />
          <col />
          <col className="w-[100px]" />
          <col className="w-[100px]" />
          <col className="w-[120px]" />
          <col className="w-[130px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head" aria-label="Select" />
            <th className="qb-grid-head">Vendor</th>
            <th className="qb-grid-head">Bill no.</th>
            <th className="qb-grid-head">Due date</th>
            <th className="qb-grid-head text-right">Open balance</th>
            <th className="qb-grid-head text-right">Payment</th>
          </tr>
        </thead>
        <tbody>
          {open.length === 0 && (
            <tr>
              <td colSpan={6} className="qb-grid-cell text-qb-ink-muted">No open bills to pay.</td>
            </tr>
          )}
          {open.map((b, i) => {
            const on = selected[b.id] !== undefined;
            return (
              <tr key={b.id} className={`qb-grid-row ${on ? 'is-active' : ''}`}>
                <td className="qb-grid-cell text-center">
                  <input ref={i === 0 ? firstRef : undefined} type="checkbox" className="accent-qb-green" checked={on} onChange={(e) => toggle(b, e.target.checked)} aria-label={`Pay ${b.vendorName}`} />
                </td>
                <td className="qb-grid-cell font-semibold">{b.vendorName}</td>
                <td className="qb-grid-cell">{b.billNumber ?? ''}</td>
                <td className={`qb-grid-cell ${b.dueDate < todayIso() ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(b.dueDate)}</td>
                <td className="qb-grid-cell num">{formatCents(b.balanceCents)}</td>
                <td className="qb-grid-cell-edit">
                  <input
                    className="num"
                    value={selected[b.id] ?? ''}
                    onChange={(e) => setSelected((s) => ({ ...s, [b.id]: e.target.value }))}
                    onFocus={() => !on && toggle(b, true)}
                    placeholder="0.00"
                    inputMode="decimal"
                    aria-invalid={bad.includes(b)}
                    aria-label={`Payment to ${b.vendorName}`}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={pay} disabled={busy || accountId === null}>
            {busy ? 'Paying…' : `Pay ${picked.length || ''} selected bill${picked.length === 1 ? '' : 's'}`}
          </button>
        </div>
      </div>
    </section>
  );
}

export default function Bills({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listBills);
  const vendors = useLedgerQuery(api.listVendors);
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && isCategory(a) && a.classification === 'EXPENSE')?.id ?? null, [accounts]);
  const [notice, setNotice] = useState(null);
  const [paying, setPaying] = useState(intent?.action === 'pay-bills');
  const [payments, setPayments] = useState({});
  const vendorRef = useRef(null);
  const today = todayIso();

  useEffect(() => {
    if (intent?.action === 'pay-bills') setPaying(true);
    if (intent?.action === 'bill') {
      setPaying(false);
      vendorRef.current?.focus();
    }
  }, [intent]);

  const bills = data ?? [];

  // Payments feed the per-row void menu; refreshed whenever the bills change.
  useEffect(() => {
    let cancelled = false;
    const withPayments = bills.filter((b) => b.lastPaymentDate !== null || b.amountPaidCents > 0);
    Promise.all(withPayments.map((b) => api.listBillPayments(b.id).then((list) => [b.id, list]))).then(
      (pairs) => !cancelled && setPayments(Object.fromEntries(pairs)),
      (err) => !cancelled && setNotice({ kind: 'error', text: errorText(err) }),
    );
    return () => {
      cancelled = true;
    };
  }, [data]); // eslint-disable-line react-hooks/exhaustive-deps

  const voidEntry = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Voided: a reversing journal entry now offsets it.' });
  };

  const open = bills.filter(isOpen);
  const overdue = open.filter((b) => b.dueDate < today);
  const since = addDays(today, -30);
  const paidRecent = bills.filter((b) => !b.isVoided && b.lastPaymentDate && b.lastPaymentDate >= since);
  const sum = (list, key) => list.reduce((s, b) => s + b[key], 0);

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      <MoneyBar
        items={[
          { label: 'Open bills', icon: '○', color: '#0077c5', cents: sum(open, 'balanceCents'), count: open.length },
          { label: 'Overdue', icon: '⚠', color: '#d03b3b', cents: sum(overdue, 'balanceCents'), count: overdue.length },
          { label: 'Paid last 30 days', icon: '✓', color: '#0ca30c', cents: sum(paidRecent, 'amountPaidCents'), count: paidRecent.length },
        ]}
      />
      {paying ? (
        <PayBills
          bills={bills}
          accounts={accounts}
          onCancel={() => setPaying(false)}
          onDone={(text) => {
            setPaying(false);
            setNotice({ kind: 'success', text });
          }}
        />
      ) : (
        <NewBill
          accounts={accounts}
          vendors={vendors.data ?? []}
          defaultCategory={defaultCategory}
          vendorRef={vendorRef}
          onPosted={(text) => setNotice({ kind: 'success', text })}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load bills: ${error}`} />}

      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Bills tracker</h2>
          <span className="text-xs text-qb-ink-muted">{open.length} open · ▾ for void options</span>
          {!paying && (
            <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => setPaying(true)} disabled={open.length === 0}>
              Pay bills
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[180px]" />
              <col className="w-[92px]" />
              <col className="w-[92px]" />
              <col className="w-[92px]" />
              <col className="w-[220px]" />
              <col />
              <col className="w-[112px]" />
              <col className="w-[112px]" />
              <col className="w-[92px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <th className="qb-grid-head">Vendor</th>
                <th className="qb-grid-head">Bill no.</th>
                <th className="qb-grid-head">Bill date</th>
                <th className="qb-grid-head">Due date</th>
                <th className="qb-grid-head">Category</th>
                <th className="qb-grid-head">Description</th>
                <th className="qb-grid-head text-right">Amount</th>
                <th className="qb-grid-head text-right">Open balance</th>
                <th className="qb-grid-head">Status</th>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && bills.length === 0 && (
                <tr>
                  <td colSpan={COLS} className="qb-grid-cell text-qb-ink-muted">No bills yet.</td>
                </tr>
              )}
              {bills.map((b) => {
                const isOverdue = isOpen(b) && b.dueDate < today;
                const livePayments = (payments[b.id] ?? []).filter((p) => !p.isVoided);
                const items = [
                  voidItem({
                    journalEntryId: b.journalEntryId,
                    voided: b.isVoided,
                    description: `Bill from ${b.vendorName}${b.billNumber ? ` #${b.billNumber}` : ''} (${formatCents(b.totalCents)}).${livePayments.length ? ' Its payments must be voided first.' : ''}`,
                    onVoid: voidEntry,
                  }),
                  ...livePayments.map((p) => ({
                    ...voidItem({
                      journalEntryId: p.journalEntryId,
                      voided: false,
                      description: `Payment of ${formatCents(p.amountCents)} to ${b.vendorName} on ${formatDate(p.paymentDate)}. The bill reopens for that amount.`,
                      onVoid: voidEntry,
                    }),
                    label: `Void payment ${formatCents(p.amountCents)} · ${formatDate(p.paymentDate)}`,
                  })),
                ];
                return (
                  <Fragment key={b.id}>
                    <tr className={`qb-grid-row ${b.isVoided ? 'text-qb-ink-faint' : ''}`}>
                      <td className="qb-grid-cell font-semibold" title={b.vendorName}>{b.vendorName}</td>
                      <td className="qb-grid-cell">{b.billNumber ?? ''}</td>
                      <td className="qb-grid-cell">{formatDate(b.billDate)}</td>
                      <td className={`qb-grid-cell ${isOverdue ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(b.dueDate)}</td>
                      <td className="qb-grid-cell" title={accountLabel(accounts, b.categoryAccountId)}>
                        {b.categoryAccountId !== null ? accountLabel(accounts, b.categoryAccountId) : <span className="text-qb-ink-muted">Split ({b.lineCount} lines)</span>}
                      </td>
                      <td className="qb-grid-cell text-qb-ink-muted" title={b.firstDescription ?? ''}>{b.firstDescription ?? ''}</td>
                      <td className="qb-grid-cell num">{formatCents(b.totalCents)}</td>
                      <td className="qb-grid-cell num font-semibold">{!b.isVoided && b.balanceCents ? formatCents(b.balanceCents) : ''}</td>
                      <td className="qb-grid-cell">
                        <StatusBadge status={b.status} overdue={isOverdue} voided={b.isVoided} />
                      </td>
                      <td className="qb-grid-cell cell-tight text-center">
                        <RowMenu items={items} />
                      </td>
                    </tr>
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
