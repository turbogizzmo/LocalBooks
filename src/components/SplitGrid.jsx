import AccountSelect from './AccountSelect.jsx';
import ClassSelect, { showsClassColumn, useClassTracking } from './ClassSelect.jsx';
import { centsToInput, formatCents, formatMoney, formatQuantity, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';

let nextKey = 1;
export const blankSplit = (accountId = null) => ({ key: nextKey++, accountId, productId: null, classId: null, memo: '', amount: '', qty: '', rate: '' });

/** Split rows for a posted bill's or expense's lines, to edit it. */
export const splitRowsFrom = (lines) =>
  lines.map((l) => ({
    ...blankSplit(l.accountId),
    productId: l.productServiceId ?? null,
    classId: l.classId ?? null,
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
      ...(priced ? { quantityE4, rateCents } : {}),
    });
  }
  return { lines, invalid, total };
}

/**
 * Category / description / amount split rows with an "Add split row" link. Every cell is an input
 * (data-entry grid), sized to the row so editing causes no layout shift. `withClasses` adds a Class
 * column (once the company has classes) for documents whose lines carry one.
 */
export default function SplitGrid({ rows, setRows, accounts, filter, categoryLabel = 'Category', defaultAccountId = null, footer, withPricing = false, withClasses: classesAllowed = false, currency = null, stockItems = [] }) {
  const tracking = useClassTracking();
  const { classes } = tracking;
  const withClasses = classesAllowed && showsClassColumn(tracking, rows);
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

  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[28px]" />
        {withStock && <col className="w-[150px]" />}
        <col className="w-[260px]" />
        <col />
        {withClasses && <col className="w-[150px]" />}
        {withPricing && <col className="w-[70px]" />}
        {withPricing && <col className="w-[96px]" />}
        <col className="w-[120px]" />
        <col className="w-[28px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head text-right">#</th>
          {withStock && <th className="qb-grid-head">Stock item</th>}
          <th className="qb-grid-head">{categoryLabel}</th>
          <th className="qb-grid-head">Description</th>
          {withClasses && <th className="qb-grid-head">Class</th>}
          {withPricing && <th className="qb-grid-head text-right">Qty</th>}
          {withPricing && <th className="qb-grid-head text-right">Rate</th>}
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
            <tr key={row.key} className="qb-grid-row">
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              {withStock && (
                <td className="qb-grid-cell-edit">
                  <select value={row.productId ?? ''} onChange={(e) => pickItem(row.key, e.target.value ? Number(e.target.value) : null)} aria-label={`Stock item ${i + 1}`}>
                    <option value="">—</option>
                    {stockItems.map((item) => (
                      <option key={item.productId} value={item.productId}>
                        {item.name}
                      </option>
                    ))}
                  </select>
                </td>
              )}
              <td className="qb-grid-cell-edit">
                <AccountSelect className="" accounts={accounts} filter={filter} value={row.accountId} onChange={(accountId) => update(row.key, { accountId })} placeholder="Select category" aria-label={`${categoryLabel} ${i + 1}`} disabled={Boolean(row.productId)} title={row.productId ? 'A stock item posts to its inventory account' : undefined} />
              </td>
              <td className="qb-grid-cell-edit">
                <input value={row.memo} onChange={(e) => update(row.key, { memo: e.target.value })} placeholder="What was this for?" aria-label={`Description ${i + 1}`} />
              </td>
              {withClasses && (
                <td className="qb-grid-cell-edit">
                  <ClassSelect className="" classes={classes} value={row.classId ?? null} onChange={(classId) => update(row.key, { classId })} aria-label={`Class ${i + 1}`} />
                </td>
              )}
              {withPricing && (
                <td className="qb-grid-cell-edit">
                  <input className="num" value={row.qty ?? ''} onChange={(e) => update(row.key, { qty: e.target.value })} inputMode="decimal" aria-invalid={evaluated.priced && evaluated.quantityE4 === null} aria-label={`Quantity ${i + 1}`} />
                </td>
              )}
              {withPricing && (
                <td className="qb-grid-cell-edit">
                  <input className="num" value={row.rate ?? ''} onChange={(e) => update(row.key, { rate: e.target.value })} inputMode="decimal" aria-invalid={evaluated.priced && (evaluated.rateCents === null || evaluated.rateCents <= 0)} aria-label={`Rate ${i + 1}`} />
                </td>
              )}
              {evaluated.priced ? (
                <td className={`qb-grid-cell num font-semibold ${amountBad ? 'text-qb-negative' : ''}`} title="Quantity × rate">
                  {cents !== null ? money(cents) : '—'}
                </td>
              ) : (
                <td className="qb-grid-cell-edit">
                  <input className="num" value={row.amount} onChange={(e) => update(row.key, { amount: e.target.value })} placeholder="0.00" inputMode="decimal" aria-invalid={amountBad} aria-label={`Amount ${i + 1}`} />
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
          <td className="px-2 py-0.5" colSpan={(withPricing ? 4 : 2) + (withStock ? 1 : 0) + (withClasses ? 1 : 0)}>
            <button type="button" className="text-sm font-semibold text-qb-info hover:underline" onClick={() => setRows((rs) => [...rs, blankSplit(defaultAccountId)])}>
              ＋ Add split row
            </button>
            {footer}
          </td>
          <td className="qb-grid-cell num font-semibold">{currency ? money(total) : formatCents(total)}</td>
          <td />
        </tr>
      </tbody>
    </table>
  );
}
