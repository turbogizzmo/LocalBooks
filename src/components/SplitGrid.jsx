import AccountSelect from './AccountSelect.jsx';
import { formatCents, parseMoneyToCents } from '../lib/money.js';

let nextKey = 1;
export const blankSplit = (accountId = null) => ({ key: nextKey++, accountId, memo: '', amount: '' });

/** Valid lines, invalid count and total for a set of split rows; blank rows are ignored. */
export function summarizeSplits(rows) {
  let total = 0;
  let invalid = 0;
  const lines = [];
  for (const row of rows) {
    const blank = row.memo.trim() === '' && row.amount.trim() === '';
    if (blank) continue;
    const cents = parseMoneyToCents(row.amount);
    if (row.accountId === null || cents === null || cents <= 0) {
      invalid += 1;
      continue;
    }
    total += cents;
    lines.push({ accountId: row.accountId, amountCents: cents, memo: row.memo.trim() || null });
  }
  return { lines, invalid, total };
}

/**
 * Category / description / amount split rows with an "Add split row" link. Every cell is an input
 * (data-entry grid), sized to the row so editing causes no layout shift.
 */
export default function SplitGrid({ rows, setRows, accounts, filter, categoryLabel = 'Category', defaultAccountId = null, footer }) {
  const update = (key, patch) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const remove = (key) => setRows((rs) => (rs.length > 1 ? rs.filter((r) => r.key !== key) : [blankSplit(defaultAccountId)]));
  const { total } = summarizeSplits(rows);

  return (
    <table className="qb-grid">
      <colgroup>
        <col className="w-[28px]" />
        <col className="w-[260px]" />
        <col />
        <col className="w-[120px]" />
        <col className="w-[28px]" />
      </colgroup>
      <thead>
        <tr>
          <th className="qb-grid-head text-right">#</th>
          <th className="qb-grid-head">{categoryLabel}</th>
          <th className="qb-grid-head">Description</th>
          <th className="qb-grid-head text-right">Amount</th>
          <th className="qb-grid-head" aria-label="Remove" />
        </tr>
      </thead>
      <tbody>
        {rows.map((row, i) => {
          const cents = parseMoneyToCents(row.amount);
          const amountBad = row.amount.trim() !== '' && (cents === null || cents <= 0);
          return (
            <tr key={row.key} className="qb-grid-row">
              <td className="qb-grid-cell text-right text-qb-ink-faint">{i + 1}</td>
              <td className="qb-grid-cell-edit">
                <AccountSelect className="" accounts={accounts} filter={filter} value={row.accountId} onChange={(accountId) => update(row.key, { accountId })} placeholder="Select category" aria-label={`${categoryLabel} ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input value={row.memo} onChange={(e) => update(row.key, { memo: e.target.value })} placeholder="What was this for?" aria-label={`Description ${i + 1}`} />
              </td>
              <td className="qb-grid-cell-edit">
                <input className="num" value={row.amount} onChange={(e) => update(row.key, { amount: e.target.value })} placeholder="0.00" inputMode="decimal" aria-invalid={amountBad} aria-label={`Amount ${i + 1}`} />
              </td>
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
          <td className="px-2 py-0.5" colSpan={2}>
            <button type="button" className="text-sm font-semibold text-qb-info hover:underline" onClick={() => setRows((rs) => [...rs, blankSplit(defaultAccountId)])}>
              ＋ Add split row
            </button>
            {footer}
          </td>
          <td className="qb-grid-cell num font-semibold">{formatCents(total)}</td>
          <td />
        </tr>
      </tbody>
    </table>
  );
}
