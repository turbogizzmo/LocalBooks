import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import MoneyBar from './MoneyBar.jsx';
import Notice from './Notice.jsx';
import { AchFilesPanel, OriginatorDialog, chooseAchPath, nextBusinessDay } from './AchPayments.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import SplitGrid, { blankSplit, splitRowsFrom, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { StatusBadge } from './Invoices.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

const COLS = 10;
export const isCategory = (a) =>
  !a.isSystem && (a.classification === 'EXPENSE' || ['FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_ASSET'].includes(a.accountType));
const isPaymentAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
const isOpen = (b) => !b.isVoided && (b.status === 'UNPAID' || b.status === 'PARTIAL');

/** A new bill, or (with `editing` = { bill, lines }) a posted bill being edited. */
function NewBill({ accounts, vendors, defaultCategory, vendorRef, onPosted, editing = null, onCancelEdit }) {
  const stockItems = useLedgerQuery(api.listInventoryItems);
  const today = todayIso();
  const [vendor, setVendor] = useState(editing?.bill.vendorName ?? '');
  const [billNumber, setBillNumber] = useState(editing?.bill.billNumber ?? '');
  const [billDate, setBillDate] = useState(editing?.bill.billDate ?? today);
  const [dueDate, setDueDate] = useState(editing?.bill.dueDate ?? addDays(today, 30));
  const [total, setTotal] = useState(editing ? centsToInput(editing.bill.totalCents) : '');
  const [rows, setRows] = useState(() => (editing ? splitRowsFrom(editing.lines) : [blankSplit(null)]));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const formClass = useFormClass(accounts);

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

  const [memorizing, setMemorizing] = useState(null);
  const post = async (mode) => {
    if (!vendor.trim()) return setError('Enter a vendor.');
    if (totalCents === null || totalCents <= 0) return setError('Enter the bill total.');
    if (invalid > 0) return setError(`${invalid} split row(s) need a category and an amount above zero.`);
    if (lines.length === 0) return setError('Allocate the bill to at least one category.');
    if (unallocated !== 0) return setError(`Split rows must add up to the bill total; ${formatCents(unallocated)} is unallocated.`);
    if (dueDate < billDate) return setError('Due date cannot be before the bill date.');
    const postedLines = formClass.apply(
      lines.map((l) => ({
        accountId: l.accountId,
        amountCents: l.amountCents,
        memo: l.memo,
        quantityE4: l.quantityE4 ?? null,
        rateCents: l.rateCents ?? null,
        productServiceId: l.productServiceId ?? null,
        classId: l.classId ?? null,
        customerId: l.customerId ?? null,
      })),
    );
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const input = { vendorName: vendor.trim(), billNumber: billNumber.trim() || null, billDate, dueDate, memo: null, totalCents, lines: postedLines };
      if (mode === 'memorize') return setMemorizing({ ...input, billNumber: null });
      const bill = editing ? await api.replaceBill(editing.bill.id, input) : await api.postVendorBill(input);
      onPosted(
        editing
          ? `Bill from ${input.vendorName} replaced: the original is voided and the edited bill is posted.`
          : `Bill from ${bill.vendorName} posted: Cr Accounts Payable ${formatCents(bill.totalCents)} / Dr ${lines.length} split${lines.length === 1 ? '' : 's'}.`,
      );
      if (editing) return onCancelEdit();
      setVendor('');
      setBillNumber('');
      setTotal('');
      setRows([blankSplit(defaultCategory)]);
      formClass.reset();
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
        <h2 className="mr-2 self-center text-md font-semibold">{editing ? 'Edit bill' : 'New bill'}</h2>
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
        {formClass.header}
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
          {editing && (
            <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancelEdit} disabled={busy}>
              Cancel edit
            </button>
          )}
          <button type="button" className="qb-btn qb-btn-secondary" onClick={() => post('memorize')} disabled={busy} title="Save as a recurring transaction">
            Memorize…
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={post} disabled={busy}>
            {busy ? 'Posting…' : editing ? 'Save changes' : 'Post Bill'}
          </button>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} withPricing withClasses withJobs stockItems={stockItems.data ?? []} />
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {memorizing && (
        <MemorizeDialog kind="BILL" payload={memorizing} date={billDate} defaultName={vendor.trim()} onClose={() => setMemorizing(null)} onSaved={(text) => { setMemorizing(null); onPosted(text); }} />
      )}
    </section>
  );
}

const newRequestId = () => (globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`);

function PayBills({ bills, accounts, onDone, onCancel }) {
  const open = bills.filter(isOpen).sort((a, b) => a.dueDate.localeCompare(b.dueDate));
  const defaultAccount = useMemo(() => accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null, [accounts]);
  const [accountId, setAccountId] = useState(defaultAccount);
  const [date, setDate] = useState(todayIso());
  const [selected, setSelected] = useState({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const firstRef = useRef(null);
  // Pay by ACH file: the date vendors are paid, the account's originator details (undefined while
  // loading, null when not set up), and which vendors have bank details.
  const [method, setMethod] = useState('STANDARD');
  const [effectiveDate, setEffectiveDate] = useState(nextBusinessDay(todayIso()));
  const [originator, setOriginator] = useState(undefined);
  const [editingOriginator, setEditingOriginator] = useState(false);
  const [banks, setBanks] = useState({});
  // One per Pay click that succeeds: paying again with it returns the same file instead of paying twice.
  const requestId = useRef(newRequestId());
  const ach = method === 'ACH';
  const account = accounts.find((a) => a.id === accountId);

  useEffect(() => firstRef.current?.focus(), []);

  useEffect(() => {
    if (!ach || accountId === null) return undefined;
    let cancelled = false;
    setOriginator(undefined);
    api.getAchOriginator(accountId).then(
      (o) => !cancelled && setOriginator(o),
      (err) => !cancelled && setError(errorText(err)),
    );
    return () => {
      cancelled = true;
    };
  }, [ach, accountId]);

  const vendorIds = useMemo(() => [...new Set(open.map((b) => b.vendorId))], [bills]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!ach) return undefined;
    let cancelled = false;
    Promise.all(vendorIds.map((id) => api.getVendorBank(id).then((bank) => [id, bank]))).then(
      (pairs) => !cancelled && setBanks(Object.fromEntries(pairs)),
      (err) => !cancelled && setError(errorText(err)),
    );
    return () => {
      cancelled = true;
    };
  }, [ach, vendorIds]);

  const chooseMethod = (next) => {
    setMethod(next);
    setError(null);
    if (next === 'ACH' && account?.accountType !== 'BANK') setAccountId(defaultAccount);
  };

  const toggle = (bill, on) => setSelected((s) => ({ ...s, [bill.id]: on ? centsToInput(bill.balanceCents) : undefined }));
  const picked = open.filter((b) => selected[b.id] !== undefined);
  const cents = (b) => parseMoneyToCents(selected[b.id] ?? '');
  const bad = picked.filter((b) => cents(b) === null || cents(b) <= 0 || cents(b) > b.balanceCents);
  const total = picked.reduce((s, b) => s + Math.max(0, cents(b) ?? 0), 0);
  const noBank = ach ? [...new Set(picked.filter((b) => banks[b.vendorId] === null).map((b) => b.vendorName))] : [];

  const pay = async () => {
    if (picked.length === 0) return setError('Select at least one bill.');
    if (bad.length > 0) return setError('Each payment must be above zero and no more than the open balance.');
    if (picked.some((b) => date < b.billDate)) return setError('The payment date is earlier than one of the bill dates.');
    if (ach) return payByAch();
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

  const payByAch = async () => {
    if (!originator) return setError('Set up this bank account\'s ACH details first.');
    if (noBank.length > 0) return setError(`${noBank.join(', ')} ${noBank.length === 1 ? 'has' : 'have'} no bank details. Add them on the vendor, or pay those bills another way.`);
    if (effectiveDate < date) return setError('The date vendors are paid cannot be before the payment date.');
    const path = await chooseAchPath(`ACH-${todayIso().replaceAll('-', '')}.ach`);
    if (!path) return undefined;
    setBusy(true);
    try {
      const file = await api.payBillsByAch(
        { requestId: requestId.current, paymentAccountId: accountId, paymentDate: date, effectiveDate, bills: picked.map((b) => ({ billId: b.id, amountCents: cents(b) })) },
        path,
      );
      requestId.current = newRequestId();
      const vendors = file.entries.length;
      onDone(
        `Paid ${picked.length} bill${picked.length === 1 ? '' : 's'} to ${vendors} vendor${vendors === 1 ? '' : 's'} by ACH (${formatCents(file.totalCents)}) and saved ${path}. ` +
          'Upload it to your bank once. If this is your first ACH file, have the bank check it before it goes out.',
      );
    } catch (err) {
      const text = errorText(err);
      if (String(text).includes('ACH_FILE_NOT_SAVED')) {
        requestId.current = newRequestId();
        onDone(text, 'error');
      } else {
        setError(text);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="qb-panel flex-none">
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">Pay bills</h2>
        <label>
          <span className="qb-label">Pay by</span>
          <select className="qb-input w-[150px]" value={method} onChange={(e) => chooseMethod(e.target.value)}>
            <option value="STANDARD">Check or transfer</option>
            <option value="ACH">Pay by ACH file</option>
          </select>
        </label>
        <label>
          <span className="qb-label">Payment account</span>
          <AccountSelect
            accounts={accounts}
            filter={ach ? (a) => a.accountType === 'BANK' : isPaymentAccount}
            value={accountId}
            onChange={setAccountId}
            className="qb-input w-[200px]"
          />
        </label>
        <label>
          <span className="qb-label">Payment date</span>
          <input type="date" className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {ach && (
          <label>
            <span className="qb-label">Vendors paid on</span>
            <input type="date" className="qb-input w-[132px]" value={effectiveDate} min={date} onChange={(e) => setEffectiveDate(e.target.value)} />
          </label>
        )}
        {ach && originator !== undefined && (
          <button type="button" className="qb-btn qb-btn-secondary h-row self-end" onClick={() => setEditingOriginator(true)}>
            {originator ? 'ACH details…' : 'Set up ACH…'}
          </button>
        )}
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
                <td className="qb-grid-cell font-semibold">
                  {b.vendorName}
                  {ach && banks[b.vendorId] === null && <span className="ml-2 font-normal text-qb-negative">No bank details</span>}
                  {ach && banks[b.vendorId] && <span className="ml-2 font-normal text-qb-ink-muted">Account {banks[b.vendorId].accountMasked}</span>}
                </td>
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
      {ach && originator === null && (
        <div className="px-2 py-1 text-sm text-qb-ink-muted">
          To pay by ACH file, enter the details your bank gave you for sending ACH files from {account?.name ?? 'this account'}: choose Set up ACH.
        </div>
      )}
      {ach && originator && (
        <div className="px-2 py-1 text-xs text-qb-ink-muted">
          Records one payment per bill and writes one NACHA file that pays each vendor once. Upload it to your bank yourself; check your first file with the bank.
        </div>
      )}
      <div className="flex items-center gap-2 px-2 py-1">
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={pay} disabled={busy || accountId === null || (ach && !originator)}>
            {busy ? 'Paying…' : ach ? `Pay ${picked.length || ''} by ACH file…` : `Pay ${picked.length || ''} selected bill${picked.length === 1 ? '' : 's'}`}
          </button>
        </div>
      </div>
      {editingOriginator && account && (
        <OriginatorDialog
          account={account}
          current={originator}
          onClose={() => setEditingOriginator(false)}
          onSaved={(saved) => {
            setOriginator(saved);
            setEditingOriginator(false);
          }}
        />
      )}
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
  const [editing, setEditing] = useState(null); // { bill, lines }
  const [achFiles, setAchFiles] = useState(false);
  const vendorRef = useRef(null);
  const startEdit = (bill) =>
    api.getDocumentLines('bill', bill.id).then(
      (lines) => {
        setPaying(false);
        setEditing({ bill, lines });
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const today = todayIso();

  useEffect(() => {
    if (intent?.action === 'pay-bills') setPaying(true);
    if (intent?.action === 'bill') {
      setPaying(false);
      vendorRef.current?.focus();
    }
  }, [intent]);

  const bills = data ?? [];
  const sorter = useSort(bills, {
    vendor: (b) => b.vendorName,
    number: (b) => b.billNumber,
    date: (b) => b.billDate,
    due: (b) => b.dueDate,
    category: (b) => (b.categoryAccountId !== null ? accountLabel(accounts, b.categoryAccountId) : null),
    description: (b) => b.firstDescription,
    amount: (b) => b.totalCents,
    balance: (b) => (b.isVoided ? null : b.balanceCents),
    status: (b) => (b.isVoided ? 'VOIDED' : b.status),
  });

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
          onDone={(text, kind = 'success') => {
            setPaying(false);
            setNotice({ kind, text });
          }}
        />
      ) : (
        <NewBill
          key={editing ? `edit-${editing.bill.id}` : 'new'}
          accounts={accounts}
          vendors={vendors.data ?? []}
          defaultCategory={defaultCategory}
          vendorRef={vendorRef}
          editing={editing}
          onCancelEdit={() => setEditing(null)}
          onPosted={(text) => setNotice({ kind: 'success', text })}
        />
      )}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load bills: ${error}`} />}
      {achFiles && <AchFilesPanel onClose={() => setAchFiles(false)} />}

      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Bills tracker</h2>
          <span className="text-xs text-qb-ink-muted">{open.length} open · ▾ for void options</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto h-row" onClick={() => setAchFiles((v) => !v)}>
            ACH files
          </button>
          {!paying && (
            <button type="button" className="qb-btn qb-btn-secondary h-row" onClick={() => setPaying(true)} disabled={open.length === 0}>
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
                <SortTh sorter={sorter} col="vendor">Vendor</SortTh>
                <SortTh sorter={sorter} col="number">Bill no.</SortTh>
                <SortTh sorter={sorter} col="date">Bill date</SortTh>
                <SortTh sorter={sorter} col="due">Due date</SortTh>
                <SortTh sorter={sorter} col="category">Category</SortTh>
                <SortTh sorter={sorter} col="description">Description</SortTh>
                <SortTh sorter={sorter} col="amount" className="text-right">Amount</SortTh>
                <SortTh sorter={sorter} col="balance" className="text-right">Open balance</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && bills.length === 0 && (
                <tr>
                  <td colSpan={COLS} className="qb-grid-cell text-qb-ink-muted">No bills yet.</td>
                </tr>
              )}
              {sorter.sorted.map((b) => {
                const isOverdue = isOpen(b) && b.dueDate < today;
                const livePayments = (payments[b.id] ?? []).filter((p) => !p.isVoided);
                const items = [
                  {
                    label: 'Edit…',
                    disabled: b.isVoided || b.amountPaidCents > 0,
                    hint: b.amountPaidCents > 0 ? 'Void its payments and remove its credits first' : undefined,
                    onSelect: () => startEdit(b),
                  },
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
