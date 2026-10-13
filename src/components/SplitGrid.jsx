import AccountSelect from './AccountSelect.jsx';
import { formatCents, formatMoney, parseMoneyToCents, parseQuantityE4, quantityAmount } from '../lib/money.js';

let nextKey = 1;
export const blankSplit = (accountId = null) => ({ key: nextKey++, accountId, memo: '', amount: '', qty: '', rate: '' });

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
    const blank = row.memo.trim() === '' && row.amount.trim() === '' && (row.qty ?? '').trim() === '' && (row.rate ?? '').trim() === '';
    if (blank) continue;
    const { priced, quantityE4, rateCents, amount } = evaluateSplit(row);
    if (row.accountId === null || amount === null || amount <= 0) {
      invalid += 1;
      continue;
    }
    total += amount;
    lines.push({
      accountId: row.accountId,
      amountCents: amount,
      memo: row.memo.trim() || null,
      ...(priced ? { quantityE4, rateCents } : {}),
    });
  }
  return { lines, invalid, total };
}

/**
 * Category / description / amount split rows with an "Add split row" link. Every cell is an input
 * (data-entry grid), sized to the row so editing causes no layout shift.
 */
export default function SplitGrid({ rows, setRows, accounts, filter, categoryLabel = 'Category', defaultAccountId = null, footer, withPricing = false, currency = null }) {
  const money = (cents) => formatMoney(cents, currency);
  const update = (key, patch) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const remove = (key) => setRows((rs) => (rs.length > 1 ? rs.filter((r) => r.key !== key) : [blankSplit(defaultAccountId)]));
  const { total } = summarizeSplits(rows);

  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[28px]" />
        <col className="w-[260px]" />
        <col />
        {withPricing && <col className="w-[70px]" />}
        {withPricing && <col className="w-[96px]" />}
        <col className="w-[120px]" />
        <col className="w-[28px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head text-right">#</th>
          <th className="qb-grid-head">{categoryLabel}</th>
          <th className="qb-grid-head">Description</th>
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
              <td className="qb-grid-cell-edit">
                <AccountSelect className="" accounts={accounts} filter={filter} value={row.accountId} onChange={(accountId) => update(row.key, { accountId })} placeholder="Select category" aria-label={`${categoryLabel} ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input value={row.memo} onChange={(e) => update(row.key, { memo: e.target.value })} placeholder="What was this for?" aria-label={`Description ${i + 1}`} />
              </td>
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
          <td className="px-2 py-0.5" colSpan={withPricing ? 4 : 2}>
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
