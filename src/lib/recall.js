// My Preferences › General › "Automatically recall information": when a new bill or check is for
// a vendor you have paid before, its lines start as on that vendor's last bill, check or expense.
// `last` copies accounts and amounts; `accounts` copies the accounts and descriptions only.

import { useEffect, useRef } from 'react';
import { api } from './api.js';
import { useMyPreferences } from './myPreferences.js';
import { blankSplit } from '../components/SplitGrid.jsx';
import { centsToInput, formatQuantity } from './money.js';

const sameName = (a, b) => a.trim().localeCompare(b.trim(), undefined, { sensitivity: 'accent' }) === 0;

/** Split rows for a recalled transaction's lines, with or without their amounts. */
export function recalledRows(lines, withAmounts) {
  return lines.map((l) => ({
    ...blankSplit(l.accountId),
    productId: l.productServiceId ?? null,
    classId: l.classId ?? null,
    customerId: l.customerId ?? null,
    billable: l.customerId ? Boolean(l.billable) : null,
    memo: l.memo ?? '',
    amount: withAmounts && !l.quantityE4 ? centsToInput(l.amountCents) : '',
    qty: withAmounts && l.quantityE4 ? formatQuantity(l.quantityE4).replace(/,/g, '') : '',
    rate: withAmounts && l.quantityE4 && l.rateCents ? centsToInput(l.rateCents) : '',
  }));
}

/**
 * Fills a new form's rows from `vendor`'s last transaction once the vendor matches a known one,
 * while the rows are still untouched. `kind` is `bill` or `expense`; `onRecalled(recalled, rows)`
 * receives what was found and the rows to show.
 */
export function useVendorRecall({ vendor, vendors, rows, enabled, kind, onRecalled }) {
  const { recallVendorTransaction: mode } = useMyPreferences();
  const done = useRef('');
  const untouched = rows.every((r) => r.memo.trim() === '' && r.amount.trim() === '' && (r.qty ?? '').trim() === '' && (r.rate ?? '').trim() === '' && !r.customerId);
  useEffect(() => {
    if (!enabled || mode === 'off' || !untouched) return undefined;
    const known = (vendors ?? []).find((v) => sameName(v.name ?? v, vendor));
    if (!known) return undefined;
    const name = known.name ?? known;
    if (sameName(done.current, name)) return undefined;
    let cancelled = false;
    api.getRecalledVendorTransaction(name, kind).then(
      (recalled) => {
        if (cancelled || !recalled?.lines?.length) return;
        done.current = name;
        onRecalled(recalled, recalledRows(recalled.lines, mode === 'last'));
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [vendor, vendors, enabled, mode, untouched, kind]); // eslint-disable-line react-hooks/exhaustive-deps
}
