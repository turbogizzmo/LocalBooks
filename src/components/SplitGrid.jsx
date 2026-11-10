import { useRef } from 'react';
import { useLineCommands } from '../lib/formFlow.jsx';
import { pasteLineAt } from '../lib/formKeys.js';
import AccountSelect from './AccountSelect.jsx';
import ClassSelect, { showsClassColumn, useClassTracking } from './ClassSelect.jsx';
import ColumnsMenu, { useColumns } from './ColumnsMenu.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { centsToInput, formatCents, formatMoney, formatQuantity, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';

let nextKey = 1;
// The same empty list every time, so a grid without Received From never re-renders for it.
const NO_VENDORS = [];
const noVendors = async () => NO_VENDORS;
// `billable` is null until the row's Billable box is touched: the company default then decides
// (Preferences › Time & Expenses › "Mark all expenses as billable").
export const blankSplit = (accountId = null) => ({ key: nextKey++, accountId, productId: null, classId: null, customerId: null, vendorId: null, billable: null, memo: '', amount: '', qty: '', rate: '' });

/** Split rows for a posted bill's or expense's lines, to edit it. */
export const splitRowsFrom = (lines) =>
  lines.map((l) => ({
    ...blankSplit(l.accountId),
    productId: l.productServiceId ?? null,
    classId: l.classId ?? null,
    customerId: l.customerId ?? null,
    billable: l.customerId ? Boolean(l.billable) : null,
    memo: l.memo ?? '',
    amount: l.quantityE4 ? '' : centsToInput(l.amountCents),
    qty: l.quantityE4 ? formatQuantity(l.quantityE4).replace(/,/g, '') : '',
    rate: l.quantityE4 && l.rateCents ? centsToInput(l.rateCents) : '',
  }));

/**
 * A row's pricing: with a quantity (up to 4 decimals) and a rate, the amount is
 * round_half_up(quantity × rate), exactly as the ledger computes it; otherwise the typed amount.
 */
export function evaluateSplit(row) {
  const qtyText = (row.qty ?? '').trim();
  const rateText = (row.rate ?? '').trim();
  if (qtyText === '' && rateText === '') {
    return { priced: false, quantityE4: null, rateCents: null, amount: parseMoneyToCents(row.amount) };
  }
  const quantityE4 = parseQuantityE4(qtyText);
  const rateCents = parseMoneyToCents(rateText);
  const amount = quantityE4 !== null && rateCents !== null && rateCents > 0 ? quantityAmount(quantityE4, rateCents) : null;
  return { priced: true, quantityE4, rateCents, amount };
}

/** Valid lines, invalid count and total for a set of split rows; blank rows are ignored. */
export function summarizeSplits(rows) {
  let total = 0;
  let invalid = 0;
  const lines = [];
  for (const row of rows) {
    const blank = !row.productId && row.memo.trim() === '' && row.amount.trim() === '' && (row.qty ?? '').trim() === '' && (row.rate ?? '').trim() === '';
    if (blank) continue;
    const { priced, quantityE4, rateCents, amount } = evaluateSplit(row);
    // A stock item is bought by quantity and rate, so its cost per unit is known.
    if (row.accountId === null || amount === null || amount <= 0 || (row.productId && !priced)) {
      invalid += 1;
      continue;
    }
    total += amount;
    lines.push({
      accountId: row.accountId,
      amountCents: amount,
      memo: row.memo.trim() || null,
      productServiceId: row.productId ?? null,
      classId: row.classId ?? null,
      customerId: row.customerId ?? null,
      vendorId: row.vendorId ?? null,
      // Only a cost for a customer can be billed; left null, the company default applies.
      billable: row.customerId ? row.billable : null,
      ...(priced ? { quantityE4, rateCents } : {}),
    });
  }
  return { lines, invalid, total };
}

/**
 * The columns between # and Amount, in their usual order. Customize Columns hides and reorders them
 * (the account, description, quantity and rate always show); each appears only on forms that have it.
 */
const SPLIT_COLUMNS = [
  { id: 'stock', label: 'Stock item', width: 'w-[150px]' },
  { id: 'from', label: 'Received from', width: 'w-[180px]' },
  { id: 'category', label: 'Account', width: 'w-[260px]', required: true },
  { id: 'description', label: 'Description', width: '', required: true },
  { id: 'job', label: 'Customer:Job', width: 'w-[170px]', title: 'The customer or job this cost is for (job costing)' },
  { id: 'bill', label: 'Bill?', width: 'w-[34px]', title: 'Billable: invoice this cost to the customer from Sales › Bill Costs' },
  { id: 'class', label: 'Class', width: 'w-[150px]' },
  { id: 'qty', label: 'Qty', width: 'w-[70px]', num: true, required: true },
  { id: 'rate', label: 'Rate', width: 'w-[96px]', num: true, required: true },
];

/**
 * Category / description / amount split rows with an "Add split row" link. Every cell is an input
 * (data-entry grid), sized to the row so editing causes no layout shift. `withClasses` adds a Class
 * column (once the company has classes) for documents whose lines carry one. `withJobs` adds a
 * Customer:Job column (once the company has customers): the job a cost is for, for job costing.
 * `withReceivedFrom` adds a Received From column (deposits): the customer or vendor the money
 * came from, kept as the row's `customerId` or `vendorId`.
 */
export default function SplitGrid({ rows, setRows, accounts, filter, categoryLabel = 'Category', defaultAccountId = null, footer, withPricing = false, withClasses: classesAllowed = false, withJobs: jobsAllowed = false, withReceivedFrom = false, currency = null, stockItems = [] }) {
  const tracking = useClassTracking();
  const { classes } = tracking;
  const withClasses = classesAllowed && showsClassColumn(tracking, rows);
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const withJobs = jobsAllowed && (customers.length > 0 || rows.some((r) => r.customerId));
  const vendors = useLedgerQuery(withReceivedFrom ? api.listVendors : noVendors).data ?? [];
  const nameValue = (row) => (row.customerId ? `c:${row.customerId}` : row.vendorId ? `v:${row.vendorId}` : '');
  const pickName = (key, value) => {
    const [kind, id] = value.split(':');
    update(key, { customerId: kind === 'c' ? Number(id) : null, vendorId: kind === 'v' ? Number(id) : null });
  };
  const prefs = useLedgerQuery(api.getPreferences).data;
  const billableByDefault = prefs?.expenses_billable_default === 'true';
  // Tracked products can be bought on a line: the line then posts to the item's inventory account.
  const withStock = withPricing && stockItems.length > 0;
  const pickItem = (key, productId) => {
    const item = stockItems.find((i) => i.productId === productId);
    update(key, item ? { productId, accountId: item.assetAccountId, memo: item.name } : { productId: null });
  };
  const money = (cents) => formatMoney(cents, currency);
  const update = (key, patch) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const remove = (key) => setRows((rs) => (rs.length > 1 ? rs.filter((r) => r.key !== key) : [blankSplit(defaultAccountId)]));
  const { total } = summarizeSplits(rows);
  // Ctrl+Insert adds a blank row above the one with focus (at the end from outside the rows);
  // Ctrl+Delete deletes the row with focus.
  const tableRef = useRef(null);
  useLineCommands(tableRef, {
    insert: (at) => setRows((rs) => [...rs.slice(0, at), blankSplit(defaultAccountId), ...rs.slice(at)]),
    remove: (at) => setRows((rs) => (rs.length > 1 ? rs.filter((_, i) => i !== at) : [blankSplit(defaultAccountId)])),
    // Copy Line / Paste Line (Ctrl+Alt+Y / Ctrl+Alt+V): the pasted line goes above the one with focus.
    copy: (at) => rows[at] ?? null,
    paste: (at, line) => setRows((rs) => pasteLineAt(rs, at, line, () => nextKey++)),
    grid: 'splits',
  });

  // The columns between # and Amount as the person arranged them (Customize Columns), of those
  // this form has.
  const offered = { stock: withStock, from: withReceivedFrom, job: withJobs, bill: withJobs, class: withClasses, qty: withPricing, rate: withPricing };
  const cols = useColumns('split-lines', SPLIT_COLUMNS, (id) => offered[id] ?? true);
  const middle = cols.columns;
  const headLabel = (c) => (c.id === 'category' ? categoryLabel : c.label);
  /** One row's cell in column `id`. */
  const cell = (id, row, i, evaluated) => {
    switch (id) {
      case 'stock':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <select value={row.productId ?? ''} onChange={(e) => pickItem(row.key, e.target.value ? Number(e.target.value) : null)} aria-label={`Stock item ${i + 1}`}>
              <option value="">—</option>
              {stockItems.map((item) => (
                <option key={item.productId} value={item.productId}>
                  {item.name}
                </option>
              ))}
            </select>
          </td>
        );
      case 'from':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <select value={nameValue(row)} onChange={(e) => pickName(row.key, e.target.value)} aria-label={`Received from ${i + 1}`}>
              <option value="">—</option>
              <optgroup label="Customers">
                {customers.filter((c) => c.isActive || c.id === row.customerId).map((c) => (
                  <option key={`c${c.id}`} value={`c:${c.id}`}>
                    {c.name}
                  </option>
                ))}
              </optgroup>
              <optgroup label="Vendors">
                {vendors.filter((v) => v.isActive !== false || v.id === row.vendorId).map((v) => (
                  <option key={`v${v.id}`} value={`v:${v.id}`}>
                    {v.name}
                  </option>
                ))}
              </optgroup>
            </select>
          </td>
        );
      case 'category':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <AccountSelect className="" accounts={accounts} filter={filter} value={row.accountId} onChange={(accountId) => update(row.key, { accountId })} placeholder="Select category" aria-label={`${categoryLabel} ${i + 1}`} disabled={Boolean(row.productId)} title={row.productId ? 'A stock item posts to its inventory account' : undefined} />
          </td>
        );
      case 'description':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <input value={row.memo} onChange={(e) => update(row.key, { memo: e.target.value })} placeholder="What was this for?" aria-label={`Description ${i + 1}`} />
          </td>
        );
      case 'job':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <select value={row.customerId ?? ''} onChange={(e) => update(row.key, { customerId: e.target.value ? Number(e.target.value) : null })} aria-label={`Customer or job ${i + 1}`}>
              <option value="">—</option>
              {customers.filter((c) => c.isActive || c.id === row.customerId).map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </td>
        );
      case 'bill':
        return (
          <td key={id} className="qb-grid-cell text-center">
            {row.customerId ? (
              <input
                type="checkbox"
                className="accent-qb-green"
                checked={row.billable ?? billableByDefault}
                onChange={(e) => update(row.key, { billable: e.target.checked })}
                aria-label={`Billable ${i + 1}`}
                title="Invoice this cost to the customer from Sales › Bill Costs"
              />
            ) : null}
          </td>
        );
      case 'class':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <ClassSelect className="" classes={classes} value={row.classId ?? null} onChange={(classId) => update(row.key, { classId })} aria-label={`Class ${i + 1}`} />
          </td>
        );
      case 'qty':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <input className="num" value={row.qty ?? ''} onChange={(e) => update(row.key, { qty: e.target.value })} inputMode="decimal" data-quantity aria-invalid={evaluated.priced && evaluated.quantityE4 === null} aria-label={`Quantity ${i + 1}`} />
          </td>
        );
      case 'rate':
        return (
          <td key={id} className="qb-grid-cell-edit">
            <input className="num" value={row.rate ?? ''} onChange={(e) => update(row.key, { rate: e.target.value })} inputMode="decimal" data-money aria-invalid={evaluated.priced && (evaluated.rateCents === null || evaluated.rateCents <= 0)} aria-label={`Rate ${i + 1}`} />
          </td>
        );
      default:
        return <td key={id} className="qb-grid-cell" />;
    }
  };

  return (
    <table ref={tableRef} className="qb-grid" data-line-grid>
      <colgroup>
        <col className="w-[28px]" />
        {middle.map((c) => (
          <col key={c.id} className={c.width} />
        ))}
        <col className="w-[120px]" />
        <col className="w-[28px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head text-right">#</th>
          {middle.map((c) => (
            <th key={c.id} className={`qb-grid-head ${c.num ? 'text-right' : ''} ${c.id === 'bill' ? 'text-center' : ''}`} title={c.title}>
              {headLabel(c)}
            </th>
          ))}
          <th className="qb-grid-head text-right">Amount</th>
          <th className="qb-grid-head" aria-label="Remove" />
        </tr>
      </thead>
      <tbody>
        {rows.map((row, i) => {
          const evaluated = evaluateSplit(row);
          const cents = evaluated.amount;
          const amountBad = evaluated.priced ? cents === null : row.amount.trim() !== '' && (cents === null || cents <= 0);
          return (
            <tr key={row.key} data-row={i} className="qb-grid-row">
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              {middle.map((c) => cell(c.id, row, i, evaluated))}
              {evaluated.priced ? (
                <td className={`qb-grid-cell num font-semibold ${amountBad ? 'text-qb-negative' : ''}`} title="Quantity × rate">
                  {cents !== null ? money(cents) : '—'}
                </td>
              ) : (
                <td className="qb-grid-cell-edit">
                  <input className="num" value={row.amount} onChange={(e) => update(row.key, { amount: e.target.value })} placeholder="0.00" inputMode="decimal" data-money aria-invalid={amountBad} aria-label={`Amount ${i + 1}`} />
                </td>
              )}
              <td className="qb-grid-cell cell-flush text-center">
                <button type="button" onClick={() => remove(row.key)} className="h-row w-full text-qb-ink-faint hover:text-qb-negative" aria-label={`Remove split ${i + 1}`}>
                  ×
                </button>
              </td>
            </tr>
          );
        })}
        <tr>
          <td />
          <td className="px-2 py-0.5" colSpan={middle.length}>
            <span className="flex items-center gap-3">
              <button type="button" className="text-sm font-semibold text-qb-info hover:underline" onClick={() => setRows((rs) => [...rs, blankSplit(defaultAccountId)])}>
                ＋ Add split row
              </button>
              <ColumnsMenu cols={cols} compact />
            </span>
            {footer}
          </td>
          <td className="qb-grid-cell num font-semibold">{currency ? money(total) : formatCents(total)}</td>
          <td />
        </tr>
      </tbody>
    </table>
  );
}
