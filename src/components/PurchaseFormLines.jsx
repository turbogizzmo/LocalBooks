import { useEffect, useState } from 'react';
import AccountSelect from './AccountSelect.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents } from '../lib/money.js';
import { blankFormLine, fromItem, layOutPurchase, purchaseLineMessage, rowsFromStored } from '../lib/purchaseLines.js';

/** Income and expense accounts, as `purchase_lines::check` allows. */
const postable = (a) => !a.isSystem && (a.classification === 'INCOME' || a.classification === 'EXPENSE');

/**
 * The discount and charge rows of a purchase form (`docs/guide/bills.md`), and their layout
 * against the form's valid split lines (`lines`, from `summarizeSplits`). Returns `[rows, setRows,
 * laid]`; with `stored` ({ doc, id }), the rows start from that saved document's lines (an edit
 * or a copy). `laid` is `layOutPurchase`'s result: `formLines` to send and the `total`, or `error`.
 * `touched` says the user changed the rows (for the form's unsaved-changes check); setting the rows
 * to a list, as clearing the form does, starts over untouched.
 */
export function usePurchaseFormLines(lines, stored = null) {
  // `stored.inputs`: lines as a form sends them (a memorized transaction), loaded as they are.
  const [rows, setRows] = useState(() => (stored?.inputs ? rowsFromStored(stored.inputs) : []));
  const [touched, setTouched] = useState(false);
  const doc = stored?.doc ?? null;
  const id = stored?.id ?? null;
  useEffect(() => {
    if (!doc || !id) return;
    let live = true;
    api.getPurchaseFormLines(doc, id).then((views) => live && setRows(rowsFromStored(views)), () => {});
    return () => {
      live = false;
    };
  }, [doc, id]);
  const laid = layOutPurchase(lines.map((l) => l.amountCents), rows);
  // The rows' own edits pass a function; a form clearing itself passes a list.
  const setByForm = (next) => {
    setTouched(typeof next === 'function');
    setRows(next);
  };
  return [rows, setByForm, laid, touched];
}

/**
 * The rows under a form's split grid: kind, the split line it follows, item, account, text and
 * amount, and (once the company has customers) the customer or job, with a Bill? box on a charge
 * unless `withBillable` is off (vendor credits and purchase orders: not a cost to bill on).
 */
export default function PurchaseFormLines({ rows, setRows, laid, lines, accounts, withBillable = true }) {
  const items = useLedgerQuery(api.listProducts).data ?? [];
  const customers = useLedgerQuery(api.listCustomers).data ?? [];
  const withJobs = customers.length > 0 || rows.some((r) => r.customerId);
  const prefs = useLedgerQuery(api.getPreferences).data;
  const billableByDefault = prefs?.expenses_billable_default === 'true';
  const update = (key, change) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...change } : r)));
  // The first split line without a discount or charge after it, for a new row.
  const free = lines.findIndex((_, i) => !rows.some((r) => Number(r.afterLine) === i + 1));
  const lineLabel = (l, i) => `${i + 1}. ${l.memo || accounts.find((a) => a.id === l.accountId)?.name || ''} ${formatCents(l.amountCents)}`.trim();
  return (
    <div className="border-t border-qb-border px-2 py-1 text-sm" aria-label="Discounts and charges">
      {rows.length > 0 && (
        <table className="w-full">
          <thead>
            <tr className="text-xs text-qb-ink-muted">
              <th className="text-left font-normal">Kind</th>
              <th className="text-left font-normal">After line</th>
              <th className="text-left font-normal">Item</th>
              <th className="text-left font-normal">Account</th>
              <th className="text-left font-normal">Description</th>
              <th className="text-left font-normal">Amount or %</th>
              <th className="text-right font-normal">Comes to</th>
              {withJobs && <th className="text-left font-normal" title="The customer or job this line is for (job costing)">Customer:Job</th>}
              {withJobs && withBillable && <th className="font-normal" title="Billable: invoice this charge to the customer from Sales › Bill Costs">Bill?</th>}
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const kindItems = items.filter((p) => p.isActive && p.itemType === (r.kind === 'DISCOUNT' ? 'DISCOUNT' : 'OTHER_CHARGE'));
              const amount = laid.amounts?.[i];
              return (
                <tr key={r.key}>
                  <td className="pr-1">
                    <select className="qb-input h-row" aria-label="Discount or charge" value={r.kind} onChange={(e) => update(r.key, { kind: e.target.value, itemId: null, billable: null })}>
                      <option value="DISCOUNT">Discount</option>
                      <option value="CHARGE">Charge</option>
                    </select>
                  </td>
                  <td className="pr-1">
                    <select className="qb-input h-row max-w-[180px]" aria-label="The split line it applies to" value={r.afterLine} onChange={(e) => update(r.key, { afterLine: Number(e.target.value) })}>
                      {lines.map((l, n) => (
                        <option key={n} value={n + 1}>{lineLabel(l, n)}</option>
                      ))}
                      {Number(r.afterLine) > lines.length && <option value={r.afterLine}>Line {r.afterLine} (no longer there)</option>}
                    </select>
                  </td>
                  <td className="pr-1">
                    <select
                      className="qb-input h-row max-w-[160px]"
                      aria-label="Item"
                      value={r.itemId ?? ''}
                      onChange={(e) => update(r.key, fromItem(r, kindItems.find((p) => p.id === Number(e.target.value))))}
                    >
                      <option value="">(none)</option>
                      {kindItems.map((p) => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                  </td>
                  <td className="pr-1">
                    <AccountSelect accounts={accounts} value={r.accountId} onChange={(id) => update(r.key, { accountId: id })} filter={postable} placeholder="Account…" className="qb-input h-row max-w-[200px]" aria-label="Account" />
                  </td>
                  <td className="pr-1">
                    <input className="qb-input h-row w-full" aria-label="Description" value={r.description} placeholder={r.kind === 'DISCOUNT' ? 'Discount' : 'Charge'} onChange={(e) => update(r.key, { description: e.target.value })} />
                  </td>
                  <td className="pr-1">
                    <input className="qb-input num h-row w-[90px]" aria-label="Amount or percent" value={r.value} placeholder="0.00 or 10%" onChange={(e) => update(r.key, { value: e.target.value })} />
                  </td>
                  <td className="num pr-1 text-right">{amount !== undefined ? formatCents(r.kind === 'DISCOUNT' ? -amount : amount) : ''}</td>
                  {withJobs && (
                    <td className="pr-1">
                      <select
                        className="qb-input h-row max-w-[170px]"
                        aria-label="Customer or job"
                        value={r.customerId ?? ''}
                        onChange={(e) => update(r.key, { customerId: e.target.value ? Number(e.target.value) : null, billable: null })}
                      >
                        <option value="">—</option>
                        {customers.filter((c) => c.isActive || c.id === r.customerId).map((c) => (
                          <option key={c.id} value={c.id}>{c.name}</option>
                        ))}
                      </select>
                    </td>
                  )}
                  {withJobs && withBillable && (
                    <td className="pr-1 text-center">
                      {r.customerId && r.kind === 'CHARGE' ? (
                        <input
                          type="checkbox"
                          className="accent-qb-green"
                          checked={r.billable ?? billableByDefault}
                          onChange={(e) => update(r.key, { billable: e.target.checked })}
                          aria-label="Billable"
                          title="Invoice this charge to the customer from Sales › Bill Costs"
                        />
                      ) : null}
                    </td>
                  )}
                  <td>
                    <button type="button" className="qb-btn qb-btn-secondary h-[20px] px-1.5" aria-label="Remove this line" onClick={() => setRows((rs) => rs.filter((x) => x.key !== r.key))}>
                      ✕
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <div className="flex items-center gap-2">
        <button
          type="button"
          className="text-xs text-qb-info hover:underline disabled:text-qb-ink-faint disabled:no-underline"
          disabled={free < 0}
          title={free < 0 ? 'Each split line already has a discount or charge after it' : undefined}
          onClick={() => setRows((rs) => [...rs, blankFormLine(free + 1)])}
        >
          Add a discount or charge
        </button>
        {laid.error && rows.length > 0 && <span className="text-xs text-qb-negative">{purchaseLineMessage(laid.error)}</span>}
      </div>
    </div>
  );
}
