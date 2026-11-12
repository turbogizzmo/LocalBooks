import { useEffect, useMemo, useRef, useState } from 'react';
import { rowKeys } from '../lib/rowKeys.js';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ChangePrices from './ChangePrices.jsx';
import PriceLevels from './PriceLevels.jsx';
import ItemGroups from './ItemGroups.jsx';
import Notice from './Notice.jsx';
import { AdjustDialog, Dialog, TrackDialog } from './InventoryDialog.jsx';
import RowMenu from './RowMenu.jsx';
import { CategorySelect, CategoryTree, MoveToCategoryDialog, useItemCategories } from './ItemCategories.jsx';
import { ItemInfoPanel, useInfoPanel } from './InfoPanel.jsx';
import { readReorderPoint } from '../lib/infoPanel.js';
import { CustomFieldInputs } from './SalesLists.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { longDate, tableExports } from './ReportParts.jsx';
import ListMenus from './ListMenus.jsx';
import { useListCommands } from '../lib/listCommands.js';
import { askConfirm } from '../lib/dialogs.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatQuantity, parseMoneyToCents } from '../lib/money.js';
import { quickReport, reportShortcut, useReportOpener } from '../lib/quickReport.js';
import { todayIso } from '../lib/dates.js';
import { parsePercentPpm, percentText } from '../lib/itemTypes.js';
import { useSalesPrefs } from '../lib/salesPrefs.js';
import { SortTh, useSort } from './SortHead.jsx';
import { useMergeOffer } from './MergeOffer.jsx';
import Attachments from './Attachments.jsx';
import ColumnsMenu, { useColumns } from './ColumnsMenu.jsx';
import { PASTE_COLUMNS, parsePastedItems } from '../lib/pasteItems.js';

/** Customize Columns: the item list's columns in their usual order. */
const ITEM_COLUMNS = [
  { id: 'name', label: 'Name', required: true },
  { id: 'description', label: 'Description' },
  { id: 'purchaseDescription', label: 'Purchase description', hidden: true },
  { id: 'rate', label: 'Rate' },
  { id: 'cost', label: 'Cost', hidden: true },
  { id: 'income', label: 'Income account' },
  { id: 'onHand', label: 'On hand' },
  { id: 'avgCost', label: 'Avg cost' },
  { id: 'value', label: 'Stock value' },
];
const ITEM_COLUMN_WIDTH = { name: 'w-[220px]', rate: 'w-[112px]', cost: 'w-[100px]', income: 'w-[220px]', onHand: 'w-[90px]', avgCost: 'w-[100px]', value: 'w-[110px]' };
const RIGHT_ITEM_COLUMNS = new Set(['rate', 'cost', 'onHand', 'avgCost', 'value']);
/** The search row's "in" box: which field the words are looked for in. */
const SEARCH_FIELDS = [
  ['all', 'All fields', (p) => [p.name, p.description, p.purchaseDescription]],
  ['name', 'Name', (p) => [p.name]],
  ['description', 'Description', (p) => [p.description]],
  ['purchaseDescription', 'Purchase description', (p) => [p.purchaseDescription]],
];
/** Add/Edit Multiple Items: paste rows from a spreadsheet; names already on the list are changed. */
function PasteItemsDialog({ onClose, onDone }) {
  const [text, setText] = useState('');
  const [problems, setProblems] = useState([]);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const rows = parsePastedItems(text);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.pasteItems(rows);
      if (result.problems.length > 0) {
        setProblems(result.problems);
        setBusy(false);
        return;
      }
      onDone(`Added ${result.added} and changed ${result.updated} item${result.added + result.updated === 1 ? '' : 's'}.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };
  return (
    <Dialog title="Add/Edit Multiple Items" onClose={onClose}>
      <div className="flex w-[720px] max-w-full flex-col gap-2 px-3 py-2 text-xs">
        <p className="text-qb-ink-muted">
          Copy rows from a spreadsheet with the columns {PASTE_COLUMNS.map(([, l]) => l).join(', ')}, in that order, and paste them below. A name already on the list changes that item (only the cells you
          fill in); a new name adds an item. Nothing is saved until every row reads.
        </p>
        <textarea
          className="qb-input h-[110px] py-1 font-mono"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setProblems([]);
          }}
          placeholder={'Bolt\tHex bolt\t0.60\tSales\tHex bolt, box of 100\t0.12'}
          aria-label="Pasted rows"
          autoFocus
        />
        {rows.length > 0 && (
          <div className="max-h-[200px] overflow-auto border border-qb-border scrollbar-thin">
            <table className="qb-grid">
              <thead>
                <tr>
                  <th className="qb-grid-head w-[36px]">Row</th>
                  {PASTE_COLUMNS.map(([id, label]) => (
                    <th key={id} className="qb-grid-head">{label}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i} className={problems.some((p) => p.row === i + 1) ? 'bg-qb-negative-tint' : ''}>
                    <td className="qb-grid-cell text-qb-ink-faint">{i + 1}</td>
                    {PASTE_COLUMNS.map(([id]) => (
                      <td key={id} className="qb-grid-cell">{r[id]}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {problems.length > 0 && (
          <ul className="text-qb-negative" role="alert">
            {problems.map((p) => (
              <li key={`${p.row}-${p.message}`}>Row {p.row}: {p.message}</li>
            ))}
          </ul>
        )}
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy || rows.length === 0} onClick={save}>
            Save {rows.length > 0 ? rows.length : ''} row{rows.length === 1 ? '' : 's'}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
import { ListViewControls, TreeToggle, useListOrder } from './ListOrder.jsx';
import { parentByName } from '../lib/listOrder.js';

const incomeOnly = (a) => a.classification === 'INCOME';
/** An Other Charge or Discount item's account on purchase forms: any income or expense account. */
const purchaseAccount = (a) => !a.isSystem && (a.classification === 'INCOME' || a.classification === 'EXPENSE');

/** The kinds of item, as the Type box offers them (see docs/plans/item-types.md). */
export const ITEM_TYPES = [
  { value: 'ITEM', label: 'Service or part' },
  { value: 'OTHER_CHARGE', label: 'Other charge' },
  { value: 'DISCOUNT', label: 'Discount' },
];

/**
 * The rate box read for an item type: a discount or an other charge takes an amount or a
 * percentage such as 10%. Returns { rateCents, kind } or { error }.
 */
export function readRate(itemType, text) {
  const t = text.trim();
  if (t.endsWith('%')) {
    if (itemType === 'ITEM') return { error: 'Only a discount or a charge can be a percentage.' };
    const percentPpm = parsePercentPpm(t);
    return percentPpm === null ? { error: 'Enter the percentage as 0.0001% to 100%.' } : { rateCents: 0, kind: { itemType, percentPpm } };
  }
  const rateCents = t === '' ? 0 : parseMoneyToCents(t);
  if (rateCents === null || rateCents < 0) return { error: 'Enter a valid rate.' };
  return { rateCents, kind: { itemType, percentPpm: null } };
}

/** "10%" or the amount, as the list shows an item's rate. */
const rateLabel = (p) => (p.percentPpm ? percentText(p.percentPpm) : p.rateCents ? formatCents(p.rateCents) : '');
const typeLabel = (p) => (p.itemType && p.itemType !== 'ITEM' ? ITEM_TYPES.find((t) => t.value === p.itemType)?.label : null);

function ProductDialog({ product, products = [], accounts, categories, onClose, onSaved }) {
  // Subitem of: a sub-item's name is "Parent:Item"; the box edits the last part.
  const split = product.name.lastIndexOf(':');
  const [parent, setParent] = useState(split > 0 ? product.name.slice(0, split) : '');
  const [name, setName] = useState(split > 0 ? product.name.slice(split + 1) : product.name);
  const fullName = parent ? `${parent}:${name.trim()}` : name;
  const parents = products.filter((p) => p.id !== product.id && p.name !== product.name && !p.name.startsWith(`${product.name}:`) && (p.isActive || p.name === parent));
  const [purchaseDescription, setPurchaseDescription] = useState(product.purchaseDescription ?? '');
  const [cost, setCost] = useState(product.purchaseCostCents == null ? '' : centsToInput(product.purchaseCostCents));
  const costCents = cost.trim() === '' ? null : parseMoneyToCents(cost);
  const [categoryId, setCategoryId] = useState(product.categoryId ?? null);
  const [description, setDescription] = useState(product.description ?? '');
  const [itemType, setItemType] = useState(product.itemType || 'ITEM');
  const [rate, setRate] = useState(product.percentPpm ? percentText(product.percentPpm) : product.rateCents ? (product.rateCents / 100).toFixed(2) : '');
  const [accountId, setAccountId] = useState(product.incomeAccountId);
  const [purchaseAccountId, setPurchaseAccountId] = useState(product.expenseAccountId ?? null);
  const [taxable, setTaxable] = useState(product.isTaxable !== false);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [fields, setFields] = useState([]);
  // Renaming to another item's name offers to merge into it.
  const mergeOffer = useMergeOffer('ITEM', product.id, product.name, (text) => onSaved(text));
  const read = readRate(itemType, rate);
  // Purchasing: the preferred vendor, and for a tracked item the reorder point.
  const tracked = product.qtyOnHandE4 != null;
  const vendors = useLedgerQuery(api.listVendors).data ?? [];
  const [vendorId, setVendorId] = useState(null);
  const [reorder, setReorder] = useState('');
  const [purchasing, setPurchasing] = useState(null);
  const reorderRead = readReorderPoint(reorder);
  useEffect(() => {
    api.getCustomValues('ITEM', product.id).then(setFields, () => setFields([]));
    api.getItemSummary(product.id).then(
      (s) => {
        setPurchasing(s);
        setVendorId(s.preferredVendorId ?? null);
        setReorder(s.reorderPointE4 === null || s.reorderPointE4 === undefined ? '' : formatQuantity(s.reorderPointE4));
      },
      () => setPurchasing(null),
    );
  }, [product.id]);

  const submit = async (e) => {
    e.preventDefault();
    if (read.error) return setError(read.error);
    if (tracked && reorderRead.error) return setError(reorderRead.error);
    if (!name.trim()) return setError('Enter the item’s name.');
    if (cost.trim() !== '' && (costCents === null || costCents < 0)) return setError('Enter the cost as an amount of zero or more, or leave it blank.');
    setBusy(true);
    try {
      if (await mergeOffer.check(fullName)) return setBusy(false);
      const input = { name: fullName, description: description.trim() || null, rateCents: read.rateCents, incomeAccountId: accountId, isTaxable: taxable };
      const saved = await api.updateProduct(product.id, input, { ...read.kind, expenseAccountId: itemType === 'ITEM' ? null : purchaseAccountId });
      if ((purchaseDescription.trim() || null) !== (product.purchaseDescription ?? null) || costCents !== (product.purchaseCostCents ?? null)) {
        await api.setItemPurchaseDetails(product.id, purchaseDescription.trim() || null, costCents);
      }
      if (categories && categoryId !== (product.categoryId ?? null)) await api.moveItemsToCategory([product.id], categoryId);
      if (fields.length > 0) await api.saveCustomValues('ITEM', product.id, fields.map(({ fieldId, value }) => ({ fieldId, value })));
      const reorderE4 = tracked ? reorderRead.e4 : null;
      if (purchasing && (reorderE4 !== (purchasing.reorderPointE4 ?? null) || vendorId !== (purchasing.preferredVendorId ?? null))) {
        await api.setItemPurchasing(product.id, reorderE4, vendorId);
      }
      onSaved(`Saved ${saved.name}. Transactions already posted keep their original details.`);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog title={`Edit ${product.name}`} onClose={onClose}>
      <form onSubmit={submit} className="flex flex-col gap-2 px-3 py-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Name</span>
            <input className="qb-input" value={name} onChange={(e) => setName(e.target.value.replace(/:/g, ''))} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Subitem of</span>
            <select className="qb-input" value={parent} onChange={(e) => setParent(e.target.value)} title="A sub-item is listed under its parent and named Parent:Item">
              <option value="">None (a top-level item)</option>
              {parents.map((p) => (
                <option key={p.id} value={p.name}>{p.name}</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{itemType === 'ITEM' ? 'Rate' : 'Amount or %'}</span>
            <input className="qb-input num" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" aria-invalid={Boolean(read.error)} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Type</span>
            <select className="qb-input" value={itemType} onChange={(e) => setItemType(e.target.value)} disabled={product.qtyOnHandE4 != null} title={product.qtyOnHandE4 != null ? 'An item tracked in stock stays a service or part' : undefined}>
              {ITEM_TYPES.map((t) => (
                <option key={t.value} value={t.value}>{t.label}</option>
              ))}
            </select>
          </label>
          {categories && (
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">Category</span>
              <CategorySelect categories={categories} value={categoryId} onChange={setCategoryId} />
            </label>
          )}
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description on sales forms</span>
            <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description on purchase forms</span>
            <input className="qb-input" value={purchaseDescription} maxLength={4000} onChange={(e) => setPurchaseDescription(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Cost</span>
            <input className="qb-input num" value={cost} onChange={(e) => setCost(e.target.value)} inputMode="decimal" placeholder="0.00" aria-invalid={cost.trim() !== '' && (costCents === null || costCents < 0)} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{itemType === 'DISCOUNT' ? 'Discount account (debited)' : 'Income account'}</span>
            <AccountSelect accounts={accounts} filter={incomeOnly} value={accountId} onChange={setAccountId} />
          </label>
          {itemType !== 'ITEM' && (
            <label className="col-span-2 flex flex-col gap-0.5">
              <span className="font-semibold text-qb-ink-muted">{itemType === 'DISCOUNT' ? 'Account on bills and checks (credited)' : 'Expense account on bills and checks'}</span>
              <AccountSelect accounts={accounts} filter={purchaseAccount} value={purchaseAccountId} onChange={setPurchaseAccountId} placeholder="Same as above" />
            </label>
          )}
          <label className="col-span-2 flex items-center gap-1.5 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
            {itemType === 'DISCOUNT' ? 'Taxable: the discount lowers the taxable amount' : 'Taxable: charge sales tax when this is sold'}
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Preferred vendor</span>
            <select className="qb-input" value={vendorId ?? ''} onChange={(e) => setVendorId(e.target.value === '' ? null : Number(e.target.value))} disabled={!purchasing}>
              <option value="">None</option>
              {vendors.filter((v) => v.isActive || v.id === vendorId).map((v) => (
                <option key={v.id} value={v.id}>{v.name}</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Reorder point</span>
            <input
              className="qb-input num"
              value={reorder}
              onChange={(e) => setReorder(e.target.value)}
              inputMode="decimal"
              disabled={!tracked || !purchasing}
              placeholder={tracked ? 'None' : 'Track stock first'}
              title={tracked ? 'Order more when the quantity on hand falls to this' : 'A reorder point compares with the quantity on hand, so track this item’s stock first'}
              aria-invalid={tracked && Boolean(reorderRead.error)}
            />
          </label>
          <CustomFieldInputs fields={fields} onChange={setFields} />
        </div>
        {/* Files kept with the item (a spec sheet, a photo): saved at once, not with Save. */}
        <div className="-mx-3 border-y border-qb-border bg-qb-canvas">
          <Attachments record={{ kind: 'ITEM', id: product.id }} what="this item" />
        </div>
        {error && <Notice kind="error" text={error} onDismiss={() => setError(null)} />}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>Cancel</button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={busy}>Save</button>
        </div>
        {mergeOffer.panel}
      </form>
    </Dialog>
  );
}

/** Adjust Quantity/Value on Hand from the menu bar: which tracked item to adjust. */
function PickAdjustDialog({ products, items, onClose, onPick }) {
  const names = new Map(products.map((p) => [p.id, p.name]));
  const tracked = items.filter((i) => names.has(i.productId)).sort((a, b) => names.get(a.productId).localeCompare(names.get(b.productId)));
  const [chosen, setChosen] = useState(tracked[0]?.id ?? null);
  return (
    <Dialog title="Adjust quantity or value on hand" onClose={onClose}>
      <form
        className="flex flex-col gap-2 px-3 py-2 text-xs"
        onSubmit={(e) => {
          e.preventDefault();
          const item = tracked.find((i) => i.id === chosen);
          if (item) onPick(item);
        }}
      >
        {tracked.length === 0 ? (
          <p className="text-qb-ink-muted">No items are tracked in stock yet. Choose Track stock… on an item first.</p>
        ) : (
          <label className="flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Item</span>
            <select className="qb-input" value={chosen ?? ''} onChange={(e) => setChosen(Number(e.target.value))} autoFocus>
              {tracked.map((i) => (
                <option key={i.id} value={i.id}>
                  {names.get(i.productId)} · {formatQuantity(i.qtyOnHandE4)} on hand
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="flex justify-end gap-2 border-t border-qb-border pt-2">
          <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="qb-btn qb-btn-primary h-row-lg px-3" disabled={tracked.length === 0}>
            Next
          </button>
        </div>
      </form>
    </Dialog>
  );
}

export default function Products({ accounts, intent }) {
  const { data, error } = useLedgerQuery(api.listProducts);
  // A click selects an item; Ctrl+Q opens its QuickReport (every line it is on).
  const [selected, setSelected] = useState(null);
  const openReport = useReportOpener();
  const runQuickReport = (p) => openReport(quickReport({ kind: 'item', id: p.id, name: p.name }));
  const onKeyDown = (e) => {
    const p = selected !== null && (data ?? []).find((x) => x.id === selected);
    if (reportShortcut(e) !== 'quickReport' || !p) return;
    e.preventDefault();
    runQuickReport(p);
  };
  // The information panel beside the list, for the item clicked last; it can be hidden.
  const info = useInfoPanel('item');
  const selectedProduct = selected === null ? null : (data ?? []).find((x) => x.id === selected) ?? null;
  const stock = useLedgerQuery(api.listInventoryItems);
  const items = useMemo(() => new Map((stock.data ?? []).map((i) => [i.productId, i])), [stock.data]);
  // Preferences › Items & Inventory › inventory off: products already tracked keep their stock; no new ones.
  const inventoryOn = useLedgerQuery(api.getPreferences).data?.inventory_enabled !== 'false';
  const sorter = useSort(data, {
    name: (p) => p.name,
    description: (p) => p.description,
    purchaseDescription: (p) => p.purchaseDescription,
    rate: (p) => p.rateCents || null,
    cost: (p) => p.purchaseCostCents,
    income: (p) => accountLabel(accounts, p.incomeAccountId),
    onHand: (p) => items.get(p.id)?.qtyOnHandE4,
    avgCost: (p) => items.get(p.id)?.averageCostCents,
    value: (p) => items.get(p.id)?.valueCents,
  }, { storageKey: 'products' });
  const cols = useColumns('items', ITEM_COLUMNS);
  // One cell of an item's row, by column.
  const itemCell = (c, p, item) => {
    const stock = items.get(p.id);
    switch (c.id) {
      case 'name':
        return (
          <td key={c.id} className="qb-grid-cell font-semibold" style={customOrder && listOrder.tree ? { paddingLeft: 8 + item.depth * 16 } : undefined}>
            <input
              type="checkbox"
              className="mr-1 align-middle accent-qb-green"
              checked={ticked.has(p.id)}
              onClick={(e) => e.stopPropagation()}
              onChange={() => tick(p.id)}
              aria-label={`Tick ${p.name}`}
            />
            {customOrder && <TreeToggle ctl={listOrder} item={{ id: p.id, ...item }} />}
            {customOrder && listOrder.tree ? p.name.slice(p.name.lastIndexOf(':') + 1) : p.name}
            {typeLabel(p) && <span className="ml-1 text-2xs font-normal uppercase tracking-wide text-qb-ink-muted">{typeLabel(p)}</span>}
            {!p.isActive && <span className="ml-1 text-2xs font-normal uppercase tracking-wide">Inactive</span>}
            {cats.enabled && p.categoryId != null && category === undefined && (
              <span className="ml-1 text-2xs font-normal text-qb-ink-muted" title="Category">{categoryName(p.categoryId)}</span>
            )}
          </td>
        );
      case 'description':
        return <td key={c.id} className="qb-grid-cell text-qb-ink-muted" title={p.description ?? ''}>{p.description ?? ''}</td>;
      case 'purchaseDescription':
        return <td key={c.id} className="qb-grid-cell text-qb-ink-muted" title={p.purchaseDescription ?? ''}>{p.purchaseDescription ?? ''}</td>;
      case 'cost':
        return <td key={c.id} className="qb-grid-cell num">{p.purchaseCostCents == null ? '' : formatCents(p.purchaseCostCents)}</td>;
      case 'rate':
        return <td key={c.id} className="qb-grid-cell num">{rateLabel(p)}</td>;
      case 'income':
        return (
          <td key={c.id} className="qb-grid-cell">
            {accountLabel(accounts, p.incomeAccountId)}
            {p.isTaxable && <span className="ml-1 text-2xs font-semibold uppercase text-qb-ink-muted" title="Taxable">Tax</span>}
          </td>
        );
      case 'onHand':
        return stock ? (
          <td key={c.id} className={`qb-grid-cell num ${stock.qtyOnHandE4 < 0 ? 'font-semibold text-qb-negative' : ''}`} title={stock.qtyOnHandE4 < 0 ? 'Sold before it was bought: the cost is trued up when the bill is entered' : undefined}>
            {formatQuantity(stock.qtyOnHandE4)}
          </td>
        ) : (
          <td key={c.id} className="qb-grid-cell text-qb-ink-faint">{(p.itemType ?? 'ITEM') === 'ITEM' ? 'Not tracked' : ''}</td>
        );
      case 'avgCost':
        return <td key={c.id} className="qb-grid-cell num">{stock ? (stock.averageCostCents !== null ? formatCents(stock.averageCostCents) : '—') : ''}</td>;
      default:
        return <td key={c.id} className="qb-grid-cell num">{stock ? formatCents(stock.valueCents) : ''}</td>;
    }
  };
  // { mode: 'track', product }, { mode: 'adjust', item } or { mode: 'edit', product }
  const [dialog, setDialog] = useState(null);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && incomeOnly(a))?.id ?? null, [accounts]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [rate, setRate] = useState('');
  const [newType, setNewType] = useState('ITEM');
  const [accountId, setAccountId] = useState(null);
  const [taxable, setTaxable] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const [changingPrices, setChangingPrices] = useState(false);
  const [pricingLevels, setPricingLevels] = useState(false);
  const [grouping, setGrouping] = useState(false);
  const { priceLevels: levelsOn, loaded } = useSalesPrefs();
  // Item categories: the tree on the left picks which items show (undefined = all, null = none).
  const cats = useItemCategories();
  const [category, setCategory] = useState(undefined);
  const inCategory = (p) => !cats.enabled || cats.index.covers(category, p.categoryId ?? null);
  const categoryCounts = useMemo(() => cats.index.counts((data ?? []).filter((p) => showInactive || p.isActive)), [cats.index, data, showInactive]);
  const categoryName = (id) => cats.categories.find((c) => c.id === id)?.fullName ?? '';
  // Without a sorted column, the custom order (drag a row) as a tree of sub-items ("Parent:Child") or flat.
  const parentOfItem = useMemo(() => {
    const byName = new Map((data ?? []).map((p) => [p.name.toLowerCase(), p.id]));
    return (p) => parentByName(p.name, byName);
  }, [data]);
  const customOrder = !sorter.sort;
  const listOrder = useListOrder('ITEMS', data, { parentOf: parentOfItem, dragEnabled: customOrder, onNotice: setNotice, defaultView: 'flat' });
  // The search row: words looked for in a field, applied by Search and cleared by Reset.
  const [lookFor, setLookFor] = useState({ text: '', field: 'all' });
  const [search, setSearch] = useState(null);
  const matches = (p) => {
    if (!search) return true;
    const values = SEARCH_FIELDS.find(([id]) => id === search.field)[2](p);
    const words = search.text.toLowerCase().split(/\s+/).filter(Boolean);
    return words.every((w) => values.some((v) => (v ?? '').toLowerCase().includes(w)));
  };
  const shownItems = (customOrder ? listOrder.visible : sorter.sorted.map((row) => ({ row, id: row.id, depth: 0, hasChildren: false }))).filter((x) => (showInactive || x.row.isActive) && matches(x.row) && inCategory(x.row));
  // Ticked items, for actions on several at once.
  const [ticked, setTicked] = useState(new Set());
  const tick = (id) =>
    setTicked((t) => {
      const next = new Set(t);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const [pasting, setPasting] = useState(false);
  const setTickedActive = async (active) => {
    try {
      const changed = await api.setItemsActive([...ticked], active);
      setTicked(new Set());
      setNotice({ kind: 'success', text: `${changed} item${changed === 1 ? '' : 's'} made ${active ? 'active' : 'inactive'}.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  // Customers › Change Item Prices (menu bar) opens the price changer.
  useEffect(() => {
    if (intent?.action === 'change-prices') setChangingPrices(true);
    // Vendors › Inventory Activities › Adjust Quantity/Value on Hand: pick the item first.
    if (intent?.action === 'adjust-stock') setDialog({ mode: 'pick-adjust' });
    if (intent?.action === 'price-levels') setPricingLevels(true);
  }, [intent]);
  useEffect(() => {
    if (!loaded || levelsOn || !pricingLevels) return;
    setPricingLevels(false);
    setNotice({ kind: 'info', text: 'Price levels are turned off. Turn them on in Preferences › Sales & Customers.' });
  }, [loaded, levelsOn, pricingLevels]);
  const setActive = async (p, active) => {
    try {
      await api.setProductActive(p.id, active);
      setNotice({ kind: 'success', text: active ? `${p.name} is active again.` : `${p.name} is inactive: hidden from sales forms, with its history kept.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };

  // Edit › New, Edit, Delete, Make Inactive and Show All act on this list and its selected item.
  const listRef = useRef(null);
  const newNameRef = useRef(null);
  const chosen = () => (data ?? []).find((p) => p.id === selected) ?? null;
  const needChoice = () => setNotice({ kind: 'info', text: 'Click an item in the list first.' });
  const removeItem = async (p) => {
    if (!(await askConfirm(`Delete ${p.name}? Only an item on no transactions, groups or stock records can be deleted; this can't be undone.`, { confirmLabel: 'Delete', danger: true }))) return;
    try {
      await api.deleteListRecord('item', p.id);
      setSelected(null);
      setNotice({ kind: 'success', text: `Deleted ${p.name}.` });
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    }
  };
  useListCommands(listRef, {
    new: () => newNameRef.current?.focus(),
    edit: () => (chosen() ? setDialog({ mode: 'edit', product: chosen() }) : needChoice()),
    delete: () => (chosen() ? removeItem(chosen()) : needChoice()),
    'toggle-active': () => (chosen() ? setActive(chosen(), !chosen().isActive) : needChoice()),
    'show-all': () => setShowInactive((on) => !on),
  });

  const company = useCompany();
  const exports = tableExports({
    company: company.name,
    title: 'Products & Services List',
    subtitle: `As of ${longDate(todayIso())}`,
    columns: [
      { label: 'Name', kind: 'text' },
      { label: 'Description', kind: 'text' },
      ...(cats.enabled ? [{ label: 'Category', kind: 'text' }] : []),
      { label: 'Rate', kind: 'money' },
      { label: 'Income account', kind: 'text' },
      { label: 'Taxable', kind: 'text' },
      { label: 'On hand', kind: 'number' },
      { label: 'Avg cost', kind: 'money' },
      { label: 'Stock value', kind: 'money' },
    ],
    rows: () =>
      (data ?? [])
        .filter((p) => (showInactive || p.isActive) && inCategory(p))
        .map((p) => {
          const item = items.get(p.id);
          return {
            cells: [
              `${p.name}${p.isActive ? '' : ' (inactive)'}`,
              p.description ?? '',
              ...(cats.enabled ? [p.categoryId != null ? categoryName(p.categoryId) : 'Uncategorized'] : []),
              p.rateCents || null,
              accountLabel(accounts, p.incomeAccountId),
              p.isTaxable ? 'Yes' : 'No',
              item ? item.qtyOnHandE4 / 10_000 : null,
              item?.averageCostCents ?? null,
              item ? item.valueCents : null,
            ],
          };
        }),
    filename: 'Products-and-Services',
    onNotice: setNotice,
  });

  const chosenAccount = accountId ?? defaultIncome;
  const read = readRate(newType, rate);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a product or service name.' });
    if (read.error) return setNotice({ kind: 'error', text: read.error });
    if (chosenAccount === null) return setNotice({ kind: 'error', text: 'Choose an income account.' });
    setBusy(true);
    try {
      const product = await api.createProduct(
        {
          name: name.trim(),
          description: description.trim() || null,
          rateCents: read.rateCents,
          incomeAccountId: chosenAccount,
          isTaxable: taxable,
        },
        read.kind,
      );
      // Added while a category is chosen in the tree: the item goes in it.
      const into = cats.enabled && category != null ? category : null;
      if (into !== null) await api.moveItemsToCategory([product.id], into);
      setNotice({ kind: 'success', text: `Added ${product.name}${into !== null ? ` to ${categoryName(into)}` : ''}.` });
      setName('');
      setDescription('');
      setRate('');
    } catch (err) {
      setNotice({ kind: 'error', text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div ref={listRef} className="flex h-full flex-col gap-2 p-2 outline-hidden" tabIndex={-1} onKeyDown={onKeyDown}>
      {dialog?.mode === 'track' && (
        <TrackDialog
          product={dialog.product}
          onClose={() => setDialog(null)}
          onDone={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {dialog?.mode === 'edit' && (
        <ProductDialog
          product={dialog.product}
          products={data ?? []}
          accounts={accounts}
          categories={cats.enabled ? cats.categories : null}
          onClose={() => setDialog(null)}
          onSaved={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {dialog?.mode === 'pick-adjust' && (
        <PickAdjustDialog
          products={data ?? []}
          items={stock.data ?? []}
          onClose={() => setDialog(null)}
          onPick={(item) => setDialog({ mode: 'adjust', item })}
        />
      )}
      {dialog?.mode === 'adjust' && (
        <AdjustDialog
          item={dialog.item}
          accounts={accounts}
          onClose={() => setDialog(null)}
          onDone={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {pasting && (
        <PasteItemsDialog
          onClose={() => setPasting(false)}
          onDone={(text) => {
            setPasting(false);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {dialog?.mode === 'move' && (
        <MoveToCategoryDialog
          items={[dialog.product]}
          categories={cats.categories}
          onClose={() => setDialog(null)}
          onDone={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
        />
      )}
      {grouping && <ItemGroups products={data ?? []} onClose={() => setGrouping(false)} />}
      {changingPrices && <ChangePrices onClose={() => setChangingPrices(false)} />}
      {pricingLevels && !changingPrices && <PriceLevels onClose={() => setPricingLevels(false)} />}
      <div className={`min-h-0 flex-1 gap-2 ${changingPrices || pricingLevels ? 'hidden' : 'flex'}`}>
      <section className="qb-panel flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Products & Services</h2>
          <span className="ml-2 min-w-0 truncate text-xs text-qb-ink-muted">Picking one on an invoice or sales receipt fills its description, rate and income account. Track stock to keep quantity and cost.</span>
          <span className="ml-auto" />
          <button type="button" className="qb-btn qb-btn-secondary mr-2 h-row flex-none" onClick={() => setPasting(true)} title="Paste rows from a spreadsheet to add or change many items">
            Add/Edit Multiple…
          </button>
          <button type="button" className="qb-btn qb-btn-secondary mr-2 h-row flex-none" onClick={() => setGrouping(true)}>
            Groups…
          </button>
          {levelsOn && (
            <button type="button" className="qb-btn qb-btn-secondary mr-2 h-row flex-none" onClick={() => setPricingLevels(true)}>
              Price levels…
            </button>
          )}
          <button type="button" className="qb-btn qb-btn-secondary mr-2 h-row flex-none" onClick={() => setChangingPrices(true)}>
            Change prices…
          </button>
          <span className="mr-2 flex-none">{info.button}</span>
          <label className="flex flex-none items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive
          </label>
          {customOrder && <ListViewControls ctl={listOrder} label="items" />}
          <div className="ml-2 flex-none">
            <ListMenus kind="item" exports={exports} />
          </div>
          <ColumnsMenu cols={cols} />
        </div>
        <form
          className="flex flex-none flex-wrap items-center gap-2 border-b border-qb-border px-2 py-1 text-sm"
          onSubmit={(e) => {
            e.preventDefault();
            setSearch(lookFor.text.trim() ? { ...lookFor } : null);
          }}
          aria-label="Search the items"
        >
          <label className="flex items-center gap-1">
            Look for
            <input className="qb-input h-row w-[200px]" value={lookFor.text} onChange={(e) => setLookFor((l) => ({ ...l, text: e.target.value }))} aria-label="Look for" />
          </label>
          <label className="flex items-center gap-1">
            in
            <select className="qb-input h-row" value={lookFor.field} onChange={(e) => setLookFor((l) => ({ ...l, field: e.target.value }))} aria-label="In field">
              {SEARCH_FIELDS.map(([id, label]) => (
                <option key={id} value={id}>{label}</option>
              ))}
            </select>
          </label>
          <button type="submit" className="qb-btn qb-btn-secondary h-row px-2">
            Search
          </button>
          <button
            type="button"
            className="qb-btn qb-btn-secondary h-row px-2"
            onClick={() => {
              setLookFor({ text: '', field: 'all' });
              setSearch(null);
            }}
          >
            Reset
          </button>
          {search && <span className="text-xs text-qb-ink-muted">{shownItems.length} found</span>}
          {ticked.size > 0 && (
            <span className="ml-auto flex items-center gap-2 text-xs">
              <strong>{ticked.size} ticked</strong>
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => setTickedActive(false)}>
                Make inactive
              </button>
              <button type="button" className="qb-btn qb-btn-secondary h-row px-2" onClick={() => setTickedActive(true)}>
                Make active
              </button>
              <button type="button" className="text-qb-link hover:underline" onClick={() => setTicked(new Set())}>
                Clear
              </button>
            </span>
          )}
        </form>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="flex min-h-0 flex-1">
          {cats.enabled && <CategoryTree categories={cats.categories} selected={category} onSelect={setCategory} counts={categoryCounts} onNotice={setNotice} />}
          <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <form onSubmit={add}>
            <table className="qb-grid">
              <colgroup>
                {cols.columns.map((c) => (
                  <col key={c.id} className={ITEM_COLUMN_WIDTH[c.id]} />
                ))}
                <col className="w-[120px]" />
                <col className="w-[30px]" />
              </colgroup>
              <thead>
                <tr>
                  {cols.columns.map((c) => (
                    <SortTh key={c.id} sorter={sorter} col={c.id} className={RIGHT_ITEM_COLUMNS.has(c.id) ? 'text-right' : ''}>
                      {c.label}
                    </SortTh>
                  ))}
                  <th className="qb-grid-head" aria-label="Stock" />
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  {cols.columns.map((c) => {
                    if (c.id === 'name')
                      return (
                        <td key={c.id} className="qb-grid-cell-edit">
                          <input ref={newNameRef} value={name} onChange={(e) => setName(e.target.value)} placeholder="New product or service" aria-label="Name" />
                        </td>
                      );
                    if (c.id === 'description')
                      return (
                        <td key={c.id} className="qb-grid-cell-edit">
                          <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Description on sales forms" aria-label="Description" />
                        </td>
                      );
                    if (c.id === 'rate')
                      return (
                        <td key={c.id} className="qb-grid-cell-edit">
                          <input className="num" value={rate} onChange={(e) => setRate(e.target.value)} placeholder={newType === 'ITEM' ? '0.00' : '0.00 or %'} inputMode="decimal" aria-label="Rate" aria-invalid={Boolean(read.error)} />
                        </td>
                      );
                    if (c.id === 'income')
                      return (
                        <td key={c.id} className="qb-grid-cell-edit">
                          <span className="flex items-center gap-1">
                            <AccountSelect className="min-w-0 flex-1" accounts={accounts} filter={incomeOnly} value={chosenAccount} onChange={setAccountId} aria-label="Income account" />
                            <label className="flex flex-none items-center gap-0.5 pr-1 text-xs" title="Charge sales tax when sold">
                              <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
                              Tax
                            </label>
                          </span>
                        </td>
                      );
                    return <td key={c.id} className="qb-grid-cell" />;
                  })}
                  <td className="qb-grid-cell-edit">
                    <select value={newType} onChange={(e) => setNewType(e.target.value)} aria-label="Type">
                      {ITEM_TYPES.map((t) => (
                        <option key={t.value} value={t.value}>{t.label}</option>
                      ))}
                    </select>
                  </td>
                  <td className="qb-grid-cell cell-tight">
                    <button type="submit" className="qb-btn qb-btn-primary h-[20px] w-full" disabled={busy}>
                      Add
                    </button>
                  </td>
                </tr>
                {data && data.length === 0 && (
                  <tr>
                    <td colSpan={cols.columns.length + 2} className="qb-grid-cell text-qb-ink-muted">No products or services yet.</td>
                  </tr>
                )}
                {shownItems.map(({ row: p, ...item }) => (
                  <tr
                    key={p.id}
                    {...listOrder.rowProps(p.id)}
                    className={`qb-grid-row ${p.id === selected ? 'is-active' : ''} ${p.isActive ? '' : 'text-qb-ink-faint'}`}
                    aria-selected={p.id === selected}
                    onClick={() => setSelected(p.id)}
                    {...rowKeys({ first: p === shownItems[0]?.row, open: () => setDialog({ mode: 'edit', product: p }), select: () => setSelected(p.id) })}
                  >
                    {cols.columns.map((c) => itemCell(c, p, item))}
                    <td className="qb-grid-cell cell-tight">
                      {items.has(p.id) ? (
                        <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setDialog({ mode: 'adjust', item: items.get(p.id) })}>
                          Adjust stock…
                        </button>
                      ) : (
                        inventoryOn && (p.itemType ?? 'ITEM') === 'ITEM' && (
                          <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setDialog({ mode: 'track', product: p })}>
                            Track stock…
                          </button>
                        )
                      )}
                    </td>
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${p.name}`}
                        items={[
                          { label: 'Edit…', onSelect: () => setDialog({ mode: 'edit', product: p }) },
                          { label: 'QuickReport (Ctrl+Q)', onSelect: () => runQuickReport(p) },
                          ...(cats.enabled ? [{ label: 'Move to a different category…', onSelect: () => setDialog({ mode: 'move', product: p }) }] : []),
                          p.isActive ? { label: 'Make inactive', onSelect: () => setActive(p, false) } : { label: 'Make active', onSelect: () => setActive(p, true) },
                          { label: 'Delete…', hint: 'Only one on no transactions, groups or stock records', onSelect: () => removeItem(p) },
                        ]}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </form>
          </div>
        </div>
      </section>
      {info.shown && selectedProduct && <ItemInfoPanel product={selectedProduct} onHide={info.hide} />}
      </div>
    </div>
  );
}
