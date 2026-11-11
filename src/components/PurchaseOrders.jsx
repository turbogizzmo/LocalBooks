import { useEffect, useMemo, useState } from 'react';
import { rowKeys } from '../lib/rowKeys.js';
import { newTransactionDate, useTransactionDate } from '../lib/myPreferences.js';
import NameField from './NameField.jsx';
import Notice from './Notice.jsx';
import FormToolbar, { fileButtons, navButtons } from './FormToolbar.jsx';
import { useDirtySince, useEditIntent, useOpenFromList } from '../lib/windows.js';
import SavedDocView, { useSavedDoc } from './SavedDocView.jsx';
import { byDateThenNumber } from '../lib/formStep.js';
import RowMenu from './RowMenu.jsx';
import SplitGrid, { blankSplit, splitRowsFromCopy, summarizeSplits } from './SplitGrid.jsx';
import PurchaseFormLines, { usePurchaseFormLines } from './PurchaseFormLines.jsx';
import { purchaseLineMessage } from '../lib/purchaseLines.js';
import { useFormClass } from './ClassSelect.jsx';
import { isCategory } from './Bills.jsx';
import { isHeldForApproval } from '../lib/approvals.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { addDays, formatDate, todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';
import DateBox from './DateBox.jsx';
import { SaveButtons, useFormFlow } from '../lib/formFlow.jsx';
import { FormInfoPanel } from './InfoPanel.jsx';
import { useFormPrinter } from './FormPages.jsx';

const STATUS = { OPEN: 'Open', CLOSED: 'Closed', BILLED: 'Billed' };

/**
 * `copyOf` (from Duplicate) starts the order as a copy of a saved one, dated today. With
 * `copyOf.editing` (from Edit…) it is the saved order itself, with its own dates, and saving
 * changes it in place under its number.
 */
function NewPurchaseOrder({ accounts, vendors, defaultCategory, onSaved, onCancel, savedStep, copyOf = null }) {
  const editing = copyOf?.editing ?? null; // { id, poNumber, poDate, expectedDate }
  const stockItems = useLedgerQuery(api.listInventoryItems);
  const [vendor, setVendor] = useState(copyOf?.name ?? '');
  const [poDate, setPoDate] = useTransactionDate();
  useEffect(() => {
    if (editing) setPoDate(editing.poDate);
    // Once, for the order being edited.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [expected, setExpected] = useState(editing?.expectedDate ?? '');
  const [memo, setMemo] = useState(copyOf?.memo ?? '');
  const [rows, setRows] = useState(() => (copyOf ? splitRowsFromCopy(copyOf.lines) : [blankSplit(defaultCategory), blankSplit(defaultCategory)]));
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const formClass = useFormClass(accounts);
  const openFromList = useOpenFromList();
  const { lines, invalid, total: splitTotal } = summarizeSplits(rows);
  const [formRows, setFormRows, laid, formTouched] = usePurchaseFormLines(lines, copyOf?.sourceId ? { doc: 'PURCHASE_ORDER', id: copyOf.sourceId } : null);
  const total = laid.error ? splitTotal : laid.total;

  const clear = () => {
    setVendor('');
    setPoDate(newTransactionDate());
    setExpected('');
    setMemo('');
    setRows([blankSplit(defaultCategory), blankSplit(defaultCategory)]);
    setFormRows([]);
    formClass.reset();
    setError(null);
  };

  const save = async (mode) => {
    if (!vendor.trim()) return setError('Enter a vendor.');
    if (invalid > 0) return setError(`${invalid} row(s) need an account and an amount above zero.`);
    if (lines.length === 0) return setError('Add at least one line.');
    if (laid.error && formRows.length > 0) return setError(purchaseLineMessage(laid.error));
    const posted = formClass.apply(lines).map(({ memo: description, ...l }) => ({ ...l, description }));
    setBusy(true);
    try {
      const input = { vendorName: vendor.trim(), poDate, expectedDate: expected || null, memo: memo.trim() || null, lines: posted, formLines: laid.formLines ?? [] };
      if (editing) {
        await api.updatePurchaseOrder(editing.id, input);
        onSaved(`PO #${editing.poNumber} for ${vendor.trim()} saved: ${formatCents(total)}.`, mode === 'print' ? editing.id : null);
        return true;
      }
      let id;
      try {
        id = await api.createPurchaseOrder(input);
      } catch (err) {
        // An approval rule holds this order: it waits under Expenses › Approvals.
        if (!isHeldForApproval(err)) throw err;
        await api.submitForApproval('PURCHASE_ORDER', input);
        onSaved(`Purchase order for ${vendor.trim()} sent for approval: ${formatCents(total)}. It isn't an open order until it is approved under Expenses › Approvals.`, null);
        clear();
        return true;
      }
      onSaved(`Purchase order for ${vendor.trim()} saved: ${formatCents(total)}. It does not post to the books until you turn it into a bill.`, mode === 'print' ? id : null);
      clear();
      return true;
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const flow = useFormFlow({ save: () => save('save'), clear, onClose: onCancel, busy });
  // Opening a saved order (Previous) asks for the entries to be saved or cleared first.
  const dirty = useDirtySince('purchase-order', { vendor, memo, rows: rows.map((r) => [r.memo, r.amount, r.qty, r.rate]), formTouched });

  return (
    <div className="flex flex-none gap-2">
    <section ref={flow.formRef} onKeyDown={flow.onKeyDown} className="qb-panel min-w-0 flex-1">
      <FormToolbar
        label="Purchase order"
        groups={[
          navButtons({ onFind: () => openFromList('accounting/find', 'find', { text: '' }), ...savedStep(busy || dirty) }),
          fileButtons({ onNew: clear, onSave: flow.saveAndNew, busy }),
          [{ id: 'print', label: 'Print', icon: 'Print', menu: [{ label: 'Save and print…', disabled: busy, onSelect: () => save('print') }] }],
        ]}
      />
      <div className="flex flex-wrap items-end gap-2 border-b border-qb-border px-2 py-1">
        <h2 className="mr-2 self-center text-md font-semibold">New purchase order</h2>
        <label>
          <span className="qb-label">Vendor</span>
          <NameField kind="vendor" names={vendors} value={vendor} onChange={setVendor} autoFocus />
        </label>
        <label>
          <span className="qb-label">PO date</span>
          <DateBox className="qb-input w-[132px]" value={poDate} onChange={(e) => setPoDate(e.target.value)} />
        </label>
        <label>
          <span className="qb-label">Expected</span>
          <DateBox className="qb-input w-[132px]" value={expected} onChange={(e) => setExpected(e.target.value)} />
        </label>
        {formClass.header}
        <div className="ml-auto self-center text-sm">
          <span className="text-qb-ink-muted">Total </span>
          <span className="num font-semibold">{formatCents(total)}</span>
        </div>
      </div>
      <SplitGrid rows={rows} setRows={setRows} accounts={accounts} filter={isCategory} defaultAccountId={defaultCategory} withPricing withClasses withJobs withBillable={false} stockItems={stockItems.data ?? []} />
      <PurchaseFormLines rows={formRows} setRows={setFormRows} laid={laid} lines={lines} accounts={accounts} />
      <div className="flex items-center gap-2 px-2 py-1">
        <input className="qb-input w-[320px]" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Message to vendor" />
        {error && <span className="text-sm text-qb-negative">{error}</span>}
        <div className="ml-auto flex gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onCancel} disabled={busy}>Close</button>
          <SaveButtons flow={flow} />
        </div>
      </div>
    </section>
    {/* The chosen vendor's information panel (T56); it can be hidden. */}
    <FormInfoPanel kind="vendor" name={vendor} names={vendors} />
    </div>
  );
}

function BillDialog({ order, onClose, onDone }) {
  const [billDate, setBillDate] = useState(todayIso);
  const [dueDate, setDueDate] = useState(() => addDays(todayIso(), 30));
  // The due date follows the vendor's terms (or the company's bill preference), as on Enter Bills,
  // until it is typed over.
  const [dueTyped, setDueTyped] = useState(false);
  useEffect(() => {
    if (dueTyped) return undefined;
    let live = true;
    api.getBillDueDate(order.vendorName, billDate).then((d) => live && setDueDate(d)).catch(() => {});
    return () => {
      live = false;
    };
  }, [order.vendorName, billDate, dueTyped]);
  const [number, setNumber] = useState('');
  const [error, setError] = useState(null);
  const create = async () => {
    try {
      await api.billPurchaseOrder(order.id, billDate, dueDate, number.trim() || null);
      onDone(`PO #${order.poNumber} turned into a bill from ${order.vendorName} for ${formatCents(order.totalCents)}; the bill is posted to Accounts Payable.`);
    } catch (err) {
      setError(
        isHeldForApproval(err)
          ? `${errorText(err)}. A bill from a purchase order can't be submitted from here yet: enter it in Enter Bills and choose Submit for approval, then close this purchase order once the bill is approved.`
          : errorText(err),
      );
    }
  };
  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/30" role="dialog" aria-modal="true" aria-label="Create bill">
      <div className="qb-panel w-[380px] p-3 shadow-panel">
        <h2 className="mb-2 text-md font-semibold">Create bill from PO #{order.poNumber}</h2>
        <div className="grid grid-cols-2 gap-2">
          <label>
            <span className="qb-label">Bill date</span>
            <DateBox className="qb-input w-full" value={billDate} onChange={(e) => setBillDate(e.target.value)} />
          </label>
          <label>
            <span className="qb-label">Due date</span>
            <DateBox className="qb-input w-full" value={dueDate} onChange={(e) => { setDueTyped(true); setDueDate(e.target.value); }} />
          </label>
          <label className="col-span-2">
            <span className="qb-label">Vendor’s bill no.</span>
            <input className="qb-input w-full" value={number} onChange={(e) => setNumber(e.target.value)} />
          </label>
        </div>
        {error && <div className="mt-2 text-sm text-qb-negative">{error}</div>}
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" className="qb-btn qb-btn-secondary" onClick={onClose}>Cancel</button>
          <button type="button" className="qb-btn qb-btn-primary" onClick={create} disabled={dueDate < billDate}>Create bill</button>
        </div>
      </div>
    </div>
  );
}

/** Expenses › Purchase Orders: non-posting orders to vendors, later turned into bills. */
export default function PurchaseOrders({ accounts, intent }) {
  const { data, error, reload } = useLedgerQuery(api.listPurchaseOrders);
  const vendors = useLedgerQuery(api.listVendors);
  const defaultCategory = useMemo(() => accounts.find((a) => a.isActive && isCategory(a) && a.classification === 'EXPENSE')?.id ?? null, [accounts]);
  const [composing, setComposing] = useState(intent?.action === 'purchase-order');
  const [notice, setNotice] = useState(null);
  const [billing, setBilling] = useState(null);
  // A purchase order saved with Save and print, printed once the list has it.
  const [printId, setPrintId] = useState(null);
  const [filter, setFilter] = useState('OPEN');
  const sorter = useSort((data ?? []).filter((o) => filter === 'ALL' || o.status === filter), {
    number: (o) => o.poNumber,
    date: (o) => o.poDate,
    vendor: (o) => o.vendorName,
    expected: (o) => o.expectedDate,
    memo: (o) => o.memo,
    status: (o) => STATUS[o.status] ?? o.status,
    total: (o) => o.totalCents,
  }, { storageKey: 'purchase-orders' });
  const list = sorter.sorted;

  // Printed as its Purchase Order form (Settings › Custom Forms), like the other forms.
  const [printPortal, forms] = useFormPrinter(setNotice);
  const print = (order) => forms.print('PURCHASE_ORDER', order.id);
  useEffect(() => {
    const order = printId !== null && (data ?? []).find((o) => o.id === printId);
    if (!order) return;
    setPrintId(null);
    print(order);
    // print is recreated each render; only a newly listed order should print.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [printId, data]);
  const setClosed = async (order, closed) => {
    try {
      await api.setPurchaseOrderClosed(order.id, closed);
      setNotice({ kind: 'success', text: `PO #${order.poNumber} ${closed ? 'closed' : 'reopened'}.` });
      reload();
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  const inventoryOn = useLedgerQuery(api.getPreferences).data?.inventory_enabled !== 'false';
  // Saved purchase orders open read-only in the form's place; Previous / Next step through them.
  const ordered = useMemo(() => byDateThenNumber(data ?? [], (o) => o.poDate, (o) => o.poNumber), [data]);
  const saved = useSavedDoc(ordered);
  const viewing = saved.current;
  const openFromList = useOpenFromList();
  const openOrder = (o) => {
    saved.open(o);
    setComposing(true);
  };
  // A purchase order opened from elsewhere (the bills money bar's Purchase orders list).
  useEditIntent(intent, 'purchaseOrderId', data, openOrder, () => setNotice({ kind: 'error', text: 'That purchase order is no longer in the books.' }));
  // Duplicate: the saved order the new one on the form starts from.
  const [copyOf, setCopyOf] = useState(null);
  const [formKey, setFormKey] = useState(0);
  const duplicate = (o) =>
    api.getCopySource('PURCHASE_ORDER', o.id).then(
      (copy) => {
        saved.close();
        // The copy's discounts and charges come from the original (`sourceId`).
        setCopyOf({ ...copy, sourceId: o.id });
        setFormKey((k) => k + 1);
        setComposing(true);
        setNotice({ kind: 'info', text: `A copy of PO #${o.poNumber}, dated today: it gets the next number when you save it.` });
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  // Edit…: the saved order on the form, saved in place under its number.
  const edit = (o) =>
    api.getCopySource('PURCHASE_ORDER', o.id).then(
      (copy) => {
        saved.close();
        setCopyOf({ ...copy, sourceId: o.id, editing: { id: o.id, poNumber: o.poNumber, poDate: o.poDate, expectedDate: o.expectedDate ?? '' } });
        setFormKey((k) => k + 1);
        setComposing(true);
      },
      (err) => setNotice({ kind: 'error', text: errorText(err) }),
    );
  const remove = async (o) => {
    if (!(await askConfirm(`Delete PO #${o.poNumber} for ${o.vendorName}? A purchase order posts nothing, so the books don't change.`, { confirmLabel: 'Delete', danger: true }))) return;
    try {
      await api.deletePurchaseOrder(o.id);
      saved.close();
      setComposing(false);
      setNotice({ kind: 'success', text: `PO #${o.poNumber} deleted.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  // Only an open order can be changed; one billed or with items received (even on a voided receipt) is refused by the books.
  const editable = (o) => inventoryOn && o.status === 'OPEN';
  return (
    <div className="flex h-full flex-col gap-2 p-2">
      {!inventoryOn && <Notice kind="info" text="Inventory and purchase orders are turned off in Preferences › Items & Inventory. Existing purchase orders stay here; new ones can't be made." />}
      {printPortal}
      {composing && viewing ? (
        <SavedDocView
          label="Purchase order"
          title={`PO #${viewing.poNumber} · ${viewing.vendorName} · ${STATUS[viewing.status] ?? viewing.status}`}
          step={saved.step}
          onFind={() => openFromList('accounting/find', 'find', { text: '' })}
          onNew={saved.close}
          onDuplicate={inventoryOn ? () => duplicate(viewing) : undefined}
          // Drawn, printed and emailed as its Purchase Order form (Settings › Custom Forms).
          form={{ kind: 'PURCHASE_ORDER', id: viewing.id }}
          record={{ kind: 'PURCHASE_ORDER', id: viewing.id }}
          onNotice={setNotice}
          extra={[
            [
              { id: 'edit-po', label: 'Edit', icon: 'NewForm', hint: editable(viewing) ? 'Change this purchase order' : 'Only an open purchase order can be changed', disabled: !editable(viewing), onClick: () => edit(viewing) },
              { id: 'delete-po', label: 'Delete', icon: 'Void', hint: viewing.status === 'BILLED' ? 'A billed purchase order stays' : 'Delete this purchase order', disabled: viewing.status === 'BILLED', onClick: () => remove(viewing) },
            ],
            [
              { id: 'bill', label: 'Create Bill', icon: 'HomeBill', hint: viewing.status === 'OPEN' ? 'Turn this purchase order into a bill' : 'Only an open purchase order can be billed', disabled: viewing.status !== 'OPEN', onClick: () => setBilling(viewing) },
              viewing.status === 'CLOSED'
                ? { id: 'reopen', label: 'Reopen', icon: 'ReopenDoc', hint: 'Reopen this purchase order', onClick: () => setClosed(viewing, false) }
                : { id: 'close-po', label: 'Close PO', icon: 'CloseDoc', hint: viewing.status === 'OPEN' ? 'Close it without billing' : 'Only an open purchase order can be closed', disabled: viewing.status !== 'OPEN', onClick: () => setClosed(viewing, true) },
            ],
          ]}
        >
        </SavedDocView>
      ) : composing ? (
        <NewPurchaseOrder
          key={formKey}
          copyOf={copyOf}
          savedStep={(blocked) => saved.fromNew(blocked)}
          accounts={accounts}
          vendors={vendors.data ?? []}
          defaultCategory={defaultCategory}
          onCancel={() => {
            setCopyOf(null);
            setComposing(false);
          }}
          onSaved={(text, toPrint) => {
            setCopyOf(null);
            setNotice({ kind: 'success', text });
            setPrintId(toPrint);
            reload();
          }}
        />
      ) : null}
      {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
      {error && <Notice kind="error" text={error} />}
      {billing && (
        <BillDialog
          order={billing}
          onClose={() => setBilling(null)}
          onDone={(text) => {
            setBilling(null);
            setNotice({ kind: 'success', text });
            reload();
          }}
        />
      )}
      <section className="qb-panel flex min-h-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center gap-2 border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Purchase orders</h2>
          <select className="qb-input h-row w-[120px]" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Show">
            <option value="OPEN">Open</option>
            <option value="BILLED">Billed</option>
            <option value="CLOSED">Closed</option>
            <option value="ALL">All</option>
          </select>
          {!composing && (
            <button type="button" className="qb-btn qb-btn-primary ml-auto h-row" onClick={() => setComposing(true)}>
              New purchase order
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <table className="qb-grid">
            <thead>
              <tr>
                <SortTh sorter={sorter} col="number">PO no.</SortTh>
                <SortTh sorter={sorter} col="date">Date</SortTh>
                <SortTh sorter={sorter} col="vendor">Vendor</SortTh>
                <SortTh sorter={sorter} col="expected">Expected</SortTh>
                <SortTh sorter={sorter} col="memo">Memo</SortTh>
                <SortTh sorter={sorter} col="status">Status</SortTh>
                <SortTh sorter={sorter} col="total" className="num">Total</SortTh>
                <th className="qb-grid-head w-[36px]" />
              </tr>
            </thead>
            <tbody>
              {data && list.length === 0 && (
                <tr>
                  <td colSpan={8} className="qb-grid-cell text-qb-ink-muted">No purchase orders to show.</td>
                </tr>
              )}
              {list.map((o) => (
                <tr key={o.id} className="qb-grid-row cursor-pointer" {...rowKeys({ first: o === list[0], open: () => openOrder(o) })}>
                  <td className="qb-grid-cell font-semibold">{o.poNumber}</td>
                  <td className="qb-grid-cell">{formatDate(o.poDate)}</td>
                  <td className="qb-grid-cell">{o.vendorName}</td>
                  <td className="qb-grid-cell">{o.expectedDate ? formatDate(o.expectedDate) : ''}</td>
                  <td className="qb-grid-cell text-qb-ink-muted">{o.memo ?? ''}</td>
                  <td className="qb-grid-cell">{STATUS[o.status] ?? o.status}</td>
                  <td className="qb-grid-cell num">{formatCents(o.totalCents)}</td>
                  <td className="qb-grid-cell">
                    <RowMenu
                      label={`Actions for PO ${o.poNumber}`}
                      items={[
                        { label: 'Open', onSelect: () => openOrder(o) },
                        { label: 'Edit…', disabled: !editable(o), onSelect: () => edit(o) },
                        ...(inventoryOn ? [{ label: 'Duplicate', hint: 'A new purchase order copied from this one, dated today', onSelect: () => duplicate(o) }] : []),
                        { label: 'Create bill…', disabled: o.status !== 'OPEN', onSelect: () => setBilling(o) },
                        { label: 'Print or save as PDF', onSelect: () => print(o) },
                        o.status === 'CLOSED'
                          ? { label: 'Reopen', onSelect: () => setClosed(o, false) }
                          : { label: 'Close without billing', disabled: o.status !== 'OPEN', onSelect: () => setClosed(o, true) },
                        { label: 'Delete…', disabled: o.status === 'BILLED', onSelect: () => remove(o) },
                      ]}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
