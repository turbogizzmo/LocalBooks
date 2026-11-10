import { Fragment, useEffect, useMemo, useState } from 'react';
import { rowKeys } from '../lib/rowKeys.js';
import { layoutMessage } from '../lib/itemTypes.js';
import { useTransactionDate } from '../lib/myPreferences.js';
import { attachPicked, PendingAttachments, useAttachmentRows } from './Attachments.jsx';
import FormToolbar, { attachLaterButton, fileButtons, navButtons } from './FormToolbar.jsx';
import SavedDocView, { useSavedDoc } from './SavedDocView.jsx';
import { byDateThenNumber } from '../lib/formStep.js';
import PaymentMethodTiles from './PaymentMethodTiles.jsx';
import AccountSelect from './AccountSelect.jsx';
import LineItemGrid, { blankLine, summarizeLines } from './LineItemGrid.jsx';
import { useFormClass } from './ClassSelect.jsx';
import { useSalesDetails } from './SalesDetails.jsx';
import { DiscountField, documentTotals, parseDiscount, useFormTax } from './SalesTax.jsx';
import MoneyBar from './MoneyBar.jsx';
import Notice from './Notice.jsx';
import { depositFilter, StatusBadge } from './Invoices.jsx';
import RowMenu, { voidItem } from './RowMenu.jsx';
import { EmailMarks, formMenuItems, useEmailStatuses, useFormPrinter } from './FormPages.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { usePaymentMethodChoice } from './ProfileFields.jsx';
import { useDirtySince, useOpenFromList } from '../lib/windows.js';
import { formatCents, formatMoney } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import NameField from './NameField.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { defaultDepositAccount, useSalesPrefs } from '../lib/salesPrefs.js';
import { FormInfoPanel } from './InfoPanel.jsx';

const KIND_LABEL = { INVOICE: 'Invoice', PAYMENT: 'Payment', SALES_RECEIPT: 'Sales Receipt' };

function SalesReceiptForm({ accounts, onPosted, onCancel, onNew, forms, savedStep }) {
  const products = useLedgerQuery(api.listProducts);
  const customers = useLedgerQuery(api.listCustomers);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && a.classification === 'INCOME')?.id ?? null, [accounts]);
  // Undeposited Funds, or the first bank account, as Preferences › Payments says.
  const { undepositedDefault, loaded: prefsLoaded } = useSalesPrefs();
  const defaultDeposit = useMemo(() => (prefsLoaded ? defaultDepositAccount(accounts, undepositedDefault) : null), [accounts, undepositedDefault, prefsLoaded]);
  const [customer, setCustomer] = useState('');
  const [date, setDate] = useTransactionDate();
  const method = usePaymentMethodChoice('CASH');
  const [reference, setReference] = useState('');
  const [depositId, setDepositId] = useState(defaultDeposit);
  const [memo, setMemo] = useState('');
  const [lines, setLines] = useState(() => Array.from({ length: 3 }, () => blankLine(defaultIncome)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  // Files picked before posting; attached once the receipt posts.
  const [files, setFiles] = useState([]);
  const openFromList = useOpenFromList();
  const formClass = useFormClass(accounts);
  // Opening a saved receipt (Previous) asks for the entries to be saved or cleared first.
  const dirty = useDirtySince('sales-receipt', { customer, reference, memo, lines: lines.map((l) => [l.productId, l.description, l.qty, l.rate]) });
  const tax = useFormTax(customer, customers.data);
  const sales = useSalesDetails(customer, customers.data, { billLabel: 'Sold To' });

  useEffect(() => {
    if (depositId === null && defaultDeposit !== null) setDepositId(defaultDeposit);
  }, [defaultDeposit, depositId]);
  useEffect(() => {
    if (defaultIncome === null) return;
    setLines((ls) => ls.map((l) => (l.accountId === null ? { ...l, accountId: defaultIncome } : l)));
  }, [defaultIncome]);

  const { filled, invalidCount, payload, formLines, total: linesTotal, layoutError } = summarizeLines(lines);
  const [discountText, setDiscountText] = useState('');
  const discountCents = parseDiscount(discountText, linesTotal);
  const { tax: taxCents, discount, total } = documentTotals(payload, tax.rate, discountCents ?? 0, formLines);

  const save = async (mode) => {
    if (filled.length === 0) return setError('Add at least one line.');
    if (invalidCount > 0) return setError(`${invalidCount} line(s) need an income account, a whole-number quantity and a rate above zero.`);
    if (depositId === null) return setError('Choose where the money was deposited.');
    if (layoutError) return setError(layoutMessage(layoutError));
    if (discountCents === null) return setError('Enter the discount as an amount or a percentage such as 10%.');
    const postedLines = formClass.apply(payload);
    const warning = formClass.missing(postedLines);
    if (warning) return setError(warning);
    setBusy(true);
    try {
      const receipt = await api.postSalesReceipt({
        customerName: customer.trim() || null,
        receiptDate: date,
        depositAccountId: depositId,
        ...method.payload(),
        reference: reference || null,
        memo: memo || null,
        lines: postedLines,
        taxRateId: tax.rateId,
        discountCents: discount,
        ...sales.payload(),
        formLines,
      });
      const attachError = files.length > 0 ? await attachPicked(receipt.journalEntryId, files) : null;
      if (mode === 'print') forms.print('SALES_RECEIPT', receipt.id);
      if (mode === 'email') forms.email('SALES_RECEIPT', receipt.id);
      onPosted(
        `Sales receipt #${receipt.receiptNumber} posted: Dr ${formatCents(receipt.totalCents)} to the deposit account / Cr income.` + (attachError ? ` ${attachError}` : ''),
      );
      // The screen starts a new blank receipt (onPosted).
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save: () => save('post'), clear: onNew, onClose: onCancel, busy });

  return (
    <div className="flex flex-none gap-2">
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel min-w-0 flex-1">
      <FormToolbar
        label="Sales receipt"
        groups={[
          navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...savedStep(busy || dirty) }),
          fileButtons({ onNew, onSave: flow.saveAndNew, busy }),
          [
            { id: 'print', label: 'Print', icon: 'Print', menu: [{ label: 'Post and print…', disabled: busy, onSelect: () => save('print') }] },
            { id: 'email', label: 'Email', icon: 'Email', menu: [{ label: 'Post and email…', disabled: busy, onSelect: () => save('email') }] },
            attachLaterButton({ setFiles, busy, what: 'this receipt' }),
          ],
        ]}
      />
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New sales receipt</h2>
        <label className="w-[220px]">
          <span className="qb-label">Customer (optional)</span>
          <NameField kind="customer" autoFocus className="qb-input w-full" names={customers.data ?? []} value={customer} onChange={setCustomer} placeholder="Walk-in" />
        </label>
        <label>
          <span className="qb-label">Sales receipt date</span>
          <DateBox className="qb-input w-[132px]" value={date} onChange={(e) => setDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Reference no.</span>
          <input className="qb-input w-[96px]" value={reference} onChange={(e) => setReference(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Deposit to</span>
          <AccountSelect accounts={accounts} filter={depositFilter} value={depositId} onChange={setDepositId} className="qb-input w-[190px]" />
        </label>
        {sales.picker}
        {formClass.header}
        {tax.picker}
        <DiscountField value={discountText} onChange={setDiscountText} invalid={discountCents === null} />
        <div>
          <span className="qb-label">Payment method</span>
          <PaymentMethodTiles value={method.id} onChange={method.set} />
        </div>
        <div className="ml-auto text-right">
          <div className="qb-label">Amount received</div>
          <div className="num text-3xl font-semibold leading-none">{formatCents(total)}</div>
          {(taxCents > 0 || discount > 0) && (
            <div className="text-xs text-qb-ink-muted">
              {discount > 0 && `less ${formatCents(discount)} discount`}
              {discount > 0 && taxCents > 0 && ' · '}
              {taxCents > 0 && `incl. ${formatCents(taxCents)} sales tax`}
            </div>
          )}
        </div>
      </div>
      {sales.addresses}
      <LineItemGrid lines={lines} setLines={setLines} accounts={accounts} products={products.data ?? []} defaultAccountId={defaultIncome} withTax={tax.enabled} customer={customer} customers={customers.data} itemTypes serviceDates />
      <div className="flex items-center gap-2 px-2 py-1">
        <button type="button" className="qb-btn qb-btn-secondary" onClick={() => setLines((ls) => [...ls, blankLine(defaultIncome)])}>
          Add line
        </button>
        <input className="qb-input w-[280px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Memo" />
        {files.length > 0 && <PendingAttachments paths={files} onChange={setFiles} label="Attach another…" />}
        <div className="ml-auto flex items-center gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>
            Close
          </button>
          <SaveButtons flow={flow} busyLabel="Posting…" />
        </div>
      </div>
      {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
    </section>
    {/* The chosen customer's information panel (T56); it can be hidden. */}
    <FormInfoPanel kind="customer" name={customer} names={customers.data} />
    </div>
  );
}

export default function AllSales({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listAllSales);
  const [filter, setFilter] = useState('ALL');
  const [composing, setComposing] = useState(intent?.action === 'sales-receipt');

  useEffect(() => {
    if (intent?.action === 'sales-receipt') setComposing(true);
  }, [intent]);
  const [formKey, setFormKey] = useState(0);
  const [notice, setNotice] = useState(null);
  const today = todayIso();
  const since = addDays(today, -30);

  const rows = data ?? [];
  const live = rows.filter((r) => !r.isVoided);
  const openInvoices = live.filter((r) => r.kind === 'INVOICE' && r.balanceCents > 0);
  const overdue = openInvoices.filter((r) => r.dueDate < today);
  const paidRecent = live.filter((r) => (r.kind === 'PAYMENT' || r.kind === 'SALES_RECEIPT') && r.date >= since);
  const [printPortal, forms] = useFormPrinter(setNotice);
  const emailed = useEmailStatuses('INVOICE');
  const receiptsEmailed = useEmailStatuses('SALES_RECEIPT');
  const attachments = useAttachmentRows();
  const voidRow = async (journalEntryId) => {
    await api.voidTransaction(journalEntryId);
    setNotice({ kind: 'success', text: 'Transaction voided: a reversing journal entry now offsets it.' });
  };
  // Documents can be in different currencies, so the money bar totals home-currency equivalents.
  const sum = (list, key) => list.reduce((s, r) => s + r[key], 0);
  const sorter = useSort(filter === 'ALL' ? rows : rows.filter((r) => r.kind === filter), {
    date: (r) => r.date,
    type: (r) => KIND_LABEL[r.kind],
    number: (r) => r.number,
    customer: (r) => r.customerName,
    due: (r) => r.dueDate,
    balance: (r) => (r.kind === 'INVOICE' ? r.balanceCents : null),
    total: (r) => r.homeTotalCents ?? r.totalCents,
    status: (r) => (r.isVoided ? 'VOIDED' : r.kind === 'INVOICE' ? r.status : r.kind === 'PAYMENT' ? 'Closed' : 'Paid'),
  }, { storageKey: 'all-sales' });
  const visible = sorter.sorted;
  // The first sales receipt shown: the list's Tab stop (only receipts open from the list).
  const firstReceipt = visible.find((x) => x.kind === 'SALES_RECEIPT');
  // Saved sales receipts open read-only in the form's place; Previous / Next step through them.
  const receipts = useMemo(() => byDateThenNumber(rows.filter((r) => r.kind === 'SALES_RECEIPT'), (r) => r.date, (r) => r.number), [rows]);
  const saved = useSavedDoc(receipts);
  const openFromList = useOpenFromList();
  const viewing = saved.current;
  const openReceipt = (r) => {
    saved.open(r);
    setComposing(true);
  };

  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {printPortal}
      <MoneyBar
        items={[
          { label: 'Open invoices', icon: '○', color: '#0077c5', cents: sum(openInvoices, 'homeBalanceCents'), count: openInvoices.length },
          { label: 'Overdue', icon: '⚠', color: '#d03b3b', cents: sum(overdue, 'homeBalanceCents'), count: overdue.length },
          { label: 'Paid last 30 days', icon: '✓', color: '#0ca30c', cents: sum(paidRecent, 'homeTotalCents'), count: paidRecent.length },
        ]}
      />

      {composing && viewing ? (
        <SavedDocView
          label="Sales receipt"
          title={`Sales receipt #${viewing.number}${viewing.customerName ? ` · ${viewing.customerName}` : ''}${viewing.isVoided ? ' (voided)' : ''}`}
          step={saved.step}
          onFind={() => openFromList('accounting/find', 'find', { text: '' })}
          onNew={saved.close}
          form={{ kind: 'SALES_RECEIPT', id: viewing.id }}
          entryId={viewing.journalEntryId}
          onNotice={setNotice}
          void={{
            onVoid: () => voidRow(viewing.journalEntryId),
            disabled: viewing.isVoided || !viewing.journalEntryId,
            hint: viewing.isVoided ? 'It is already voided' : undefined,
            confirm: {
              title: 'Void this transaction?',
              body: `Sales receipt #${viewing.number} (${formatMoney(viewing.totalCents, viewing.currencyCode)}). A reversing journal entry dated the same day will zero out its effect. The original stays in the books and the audit log. This cannot be undone.`,
              confirmLabel: 'Yes, void it',
            },
          }}
        />
      ) : composing ? (
        <SalesReceiptForm
          savedStep={(blocked) => saved.fromNew(blocked)}
          key={formKey}
          accounts={accounts}
          forms={forms}
          onNew={() => setFormKey((k) => k + 1)}
          onCancel={() => setComposing(false)}
          onPosted={(text) => {
            setNotice({ kind: 'success', text });
            setFormKey((k) => k + 1);
          }}
        />
      ) : null}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={`Could not load sales: ${error}`} />}

      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">All sales</h2>
          <select className="qb-input h-row w-[150px] text-sm" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Type">
            <option value="ALL">All transactions</option>
            <option value="INVOICE">Invoices</option>
            <option value="PAYMENT">Payments</option>
            <option value="SALES_RECEIPT">Sales receipts</option>
          </select>
          <span className="text-xs text-qb-ink-muted">{visible.length} transactions</span>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New sales receipt
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <colgroup>
              <col className="w-[92px]" />
              <col className="w-[110px]" />
              <col className="w-[136px]" />
              <col />
              <col className="w-[92px]" />
              <col className="w-[112px]" />
              <col className="w-[112px]" />
              <col className="w-[96px]" />
              <col className="w-[30px]" />
            </colgroup>
            <thead>
              <tr>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="type">Type</SortTh>
                <SortTh sorter={sorter} col="number">No.</SortTh>
                <SortTh sorter={sorter} col="customer">Customer</SortTh>
                <SortTh sorter={sorter} col="due">Due date</SortTh>
                <SortTh sorter={sorter} col="balance" className="text-right">Balance</SortTh>
                <SortTh sorter={sorter} col="total" className="text-right">Total</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <th className="qb-grid-head" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {data && visible.length === 0 && (
                <tr>
                  <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">No sales transactions yet.</td>
                </tr>
              )}
              {visible.map((r) => {
                const isOverdue = !r.isVoided && r.kind === 'INVOICE' && r.balanceCents > 0 && r.dueDate < today;
                const what = `${KIND_LABEL[r.kind]} #${r.number}${r.customerName ? ` for ${r.customerName}` : ''} (${formatMoney(r.totalCents, r.currencyCode)}).`;
                return (
                  <Fragment key={`${r.kind}-${r.id}`}>
                  <tr className={`qb-grid-row ${r.isVoided ? 'text-qb-ink-faint' : ''} ${r.kind === 'SALES_RECEIPT' ? 'cursor-pointer' : ''}`} {...(r.kind === 'SALES_RECEIPT' ? rowKeys({ first: r === firstReceipt, open: () => openReceipt(r) }) : {})}>
                    <td className="qb-grid-cell">{formatDate(r.date)}</td>
                    <td className="qb-grid-cell">{KIND_LABEL[r.kind]}</td>
                    <td className="qb-grid-cell">
                      {r.number}
                      {r.kind === 'INVOICE' && <EmailMarks status={emailed.get(r.id)} />}
                      {r.kind === 'SALES_RECEIPT' && <EmailMarks status={receiptsEmailed.get(r.id)} />}
                      {attachments.clip(r.journalEntryId)}
                    </td>
                    <td className="qb-grid-cell" title={r.customerName}>{r.customerName || <span className="text-qb-ink-faint">—</span>}</td>
                    <td className={`qb-grid-cell ${isOverdue ? 'font-semibold text-qb-negative' : ''}`}>{formatDate(r.dueDate)}</td>
                    <td className="qb-grid-cell num">{r.kind === 'INVOICE' ? formatMoney(r.balanceCents, r.currencyCode) : ''}</td>
                    <td className="qb-grid-cell num" title={r.currencyCode ? `${formatCents(r.homeTotalCents)} in home currency` : undefined}>
                      {formatMoney(r.totalCents, r.currencyCode)}
                    </td>
                    <td className="qb-grid-cell">
                      {r.kind === 'INVOICE' || r.isVoided ? (
                        <StatusBadge status={r.status} overdue={isOverdue} voided={r.isVoided} />
                      ) : (
                        <span className="text-qb-ink-muted">{r.kind === 'PAYMENT' ? 'Closed' : 'Paid'}</span>
                      )}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        items={[
                          ...(r.kind === 'INVOICE' ? formMenuItems('INVOICE', r.id, forms, emailed.get(r.id)) : []),
                          ...(r.kind === 'SALES_RECEIPT' ? [{ label: 'Open', onSelect: () => openReceipt(r) }, ...formMenuItems('SALES_RECEIPT', r.id, forms, receiptsEmailed.get(r.id))] : []),
                          attachments.menuItem(r.journalEntryId),
                          voidItem({ journalEntryId: r.journalEntryId, voided: r.isVoided, description: what, onVoid: voidRow }),
                        ]}
                      />
                    </td>
                  </tr>
                  {attachments.row(r.journalEntryId, 9)}
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
