import { useEffect, useMemo, useState } from 'react';
import AccountSelect, { accountLabel } from './AccountSelect.jsx';
import ChangePrices from './ChangePrices.jsx';
import Notice from './Notice.jsx';
import { AdjustDialog, Dialog, TrackDialog } from './InventoryDialog.jsx';
import RowMenu from './RowMenu.jsx';
import { CustomFieldInputs } from './SalesLists.jsx';
import { useCompany } from './PrintDocuments.jsx';
import { ExportMenu, longDate, tableExports } from './ReportParts.jsx';
import { api, errorText, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity, parseMoneyToCents } from '../lib/money.js';
import { todayIso } from '../lib/dates.js';
import { SortTh, useSort } from './SortHead.jsx';

const incomeOnly = (a) => a.classification === 'INCOME';

function ProductDialog({ product, accounts, onClose, onSaved }) {
  const [name, setName] = useState(product.name);
  const [description, setDescription] = useState(product.description ?? '');
  const [rate, setRate] = useState(product.rateCents ? (product.rateCents / 100).toFixed(2) : '');
  const [accountId, setAccountId] = useState(product.incomeAccountId);
  const [taxable, setTaxable] = useState(product.isTaxable !== false);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [fields, setFields] = useState([]);
  const rateCents = rate.trim() === '' ? 0 : parseMoneyToCents(rate);
  useEffect(() => {
    api.getCustomValues('ITEM', product.id).then(setFields, () => setFields([]));
  }, [product.id]);

  const submit = async (e) => {
    e.preventDefault();
    if (rateCents === null || rateCents < 0) return setError('Enter a valid rate.');
    setBusy(true);
    try {
      const saved = await api.updateProduct(product.id, { name, description: description.trim() || null, rateCents, incomeAccountId: accountId, isTaxable: taxable });
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
            <span className="font-semibold text-qb-ink-muted">Rate</span>
            <input className="qb-input num" value={rate} onChange={(e) => setRate(e.target.value)} inputMode="decimal" aria-invalid={rateCents === null} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Description on sales forms</span>
            <input className="qb-input" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5">
            <span className="font-semibold text-qb-ink-muted">Income account</span>
            <AccountSelect accounts={accounts} filter={incomeOnly} value={accountId} onChange={setAccountId} />
          </label>
          <label className="col-span-2 flex items-center gap-1.5 text-sm">
            <input type="checkbox" className="accent-qb-green" checked={taxable} onChange={(e) => setTaxable(e.target.checked)} />
            Taxable: charge sales tax when this is sold
          </label>
          <CustomFieldInputs fields={fields} onChange={setFields} />
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
  const stock = useLedgerQuery(api.listInventoryItems);
  const items = useMemo(() => new Map((stock.data ?? []).map((i) => [i.productId, i])), [stock.data]);
  const sorter = useSort(data, {
    name: (p) => p.name,
    description: (p) => p.description,
    rate: (p) => p.rateCents || null,
    income: (p) => accountLabel(accounts, p.incomeAccountId),
    onHand: (p) => items.get(p.id)?.qtyOnHandE4,
    avgCost: (p) => items.get(p.id)?.averageCostCents,
    value: (p) => items.get(p.id)?.valueCents,
  });
  // { mode: 'track', product }, { mode: 'adjust', item } or { mode: 'edit', product }
  const [dialog, setDialog] = useState(null);
  const defaultIncome = useMemo(() => accounts.find((a) => a.isActive && incomeOnly(a))?.id ?? null, [accounts]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [rate, setRate] = useState('');
  const [accountId, setAccountId] = useState(null);
  const [taxable, setTaxable] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(null);
  const [showInactive, setShowInactive] = useState(false);
  const [changingPrices, setChangingPrices] = useState(false);
  // Customers › Change Item Prices (menu bar) opens the price changer.
  useEffect(() => {
    if (intent?.action === 'change-prices') setChangingPrices(true);
  }, [intent]);
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
  const rateCents = rate.trim() === '' ? 0 : parseMoneyToCents(rate);

  const add = async (e) => {
    e.preventDefault();
    if (!name.trim()) return setNotice({ kind: 'error', text: 'Enter a product or service name.' });
    if (rateCents === null || rateCents < 0) return setNotice({ kind: 'error', text: 'Enter a valid rate.' });
    if (chosenAccount === null) return setNotice({ kind: 'error', text: 'Choose an income account.' });
    setBusy(true);
    try {
      const product = await api.createProduct({
        name: name.trim(),
        description: description.trim() || null,
        rateCents,
        incomeAccountId: chosenAccount,
        isTaxable: taxable,
      });
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
    <div className="flex h-full flex-col gap-2 p-2">
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
      {changingPrices && <ChangePrices onClose={() => setChangingPrices(false)} />}
      <section className={`qb-panel min-h-0 flex-1 flex-col ${changingPrices ? 'hidden' : 'flex'}`}>
        <div className="flex h-row-lg flex-none items-center border-b border-qb-border px-2">
          <h2 className="text-md font-semibold">Products & Services</h2>
          <span className="ml-2 min-w-0 truncate text-xs text-qb-ink-muted">Picking one on an invoice or sales receipt fills its description, rate and income account. Track stock to keep quantity and cost.</span>
          <button type="button" className="qb-btn qb-btn-secondary ml-auto mr-2 h-row flex-none" onClick={() => setChangingPrices(true)}>
            Change prices…
          </button>
          <label className="flex flex-none items-center gap-1 text-sm">
            <input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} />
            Show inactive
          </label>
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
                    <input className="num" value={rate} onChange={(e) => setRate(e.target.value)} placeholder="0.00" inputMode="decimal" aria-label="Rate" aria-invalid={rateCents === null} />
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
                  <td className="qb-grid-cell" colSpan={3} />
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
                {sorter.sorted.filter((p) => showInactive || p.isActive).map((p) => (
                  <tr key={p.id} className={`qb-grid-row ${p.isActive ? '' : 'text-qb-ink-faint'}`} onDoubleClick={() => setDialog({ mode: 'edit', product: p })}>
                    <td className="qb-grid-cell font-semibold">
                      {p.name}
                      {!p.isActive && <span className="ml-1 text-2xs font-normal uppercase tracking-wide">Inactive</span>}
                    </td>
                    <td className="qb-grid-cell text-qb-ink-muted" title={p.description ?? ''}>{p.description ?? ''}</td>
                    <td className="qb-grid-cell num">{p.rateCents ? formatCents(p.rateCents) : ''}</td>
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
                        <td className="qb-grid-cell text-qb-ink-faint" colSpan={3}>Not tracked</td>
                        <td className="qb-grid-cell cell-tight">
                          <button type="button" className="qb-btn qb-btn-secondary h-[20px] w-full text-xs" onClick={() => setDialog({ mode: 'track', product: p })}>
                            Track stock…
                          </button>
                        </td>
                      </>
                    )}
                    <td className="qb-grid-cell cell-tight text-center">
                      <RowMenu
                        label={`Actions for ${p.name}`}
                        items={[
                          { label: 'Edit…', onSelect: () => setDialog({ mode: 'edit', product: p }) },
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
