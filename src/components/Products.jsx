import { useEffect, useMemo, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ChangePrices from './ChangePrices.jsx';
import PriceLevels from './PriceLevels.jsx';
import ItemGroups from './ItemGroups.jsx';
import Notice from './Notice.jsx';
import { AdjustDialog, Dialog, TrackDialog } from './InventoryDialog.jsx';
import RowMenu from './RowMenu.jsx';
import { CustomFieldInputs } from './SalesLists.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate, tableExports } from './ReportParts.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity, parseMoneyToCents } from '../lib/money.js';
import { quickReport, reportShortcut, useReportOpener } from '../lib/quickReport.js';
import { todayIso } from '../lib/dates.js';
import { parsePercentPpm, percentText } from '../lib/itemTypes.js';
import { useSalesPrefs } from '../lib/salesPrefs.js';
import { SortTh, useSort } from './SortHead.jsx';
import Attachments from './Attachments.jsx';
import { ListViewControls, TreeToggle, useListOrder } from './ListOrder.jsx';
import { parentByName } from '../lib/listOrder.js';

const incomeOnly = (a) => a.classification === 'INCOME';

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

function ProductDialog({ product, accounts, onClose, onSaved }) {
  const [name, setName] = useState(product.name);
  const [description, setDescription] = useState(product.description ?? '');
  const [itemType, setItemType] = useState(product.itemType || 'ITEM');
  const [rate, setRate] = useState(product.percentPpm ? percentText(product.percentPpm) : product.rateCents ? (product.rateCents / 100).toFixed(2) : '');
  const [accountId, setAccountId] = useState(product.incomeAccountId);
  const [taxable, setTaxable] = useState(product.isTaxable !== false);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [fields, setFields] = useState([]);
  const read = readRate(itemType, rate);
  useEffect(() => {
    api.getCustomValues('ITEM', product.id).then(setFields, () => setFields([]));
  }, [product.id]);

  const submit = async (e) => {
    e.preventDefault();
    if (read.error) return setError(read.error);
    setBusy(true);
    try {
      const input = { name, description: description.trim() || null, rateCents: read.rateCents, incomeAccountId: accountId, isTaxable: taxable };
      const saved = await api.updateProduct(product.id, input, read.kind);
      if (fields.length > 0) await api.saveCustomValues('ITEM', product.id, fields.map(({ fieldId, value }) => ({ fieldId, value })));
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
            <input className="qb-input" value={name} onChange={(e) => setName(e.target.value)} />
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
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description on sales forms</span>
            <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">{itemType === 'DISCOUNT' ? 'Discount account (debited)' : 'Income account'}</span>
            <AccountSelect accounts={accounts} filter={incomeOnly} value={accountId} onChange={setAccountId} />
          </label>
          <label className="col-span-2 flex items-center gap-1.5 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
            {itemType === 'DISCOUNT' ? 'Taxable: the discount lowers the taxable amount' : 'Taxable: charge sales tax when this is sold'}
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
  const stock = useLedgerQuery(api.listInventoryItems);
  const items = useMemo(() => new Map((stock.data ?? []).map((i) => [i.productId, i])), [stock.data]);
  // Preferences › Items & Inventory › inventory off: products already tracked keep their stock; no new ones.
  const inventoryOn = useLedgerQuery(api.getPreferences).data?.inventory_enabled !== 'false';
  const sorter = useSort(data, {
    name: (p) => p.name,
    description: (p) => p.description,
    rate: (p) => p.rateCents || null,
    income: (p) => accountLabel(accounts, p.incomeAccountId),
    onHand: (p) => items.get(p.id)?.qtyOnHandE4,
    avgCost: (p) => items.get(p.id)?.averageCostCents,
    value: (p) => items.get(p.id)?.valueCents,
  }, { storageKey: 'products' });
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
  // Without a sorted column, the custom order (drag a row) as a tree of sub-items ("Parent:Child") or flat.
  const parentOfItem = useMemo(() => {
    const byName = new Map((data ?? []).map((p) => [p.name.toLowerCase(), p.id]));
    return (p) => parentByName(p.name, byName);
  }, [data]);
  const customOrder = !sorter.sort;
  const listOrder = useListOrder('ITEMS', data, { parentOf: parentOfItem, dragEnabled: customOrder, onNotice: setNotice, defaultView: 'flat' });
  const shownItems = (customOrder ? listOrder.visible : sorter.sorted.map((row) => ({ row, id: row.id, depth: 0, hasChildren: false }))).filter((x) => showInactive || x.row.isActive);
  // Customers › Change Item Prices (menu bar) opens the price changer.
  useEffect(() => {
    if (intent?.action === 'change-prices') setChangingPrices(true);
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

  const company = useCompany();
  const exports = tableExports({
    company: company.name,
    title: 'Products & Services List',
    subtitle: `As of ${longDate(todayIso())}`,
    columns: [
      { label: 'Name', kind: 'text' },
      { label: 'Description', kind: 'text' },
      { label: 'Rate', kind: 'money' },
      { label: 'Income account', kind: 'text' },
      { label: 'Taxable', kind: 'text' },
      { label: 'On hand', kind: 'number' },
      { label: 'Avg cost', kind: 'money' },
      { label: 'Stock value', kind: 'money' },
    ],
    rows: () =>
      (data ?? [])
        .filter((p) => showInactive || p.isActive)
        .map((p) => {
          const item = items.get(p.id);
          return {
            cells: [
              `${p.name}${p.isActive ? '' : ' (inactive)'}`,
              p.description ?? '',
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
      setNotice({ kind: 'success', text: `Added ${product.name}.` });
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
    <div className="flex h-full flex-col gap-2 p-2 outline-none" tabIndex={-1} onKeyDown={onKeyDown}>
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
          accounts={accounts}
          onClose={() => setDialog(null)}
          onSaved={(text) => {
            setDialog(null);
            setNotice({ kind: 'success', text });
          }}
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
      {grouping && <ItemGroups products={data ?? []} onClose={() => setGrouping(false)} />}
      {changingPrices && <ChangePrices onClose={() => setChangingPrices(false)} />}
      {pricingLevels && !changingPrices && <PriceLevels onClose={() => setPricingLevels(false)} />}
      <section className={`qb-panel min-h-0 flex-1 flex-col ${changingPrices || pricingLevels ? 'hidden' : 'flex'}`}>
        <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Products & Services</h2>
          <span className="ml-2 min-w-0 truncate text-xs text-qb-ink-muted">Picking one on an invoice or sales receipt fills its description, rate and income account. Track stock to keep quantity and cost.</span>
          <span className="ml-auto" />
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
          <label className="flex flex-none items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive
          </label>
          {customOrder && <ListViewControls ctl={listOrder} label="items" />}
          <div className="ml-2 flex-none">
            <ExportMenu printable={false} onNotice={setNotice} {...exports} />
          </div>
        </div>
        {notice && <Notice kind={notice.kind} text={notice.text} onDismiss={() => setNotice(null)} />}
        {error && <Notice kind="error" text={error} />}
        <div className="min-h-0 flex-1 overflow-auto scrollbar-thin">
          <form onSubmit={add}>
            <table className="qb-grid">
              <colgroup>
                <col className="w-[220px]" />
                <col />
                <col className="w-[112px]" />
                <col className="w-[220px]" />
                <col className="w-[90px]" />
                <col className="w-[100px]" />
                <col className="w-[110px]" />
                <col className="w-[120px]" />
                <col className="w-[30px]" />
              </colgroup>
              <thead>
                <tr>
                  <SortTh sorter={sorter} col="name">Name</SortTh>
                  <SortTh sorter={sorter} col="description">Description</SortTh>
                  <SortTh sorter={sorter} col="rate" className="text-right">Rate</SortTh>
                  <SortTh sorter={sorter} col="income">Income account</SortTh>
                  <SortTh sorter={sorter} col="onHand" className="text-right">On hand</SortTh>
                  <SortTh sorter={sorter} col="avgCost" className="text-right">Avg cost</SortTh>
                  <SortTh sorter={sorter} col="value" className="text-right">Stock value</SortTh>
                  <th className="qb-grid-head" aria-label="Stock" />
                  <th className="qb-grid-head" aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                <tr className="bg-qb-row-selected">
                  <td className="qb-grid-cell-edit">
                    <input value={name} onChange={(e) => setName(e.target.value)} placeholder="New product or service" aria-label="Name" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Description on sales forms" aria-label="Description" />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <input className="num" value={rate} onChange={(e) => setRate(e.target.value)} placeholder={newType === 'ITEM' ? '0.00' : '0.00 or %'} inputMode="decimal" aria-label="Rate" aria-invalid={Boolean(read.error)} />
                  </td>
                  <td className="qb-grid-cell-edit">
                    <span className="flex items-center gap-1">
                      <AccountSelect className="min-w-0 flex-1" accounts={accounts} filter={incomeOnly} value={chosenAccount} onChange={setAccountId} aria-label="Income account" />
                      <label className="flex flex-none items-center gap-0.5 pr-1 text-xs" title="Charge sales tax when sold">
                        <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
                        Tax
                      </label>
                    </span>
                  </td>
                  <td className="qb-grid-cell-edit" colSpan={3}>
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
                  <td className="qb-grid-cell" />
                </tr>
                {data && data.length === 0 && (
                  <tr>
                    <td colSpan={9} className="qb-grid-cell text-qb-ink-muted">No products or services yet.</td>
                  </tr>
                )}
                {shownItems.map(({ row: p, ...item }) => (
                  <tr
                    key={p.id}
                    {...listOrder.rowProps(p.id)}
                    className={`qb-grid-row ${p.id === selected ? 'is-active' : ''} ${p.isActive ? '' : 'text-qb-ink-faint'}`}
                    aria-selected={p.id === selected}
                    onClick={() => setSelected(p.id)}
                    onDoubleClick={() => setDialog({ mode: 'edit', product: p })}
                  >
                    <td className="qb-grid-cell font-semibold" style={customOrder && listOrder.tree ? { paddingLeft: 8 + item.depth * 16 } : undefined}>
                      {customOrder && <TreeToggle ctl={listOrder} item={{ id: p.id, ...item }} />}
                      {customOrder && listOrder.tree ? p.name.slice(p.name.lastIndexOf(':') + 1) : p.name}
                      {typeLabel(p) && <span className="ml-1 text-2xs font-normal uppercase tracking-wide text-qb-ink-muted">{typeLabel(p)}</span>}
                      {!p.isActive && <span className="ml-1 text-2xs font-normal uppercase tracking-wide">Inactive</span>}
                    </td>
                    <td className="qb-grid-cell text-qb-ink-muted" title={p.description ?? ''}>{p.description ?? ''}</td>
                    <td className="qb-grid-cell num">{rateLabel(p)}</td>
                    <td className="qb-grid-cell">
                      {accountLabel(accounts, p.incomeAccountId)}
                      {p.isTaxable && <span className="ml-1 text-2xs font-semibold uppercase text-qb-ink-muted" title="Taxable">Tax</span>}
                    </td>
                    {items.has(p.id) ? (
                      <>
                        <td
                          className={`qb-grid-cell num ${items.get(p.id).qtyOnHandE4 < 0 ? 'font-semibold text-qb-negative' : ''}`}
                          title={items.get(p.id).qtyOnHandE4 < 0 ? 'Sold before it was bought: the cost is trued up when the bill is entered' : undefined}
                        >
                          {formatQuantity(items.get(p.id).qtyOnHandE4)}
                        </td>
                        <td className="qb-grid-cell num">{items.get(p.id).averageCostCents !== null ? formatCents(items.get(p.id).averageCostCents) : '—'}</td>
                        <td className="qb-grid-cell num">{formatCents(items.get(p.id).valueCents)}</td>
                        <td className="qb-grid-cell cell-tight">
                          <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setDialog({ mode: 'adjust', item: items.get(p.id) })}>
                            Adjust stock…
                          </button>
                        </td>
                      </>
                    ) : (
                      <>
                        <td className="qb-grid-cell text-qb-ink-faint" colSpan={3}>{(p.itemType ?? 'ITEM') === 'ITEM' ? 'Not tracked' : ''}</td>
                        <td className="qb-grid-cell cell-tight">
                          {inventoryOn && (p.itemType ?? 'ITEM') === 'ITEM' && (
                            <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setDialog({ mode: 'track', product: p })}>
                              Track stock…
                            </button>
                          )}
                        </td>
                      </>
                    )}
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${p.name}`}
                        items={[
                          { label: 'Edit…', onSelect: () => setDialog({ mode: 'edit', product: p }) },
                          { label: 'QuickReport (Ctrl+Q)', onSelect: () => runQuickReport(p) },
                          p.isActive ? { label: 'Make inactive', onSelect: () => setActive(p, false) } : { label: 'Make active', onSelect: () => setActive(p, true) },
                        ]}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </form>
        </div>
      </section>
    </div>
  );
}
