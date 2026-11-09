import { PrintLaterBox, usePrintLater } from './PrintLater.jsx';
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { useMyPreferences, useTransactionDate } from '../lib/myPreferences.js';
import NameField from './NameField.jsx';
import { useVendorRecall } from '../lib/recall.js';
import AccountSelect, { accountLabel, EndingBalance } from './AccountSelect.jsx';
import FormToolbar, { attachLaterButton, duplicateButton, fileButtons, navButtons } from './FormToolbar.jsx';
import Attachments, { PendingAttachments, attachPicked, useAttachmentRows } from './Attachments.jsx';
import { useVoucherActions } from './VoucherPrint.jsx';
import MoneyBar from './MoneyBar.jsx';
import Notice from './Notice.jsx';
import { AchFilesPanel, OriginatorDialog, chooseAchPath, nextBusinessDay } from './AchPayments.jsx';
import { MemorizeDialog } from './Recurring.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { FormInfoPanel } from './InfoPanel.jsx';
import { useTransactionReports } from './TransactionHistory.jsx';
import SplitGrid, { blankSplit, splitRowsFrom, summarizeSplits } from './SplitGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { TermsSelect, useTerms } from './ProfileFields.jsx';
import { StatusBadge } from './Invoices.jsx';
import { isHeldForApproval } from '../lib/approvals.js';
import { api, errorText, printDocument, useLedgerQuery } from '../lib/api.js';
import { CheckDocument, PrintPortal, useCompany } from './PrintDocuments.jsx';
import { centsToInput, formatCents, parseMoneyToCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import { useDirtySince, useEditIntent, useOpenFromList, useWindowContent } from '../lib/windows.js';
import DateBox, { CheckNumberBox } from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { useSalesPrefs } from '../lib/salesPrefs.js';
import { byDateThenNumber, discountDate, duplicateDates, stepButtons } from '../lib/formStep.js';
import { PAY_BILLS_SORTS, payBillsVendors, payBillsView } from '../lib/payBillsView.js';

const COLS = 10;
export const isCategory = (a) =>
  !a.isSystem && (a.classification === 'EXPENSE' || ['FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_ASSET'].includes(a.accountType));
const isPaymentAccount = (a) => a.accountType === 'BANK' || a.accountType === 'CREDIT_CARD';
const isOpen = (b) => !b.isVoided && (b.status === 'UNPAID' || b.status === 'PARTIAL');

/**
 * A new bill, or (with `editing` = { bill, lines }) a posted bill being edited, or (with `copyOf`,
 * the same shape) a new bill copied from a saved one by Duplicate: dated today, its own number left
 * blank, due as many days later as the original was.
 */
function NewBill({ accounts, vendors, defaultCategory, vendorRef, onPosted, editing = null, copyOf = null, onDuplicate, onCancelEdit, saved = [], onOpenSaved }) {
  const stockItems = useLedgerQuery(api.listInventoryItems);
  const source = editing ?? copyOf;
  const copyDates = copyOf ? duplicateDates(copyOf.bill.billDate, copyOf.bill.dueDate, todayIso()) : null;
  const [vendor, setVendor] = useState(source?.bill.vendorName ?? '');
  const [billNumber, setBillNumber] = useState(editing?.bill.billNumber ?? '');
  const [billDate, setBillDate] = useTransactionDate(editing?.bill.billDate ?? copyDates?.date);
  const [dueDate, setDueDate] = useState(editing?.bill.dueDate ?? copyDates?.due ?? billDate);
  const termsList = useTerms();
  const [termsId, setTermsId] = useState(source?.bill.termsId ?? null);
  // Set once the user picks a term (or a saved bill is loaded): the vendor's term no longer fills in.
  const [termsTouched, setTermsTouched] = useState(Boolean(source));
  // Without a term, the bill is due after the vendor's days, or Preferences › Bills' days (10
  // unless an Admin changes them), as the backend works it out.
  const applyTerms = (t, fromDate) => {
    if (!fromDate) return;
    if (t) setDueDate(addDays(fromDate, t.netDays));
    else api.getBillDueDate(vendor.trim(), fromDate).then(setDueDate, () => {});
  };
  // Discount Date, beside Terms and the due date, when the term offers an early-payment discount.
  const billDiscountDate = discountDate(termsList.find((t) => t.id === termsId) ?? null, billDate);
  const vendorRecord = vendors.find((v) => v.name.toLowerCase() === vendor.trim().toLowerCase());
  useEffect(() => {
    if (termsTouched) return;
    const t = termsList.find((x) => x.isActive && x.id === vendorRecord?.termsId) ?? null;
    setTermsId(t?.id ?? null);
    applyTerms(t, billDate);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [vendor, vendorRecord?.termsId, termsList, termsTouched]);
  const [total, setTotal] = useState(source ? centsToInput(source.bill.totalCents) : '');
  const [rows, setRows] = useState(() => (source ? splitRowsFrom(source.lines) : [blankSplit(null)]));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  // Files to attach once the bill posts, such as the vendor's invoice.
  const [files, setFiles] = useState([]);
  const [showAttachments, setShowAttachments] = useState(false);
  // Print ▾ (Print…, Save as PDF…) and Email for the saved voucher; errors show on the form.
  const [voucherElements, voucherItems] = useVoucherActions((n) => setError(n.kind === 'error' ? n.text : null));
  const formClass = useFormClass(accounts);
  // Closing the window (or stepping to another bill) asks first while the bill has entries not
  // yet saved. The due date is left out: it fills in by itself from the date and terms.
  const dirty = useDirtySince('bill', { vendor, billNumber, billDate, total, rows: rows.map((r) => [r.memo, r.amount, r.qty, r.rate]) }, Boolean(copyOf));

  useEffect(() => {
    if (defaultCategory === null) return;
    setRows((rs) => rs.map((r) => (r.accountId === null ? { ...r, accountId: defaultCategory } : r)));
  }, [defaultCategory]);

  // My Preferences › General › recall: a known vendor's last bill fills the rows (and total).
  useVendorRecall({
    vendor,
    vendors,
    rows,
    enabled: !source,
    kind: 'bill',
    onRecalled: (recalled, recalledRows) => {
      setRows(recalledRows);
      if (recalledRows.some((r) => r.amount || r.rate)) setTotal((t) => t || centsToInput(recalled.totalCents));
    },
  });

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
  // Payments and vendor credits applied stay applied to the edited version.
  const appliedText = editing?.bill.amountPaidCents > 0 ? `This bill has ${formatCents(editing.bill.amountPaidCents)} in payments or credits applied. Saving will keep them applied to the new version.` : null;
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
    // Said before any "save anyway?" question; saving checks it again.
    const paidCents = editing?.bill.amountPaidCents ?? 0;
    if (paidCents > 0 && totalCents < paidCents) {
      return setError(`This bill has ${formatCents(paidCents)} in payments and credits applied; the edited total, ${formatCents(totalCents)}, is less. Void or remove a payment or credit first.`);
    }
    setBusy(true);
    try {
      const input = { vendorName: vendor.trim(), billNumber: billNumber.trim() || null, billDate, dueDate, memo: null, totalCents, lines: postedLines, termsId };
      if (mode === 'memorize') return setMemorizing({ ...input, billNumber: null });
      let bill;
      try {
        bill = editing ? await api.replaceBill(editing.bill.id, input, appliedText) : await api.postVendorBill(input);
      } catch (err) {
        // An approval rule holds this bill: it waits under Expenses › Approvals instead of posting.
        if (editing || !isHeldForApproval(err)) throw err;
        await api.submitForApproval('BILL', input);
        onPosted(`Bill from ${input.vendorName} sent for approval: ${formatCents(totalCents)}. It isn't posted, and isn't in Pay Bills, until it is approved under Expenses › Approvals.${files.length ? ' Attach the files once it is approved.' : ''}`);
        setFiles([]);
        setVendor('');
        setBillNumber('');
        setTotal('');
        setTermsTouched(false);
        setTermsId(null);
        setRows([blankSplit(defaultCategory)]);
        formClass.reset();
        setError(null);
        vendorRef.current?.focus();
        return true;
      }
      const attachError = !editing && files.length > 0 ? await attachPicked(bill.journalEntryId, files) : null;
      onPosted(
        editing
          ? `Bill from ${input.vendorName} replaced: the original is voided and the edited bill is posted.`
          : `Bill from ${bill.vendorName} posted: Cr Accounts Payable ${formatCents(bill.totalCents)} / Dr ${lines.length} split${lines.length === 1 ? '' : 's'}.` +
              (files.length && !attachError ? ` ${files.length} file${files.length === 1 ? '' : 's'} attached.` : ''),
      );
      if (attachError) setError(attachError);
      if (editing) return true;
      setFiles([]);
      setVendor('');
      setBillNumber('');
      setTotal('');
      setTermsTouched(false);
      setTermsId(null);
      setRows([blankSplit(defaultCategory)]);
      formClass.reset();
      setError(null);
      vendorRef.current?.focus();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  // The toolbar across the top of the form (FormToolbar.jsx).
  const openFromList = useOpenFromList();
  const clear = () => {
    if (editing) return onCancelEdit();
    setFiles([]);
    setVendor('');
    setBillNumber('');
    setTotal('');
    setRows([blankSplit(defaultCategory)]);
    formClass.reset();
    setError(null);
    vendorRef.current?.focus();
  };
  const voidBill = () =>
    api.voidTransaction(editing.bill.journalEntryId).then(() => {
      onPosted(`Bill from ${editing.bill.vendorName} voided: a reversing entry now offsets it.`);
      onCancelEdit();
    });
  // History (Ctrl+H) and Journal (Ctrl+Y) of the bill on the form.
  const reports = useTransactionReports();
  // A bill being edited closes back to a new one; a new bill's window closes (formFlow.jsx).
  const flow = useFormFlow({ save: () => post('post'), clear, onClose: editing ? onCancelEdit : undefined, afterNew: editing ? onCancelEdit : undefined, busy });
  const toolbar = (
    <FormToolbar
      label="Bill"
      groups={[
        navButtons({
          onFind: () => openFromList('accounting/find', 'find', { text: '' }),
          ...stepButtons(saved, editing?.bill.id ?? null, onOpenSaved, { blocked: busy || dirty }),
        }),
        fileButtons({
          onNew: clear,
          onSave: flow.saveAndNew,
          busy,
          onVoid: editing ? voidBill : undefined,
          voidDisabled: !editing?.bill.journalEntryId || editing.bill.isVoided,
          voidConfirm: editing && {
            title: 'Void this transaction?',
            body: `The bill from ${editing.bill.vendorName} (${formatCents(editing.bill.totalCents)}). A reversing journal entry dated the same day will zero out its effect. The original stays in the books and the audit log. This cannot be undone.`,
            confirmLabel: 'Yes, void it',
          },
        }),
        [
          ...voucherItems(
            editing?.bill.journalEntryId
              ? {
                  entryId: editing.bill.journalEntryId,
                  title: 'Bill',
                  number: editing.bill.billNumber,
                  date: editing.bill.billDate,
                  dueDate: editing.bill.dueDate,
                  name: editing.bill.vendorName,
                  nameLabel: 'Vendor',
                  amountCents: editing.bill.totalCents,
                }
              : null,
            { hint: 'Save the bill first, then print it' },
          ),
          editing
            ? {
                id: 'attach',
                label: 'Attach File',
                icon: 'Attach',
                hint: 'Show and add files attached to this bill',
                disabled: busy || !editing.bill.journalEntryId,
                onClick: () => setShowAttachments((v) => !v),
              }
            : attachLaterButton({ setFiles, busy, what: 'this bill' }),
        ],
        [
          { id: 'memorize', label: 'Memorize', icon: 'Memorize', hint: 'Save as a recurring bill', disabled: busy, onClick: () => post('memorize') },
          duplicateButton({ onDuplicate: () => onDuplicate?.(editing), saved: Boolean(editing && onDuplicate), dirty, busy, what: 'bill' }),
        ],
        reports.buttons({ journalEntryId: editing?.bill.journalEntryId, label: `Bill${editing?.bill.billNumber ? ` #${editing.bill.billNumber}` : ''}` }),
      ]}
    />
  );

  return (
    <div className="flex flex-none gap-2">
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel min-w-0 flex-1">
      {reports.panel}
      {voucherElements}
      {toolbar}
      {showAttachments && editing?.bill.journalEntryId && (
        <div className="border-b border-qb-border bg-qb-canvas">
          <Attachments entryId={editing.bill.journalEntryId} />
        </div>
      )}
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">{editing ? 'Edit bill' : 'New bill'}</h2>
        <label>
          <span className="qb-label">Vendor</span>
          <NameField kind="vendor" inputRef={vendorRef} names={vendors} value={vendor} onChange={setVendor} />
        </label>
        <label>
          <span className="qb-label">Bill no.</span>
          <input className="qb-input w-[100px]" value={billNumber} onChange={(e) => setBillNumber(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Bill date</span>
          <DateBox
            className="qb-input w-[132px]"
            value={billDate}
            onChange={(e) => {
              setBillDate(e.target.value);
              applyTerms(termsList.find((t) => t.id === termsId) ?? null, e.target.value);
            }}
          />
        </label>
        <label>
          <span className="qb-label">Terms</span>
          <TermsSelect
            className="qb-input w-[128px]"
            value={termsId}
            onChange={(t) => {
              setTermsTouched(true);
              setTermsId(t?.id ?? null);
              applyTerms(t, billDate);
            }}
          />
        </label>
        <label>
          <span className="qb-label">Due date</span>
          <DateBox className="qb-input w-[132px]" value={dueDate} onChange={(e) => setDueDate(e.target.value)} />
        </label>
        {billDiscountDate && (
          <div title="Pay by this date to take the term's early-payment discount in Pay Bills">
            <span className="qb-label">Discount date</span>
            <div className="flex h-input items-center text-sm">{formatDate(billDiscountDate)}</div>
          </div>
        )}
        {formClass.header}
        <label>
          <span className="qb-label">Bill total</span>
          <input
            className="qb-input num w-[120px]"
            value={total}
            onChange={(e) => setTotal(e.target.value)}
            onBlur={fillSingleSplit}
            placeholder="0.00"
            inputMode="decimal" data-money
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
          <SaveButtons flow={flow} busyLabel="Posting…" />
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} withPricing withClasses withJobs stockItems={stockItems.data ?? []} />
      {!editing && (
        <div className="border-t border-qb-border px-2 py-1">
          <PendingAttachments paths={files} onChange={setFiles} label={files.length ? 'Attach another…' : "Attach the vendor's invoice…"} />
        </div>
      )}
      {appliedText && <Notice kind="info" text={appliedText} />}
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
      {memorizing && (
        <MemorizeDialog kind="BILL" payload={memorizing} date={billDate} defaultName={vendor.trim()} onClose={() => setMemorizing(null)} onSaved={(text) => { setMemorizing(null); onPosted(text); }} />
      )}
    </section>
    {/* The chosen vendor's information panel (T56); it can be hidden. */}
    <FormInfoPanel kind="vendor" name={vendor} names={vendors} />
    </div>
  );
}

const newRequestId = () => (globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`);

function PayBills({ bills, accounts, onDone, onCancel }) {
  const open = bills.filter(isOpen).sort((a, b) => a.dueDate.localeCompare(b.dueDate));
  // Pay Bills opens on the account chosen in Preferences › Checking (My Preferences), else the first bank.
  const { payBillsAccountId } = useMyPreferences();
  const defaultAccount = useMemo(
    () => accounts.find((a) => a.id === payBillsAccountId && a.isActive && isPaymentAccount(a))?.id ?? accounts.find((a) => a.isActive && a.accountType === 'BANK')?.id ?? null,
    [accounts, payBillsAccountId],
  );
  const [accountId, setAccountId] = useState(defaultAccount);
  const [date, setDate] = useState(todayIso());
  const [selected, setSelected] = useState({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const firstRef = useRef(null);
  // Filter By (one vendor or all) and Sort By above the list; the bill clicked last shows below it.
  const [filterVendor, setFilterVendor] = useState(null);
  const [sortBy, setSortBy] = useState('due');
  const [highlight, setHighlight] = useState(null);
  const openFromList = useOpenFromList();
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
  // Paid from a bank account by check: one check per vendor, numbered in turn from the first.
  const checks = !ach && account?.accountType === 'BANK';
  const [checkNo, setCheckNo] = useState('');
  const [printChecks, setPrintChecks] = useState(true);
  // Print later (as on Write Checks): the checks wait in File › Print Forms › Checks unnumbered.
  const printLater = usePrintLater('CHECK', checks);

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

  useEffect(() => {
    if (!checks) return undefined;
    let cancelled = false;
    api.nextCheckNumber(accountId).then(
      (n) => !cancelled && setCheckNo(n),
      (err) => !cancelled && setError(errorText(err)),
    );
    return () => {
      cancelled = true;
    };
  }, [checks, accountId]);

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

  const toggle = (bill, on) => {
    setTyped((t) => {
      const next = new Set(t);
      next.delete(bill.id);
      return next;
    });
    setSelected((s) => ({ ...s, [bill.id]: on ? centsToInput(bill.balanceCents) : undefined }));
  };
  const picked = open.filter((b) => selected[b.id] !== undefined);
  // The list as filtered and sorted; ticked bills a filter hides are still paid.
  const shown = payBillsView(open, { vendorId: filterVendor, sortBy });
  const hiddenPicked = picked.filter((b) => !shown.includes(b)).length;
  const highlightedBill = shown.find((b) => b.id === highlight) ?? null;
  // Early-payment discounts the terms still offer, then vendor credits (oldest first), taken off the
  // ticked bills as Preferences › Bills says; not with ACH.
  const { autoCredits, autoDiscounts } = useSalesPrefs();
  const [credits, setCredits] = useState([]);
  const [discounts, setDiscounts] = useState([]);
  const [typed, setTyped] = useState(() => new Set());
  const pickedKey = picked.map((b) => b.id).join(',');
  useEffect(() => {
    if (!(autoCredits || autoDiscounts) || ach || pickedKey === '' || !date) {
      setCredits([]);
      setDiscounts([]);
      return undefined;
    }
    let live = true;
    api.getPayBillsPlan(pickedKey.split(',').map(Number), date).then(
      (plan) => {
        if (!live) return;
        setCredits(plan.credits);
        setDiscounts(plan.discounts);
      },
      (err) => live && setError(errorText(err)),
    );
    return () => {
      live = false;
    };
  }, [autoCredits, autoDiscounts, ach, pickedKey, date]);
  const discountOn = (b) => discounts.find((x) => x.billId === b.id)?.amountCents ?? 0;
  // What comes off a bill before it is paid: its discount and the credits used on it.
  const creditOn = (b) => discountOn(b) + credits.filter((c) => c.billId === b.id).reduce((s, c) => s + c.amountCents, 0);
  // A ticked bill's payment is what the credits leave, until the person types an amount.
  useEffect(() => {
    setSelected((s) => {
      let changed = false;
      const next = { ...s };
      for (const b of open) {
        if (s[b.id] === undefined || typed.has(b.id)) continue;
        const left = centsToInput(b.balanceCents - creditOn(b));
        if (next[b.id] !== left) {
          next[b.id] = left;
          changed = true;
        }
      }
      return changed ? next : s;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [credits, discounts]);
  const cents = (b) => parseMoneyToCents(selected[b.id] ?? '');
  const bad = picked.filter((b) => cents(b) === null || cents(b) < 0 || (cents(b) === 0 && creditOn(b) === 0) || cents(b) + creditOn(b) > b.balanceCents);
  const total = picked.reduce((s, b) => s + Math.max(0, cents(b) ?? 0), 0);
  const creditTotal = picked.reduce((s, b) => s + creditOn(b), 0);
  // Only bills with something left to pay get a payment (and a check).
  const paying = picked.filter((b) => (cents(b) ?? 0) > 0);
  const payees = [...new Set(paying.map((b) => b.vendorId))];
  const firstCheck = checkNo.trim();
  const numbered = /^\d+$/.test(firstCheck);
  const checkNumber = (vendorId) => {
    if (!checks || !firstCheck) return null;
    const i = payees.indexOf(vendorId);
    return numbered ? String(BigInt(firstCheck) + BigInt(i)) : firstCheck;
  };
  const lastCheck = payees.length > 1 && numbered ? checkNumber(payees[payees.length - 1]) : null;
  const noBank = ach ? [...new Set(picked.filter((b) => banks[b.vendorId] === null).map((b) => b.vendorName))] : [];

  const pay = async () => {
    if (picked.length === 0) return setError('Select at least one bill.');
    if (bad.length > 0) return setError('Each payment must be above zero (unless credits cover the bill) and, with its credits, no more than the open balance.');
    if (ach) return payByAch();
    if (checks && !printLater.on && firstCheck && !numbered && payees.length > 1) return setError('To number a check for each vendor, the check number must be digits only.');
    setBusy(true);
    try {
      const queued = printLater.on;
      const payments = paying.map((b) => ({ billId: b.id, paymentAccountId: accountId, paymentDate: date, amountCents: cents(b), reference: queued ? null : checkNumber(b.vendorId), toBePrinted: queued }));
      const used = credits.filter((c) => picked.some((b) => b.id === c.billId)).map((c) => ({ creditId: c.creditId, documentId: c.billId, amountCents: c.amountCents, appliedDate: date }));
      const taken = discounts.filter((x) => picked.some((b) => b.id === x.billId));
      const count = used.length + taken.length > 0 ? await api.payBillsWithCredits(payments, used, taken, date) : await api.payVendorBills(payments);
      const written = checks
        ? payees.map((vendorId) => {
            const theirs = paying.filter((b) => b.vendorId === vendorId);
            const numbers = theirs.map((b) => b.billNumber).filter(Boolean);
            return {
              payeeName: theirs[0].vendorName,
              expenseDate: date,
              totalCents: theirs.reduce((s, b) => s + cents(b), 0),
              currencyCode: null,
              checkNumber: checkNumber(vendorId),
              memo: numbers.length ? `Bill${numbers.length === 1 ? '' : 's'} ${numbers.join(', ')}` : null,
              paymentAccountId: accountId,
            };
          })
        : [];
      if (checks) printLater.remember(queued);
      const range = queued
        ? `; ${payees.length} check${payees.length === 1 ? ' waits' : 's wait'} in File › Print Forms › Checks`
        : written.length && firstCheck
          ? ` with check${written.length === 1 ? ` #${firstCheck}` : `s #${firstCheck}–${lastCheck ?? firstCheck}`}`
          : '';
      const creditNote = used.length + taken.length > 0 ? `, after ${formatCents(creditTotal)} of discounts and credits` : '';
      onDone(`Paid ${count} bill${count === 1 ? '' : 's'} totalling ${formatCents(total)}${creditNote}${range}.`, 'success', printChecks && !queued ? written : []);
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
        <EndingBalance accounts={accounts} accountId={accountId} />
        <label>
          <span className="qb-label">Payment date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        {checks && !printLater.on && (
          <label>
            <span className="qb-label">{payees.length > 1 ? 'First check no.' : 'Check no.'}</span>
            <CheckNumberBox className="qb-input num w-[90px]" value={checkNo} onChange={(e) => setCheckNo(e.target.value)} aria-label="Check number" />
          </label>
        )}
        {checks && !printLater.on && (
          <label className="flex h-row items-center gap-1 self-end text-sm">
            <input type="checkbox" className="accent-qb-green" checked={printChecks} onChange={(e) => setPrintChecks(e.target.checked)} />
            Print checks
          </label>
        )}
        {checks && <PrintLaterBox printLater={printLater} />}
        {ach && (
          <label>
            <span className="qb-label">Vendors paid on</span>
            <DateBox className="qb-input w-[132px]" value={effectiveDate} min={date} onChange={(e) => setEffectiveDate(e.target.value)} />
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
          {creditTotal > 0 && <div className="text-xs text-qb-ink-muted">after {formatCents(creditTotal)} of discounts and credits</div>}
        </div>
      </div>
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <label>
          <span className="qb-label">Filter by</span>
          <select className="qb-input w-[200px]" value={filterVendor ?? ''} onChange={(e) => setFilterVendor(e.target.value === '' ? null : Number(e.target.value))}>
            <option value="">All vendors</option>
            {payBillsVendors(open).map((v) => (
              <option key={v.id} value={v.id}>
                {v.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="qb-label">Sort by</span>
          <select className="qb-input w-[140px]" value={sortBy} onChange={(e) => setSortBy(e.target.value)}>
            {PAY_BILLS_SORTS.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {hiddenPicked > 0 && (
          <span className="self-center text-xs text-qb-ink-muted">
            {hiddenPicked} ticked bill{hiddenPicked === 1 ? '' : 's'} from other vendors will be paid too
          </span>
        )}
      </div>
      <table className="qb-grid">
        <colgroup>
          <col className="w-[32px]" />
          <col />
          <col className="w-[100px]" />
          <col className="w-[100px]" />
          <col className="w-[150px]" />
          <col className="w-[120px]" />
          {(autoCredits || autoDiscounts) && !ach && <col className="w-[110px]" />}
          <col className="w-[130px]" />
        </colgroup>
        <thead>
          <tr>
            <th className="qb-grid-head" aria-label="Select" />
            <th className="qb-grid-head">Vendor</th>
            <th className="qb-grid-head">Bill no.</th>
            <th className="qb-grid-head">Due date</th>
            <th className="qb-grid-head" title="Early-payment discount the bill's terms offer, and the last day to take it">Discount offered</th>
            <th className="qb-grid-head text-right">Open balance</th>
            {(autoCredits || autoDiscounts) && !ach && <th className="qb-grid-head text-right" title="The early-payment discount taken and vendor credits used on this bill">Discount &amp; credits</th>}
            <th className="qb-grid-head text-right">Payment</th>
          </tr>
        </thead>
        <tbody>
          {open.length === 0 && (
            <tr>
              <td colSpan={(autoCredits || autoDiscounts) && !ach ? 8 : 7} className="qb-grid-cell text-qb-ink-muted">No open bills to pay.</td>
            </tr>
          )}
          {shown.map((b, i) => {
            const on = selected[b.id] !== undefined;
            return (
              <tr
                key={b.id}
                className={`qb-grid-row ${on ? 'is-active' : ''} ${highlight === b.id ? 'outline outline-1 -outline-offset-1 outline-qb-green' : ''}`}
                onClick={() => setHighlight(b.id)}
              >
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
                <td className="qb-grid-cell text-sm" title={b.termsName ?? undefined}>
                  {b.discountUntil && b.earlyDiscountCents > 0 && (date <= b.discountUntil ? `${formatCents(b.earlyDiscountCents)} by ${formatDate(b.discountUntil)}` : <span className="text-qb-ink-faint">ended {formatDate(b.discountUntil)}</span>)}
                </td>
                <td className="qb-grid-cell num">{formatCents(b.balanceCents)}</td>
                {(autoCredits || autoDiscounts) && !ach && <td className="qb-grid-cell num">{creditOn(b) > 0 ? formatCents(creditOn(b)) : ''}</td>}
                <td className="qb-grid-cell-edit">
                  <input
                    className="num"
                    value={selected[b.id] ?? ''}
                    onChange={(e) => {
                      setTyped((t) => new Set(t).add(b.id));
                      setSelected((s) => ({ ...s, [b.id]: e.target.value }));
                    }}
                    onFocus={() => !on && toggle(b, true)}
                    placeholder="0.00"
                    inputMode="decimal" data-money
                    aria-invalid={bad.includes(b)}
                    aria-label={`Payment to ${b.vendorName}`}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {highlightedBill && (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-qb-border bg-qb-canvas px-2 py-1 text-sm" aria-label="Bill information">
          <span className="font-semibold">
            {highlightedBill.vendorName}
            {highlightedBill.billNumber ? ` · Bill #${highlightedBill.billNumber}` : ''}
          </span>
          <span>Due {formatDate(highlightedBill.dueDate)}</span>
          {highlightedBill.termsName && <span>Terms {highlightedBill.termsName}</span>}
          <span>
            Discount{' '}
            {highlightedBill.discountUntil && highlightedBill.earlyDiscountCents > 0
              ? `${formatCents(highlightedBill.earlyDiscountCents)} until ${formatDate(highlightedBill.discountUntil)}`
              : 'none'}
          </span>
          <span>Discount &amp; credits used {formatCents(creditOn(highlightedBill))}</span>
          <button
            type="button"
            className="qb-btn qb-btn-secondary ml-auto h-row"
            title="Open this bill in a window of its own"
            onClick={() => openFromList('expenses/bills', 'edit', { billId: highlightedBill.id })}
          >
            Go to Bill
          </button>
        </div>
      )}
      {checks && payees.length > 1 && (
        <div className="px-2 py-1 text-xs text-qb-ink-muted">
          One check per vendor{lastCheck ? `, numbered ${firstCheck} to ${lastCheck}` : ''}. Each vendor's bills are paid together on one check.
        </div>
      )}
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
  // Duplicate: the saved bill the new one on the form was copied from ({ bill, lines, n }).
  const [copy, setCopy] = useState(null);
  const duplicate = (from) => {
    setPaying(false);
    setEditing(null);
    setCopy({ ...from, n: Date.now() });
    setNotice({ kind: 'info', text: `A copy of the bill from ${from.bill.vendorName}, dated today: enter its bill number and save it as a new bill.` });
  };
  // From a list row: the bill's lines are loaded first.
  const duplicateRow = (bill) =>
    api.getDocumentLines('bill', bill.id).then(
      (lines) => duplicate({ bill, lines }),
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const [achFiles, setAchFiles] = useState(false);
  const attachments = useAttachmentRows();
  const [printing, setPrinting] = useState([]);
  const company = useCompany();
  const vendorRef = useRef(null);
  // Every way in (Edit…, a double-click here, a register, a vendor's transactions) checks the same
  // rules the save would: a voided bill, or one with payments or credits, can't be replaced.
  const startEdit = (bill) => {
    if (bill.isVoided) return setNotice({ kind: 'info', text: `That bill from ${bill.vendorName} is voided, so it can't be edited.` });
    return api.getDocumentLines('bill', bill.id).then(
      (lines) => {
        setPaying(false);
        setCopy(null);
        setEditing({ bill, lines });
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  };
  const today = todayIso();
  // A bill chosen in the list opens in a window of its own, as forms do in QuickBooks.
  const openFromList = useOpenFromList();
  const openBill = (bill) => openFromList('expenses/bills', 'edit', { billId: bill.id });
  useEditIntent(intent, 'billId', data, startEdit, () => setNotice({ kind: 'error', text: 'That bill is no longer in the books.' }));
  useWindowContent(editing ? { name: `Bill ${editing.bill.billNumber || ''} · ${editing.bill.vendorName}`.replace('Bill  ·', 'Bill ·'), action: 'edit', data: { billId: editing.bill.id } } : null);

  useEffect(() => {
    if (intent?.action === 'pay-bills') setPaying(true);
    if (intent?.action === 'bill') {
      setPaying(false);
      vendorRef.current?.focus();
    }
  }, [intent]);

  const bills = data ?? [];
  // Previous / Next on the form step through the bills it can open (voided ones can't be edited).
  const savedBills = useMemo(() => byDateThenNumber(bills.filter((b) => !b.isVoided), (b) => b.billDate, (b) => b.billNumber), [bills]);
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
  }, { storageKey: 'bills' });

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
      {printing.length > 0 && (
        <PrintPortal>
          {printing.map((check, i) => (
            <CheckDocument key={i} company={company} check={check} bankName={accountLabel(accounts, check.paymentAccountId)} />
          ))}
        </PrintPortal>
      )}
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
          onDone={(text, kind = 'success', checks = []) => {
            setPaying(false);
            setNotice({ kind, text });
            if (checks.length > 0) {
              setPrinting(checks);
              // Let React render the checks into the print root before the dialog opens.
              setTimeout(() => printDocument(checks.length === 1 ? `Check ${checks[0].checkNumber ?? ''}`.trim() : 'Checks'), 80);
            }
          }}
        />
      ) : (
        <NewBill
          key={editing ? `edit-${editing.bill.id}` : copy ? `copy-${copy.n}` : 'new'}
          accounts={accounts}
          vendors={vendors.data ?? []}
          defaultCategory={defaultCategory}
          vendorRef={vendorRef}
          editing={editing}
          copyOf={copy}
          onDuplicate={duplicate}
          onCancelEdit={() => {
            setEditing(null);
            setCopy(null);
          }}
          onPosted={(text) => setNotice({ kind: 'success', text })}
          saved={savedBills}
          onOpenSaved={startEdit}
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
                  attachments.menuItem(b.journalEntryId),
                  {
                    label: 'Edit…',
                    disabled: b.isVoided,
                    hint: b.amountPaidCents > 0 ? 'Its payments and credits stay applied to the edited version' : undefined,
                    onSelect: () => openBill(b),
                  },
                  { label: 'Duplicate', hint: 'A new bill copied from this one, dated today', onSelect: () => duplicateRow(b) },
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
                    <tr
                      className={`qb-grid-row cursor-pointer ${b.isVoided ? 'text-qb-ink-faint' : ''}`}
                      // Double-click opens the bill to edit, as Edit… in its menu does.
                      onDoubleClick={() =>
                        b.isVoided
                          ? setNotice({ kind: 'info', text: `That bill from ${b.vendorName} is voided, so it can't be edited.` })
                          : openBill(b)
                      }
                      title="Double-click to edit"
                    >
                      <td className="qb-grid-cell font-semibold" title={b.vendorName}>{b.vendorName}</td>
                      <td className="qb-grid-cell">
                        {b.billNumber ?? ''}
                        {attachments.clip(b.journalEntryId)}
                      </td>
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
                    {attachments.row(b.journalEntryId, COLS)}
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
