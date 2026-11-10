// Discount and charge lines on the purchase forms (bills, checks and expenses, vendor credits and
// purchase orders), laid out as the ledger lays them out (`src-tauri/src/purchase_lines.rs`): each
// follows one split line and is a fixed amount or a percent of it; a discount comes off the total
// and a charge is added. Imports only sibling modules (CI runs the test without node_modules).

import { centsToInput, mulDivRound, parseMoneyToCents } from './money.js';
import { parsePercentPpm, percentText } from './itemTypes.js';

let nextKey = 1;

/** A new discount or charge row, after split line `afterLine`. */
export const blankFormLine = (afterLine = 1, kind = 'DISCOUNT') => ({ key: nextKey++, afterLine, kind, itemId: null, accountId: null, classId: null, description: '', value: '' });

/** "10%" → { percentPpm }, "25.00" → { amountCents }, else null. */
export function readValue(text) {
  const t = String(text ?? '').trim();
  if (t.endsWith('%')) {
    const ppm = parsePercentPpm(t);
    return ppm === null ? null : { percentPpm: ppm, amountCents: null };
  }
  const cents = parseMoneyToCents(t);
  return cents === null || cents <= 0 ? null : { percentPpm: null, amountCents: cents };
}

/**
 * Lays the rows out against the split lines' amounts. Returns { formLines (what the ledger takes),
 * amounts, total } or { error } with a code `purchaseLineMessage` turns into words.
 */
export function layOutPurchase(splitAmounts, rows) {
  let total = splitAmounts.reduce((s, a) => s + a, 0);
  const formLines = [];
  const amounts = [];
  const placed = new Set();
  for (const row of rows) {
    const after = Number(row.afterLine);
    if (!Number.isInteger(after) || after < 1 || after > splitAmounts.length) return { error: 'PLACE' };
    if (placed.has(after)) return { error: 'NEEDS_BASE' };
    placed.add(after);
    if (row.accountId === null || row.accountId === undefined) return { error: 'ACCOUNT' };
    const value = readValue(row.value);
    if (!value) return { error: 'AMOUNT' };
    const base = splitAmounts[after - 1];
    const amount = value.percentPpm !== null ? mulDivRound(base, value.percentPpm, 1_000_000) : value.amountCents;
    if (amount === null || amount < 1) return { error: 'ZERO' };
    if (row.kind === 'DISCOUNT' && amount > base) return { error: 'TOO_LARGE' };
    total += row.kind === 'DISCOUNT' ? -amount : amount;
    amounts.push(amount);
    formLines.push({
      afterLine: after,
      kind: row.kind,
      description: row.description.trim(),
      productServiceId: row.itemId ?? null,
      percentPpm: value.percentPpm,
      amountCents: value.amountCents,
      accountId: row.accountId,
      classId: row.classId ?? null,
    });
  }
  if (total < 1) return { error: 'TOTAL' };
  return { formLines, amounts, total };
}

/** The ledger's refusal, in words. */
export function purchaseLineMessage(code) {
  return (
    {
      PLACE: 'Each discount or charge must follow one of the split lines.',
      NEEDS_BASE: 'Only one discount or charge can follow a split line; it applies to that line.',
      ACCOUNT: 'Choose an account for each discount or charge.',
      AMOUNT: 'Enter each discount or charge as an amount above zero or a percentage such as 10%.',
      ZERO: 'A percentage comes to less than a cent; use a larger percentage or an amount.',
      TOO_LARGE: 'A discount is more than the split line above it.',
      TOTAL: 'The discounts take the whole amount off; the total must be above zero.',
    }[code] ?? 'Check the discount and charge lines.'
  );
}

/** Stored lines (`get_purchase_form_lines`) back as rows to edit or copy. */
export function rowsFromStored(views) {
  return (views ?? []).map((v) => ({
    key: nextKey++,
    afterLine: v.afterLine,
    kind: v.kind,
    itemId: v.productServiceId ?? null,
    accountId: v.accountId,
    classId: v.classId ?? null,
    description: v.description,
    value: v.percentPpm ? percentText(v.percentPpm) : centsToInput(v.amountCents),
  }));
}

/** A row filled from a Discount or Other Charge item: its purchase account, text and amount. */
export function fromItem(row, item) {
  if (!item) return { ...row, itemId: null };
  return {
    ...row,
    itemId: item.id,
    accountId: item.expenseAccountId ?? item.incomeAccountId ?? row.accountId,
    description: item.description || item.name,
    value: item.percentPpm ? percentText(item.percentPpm) : item.rateCents > 0 ? centsToInput(item.rateCents) : row.value,
  };
}

/**
 * A receipt's bill: the purchase order's discounts and charges (`get_receipt_bill_form_lines`) laid
 * out against the billed amounts, as the ledger does it. A fixed discount more than its line is held
 * for a later bill (`purchase_lines::fit_carried`). Returns `layOutPurchase`'s result plus `lines`,
 * the form lines kept.
 */
export function layOutCarried(splitAmounts, formLines) {
  const kept = (formLines ?? []).filter((f) => !(f.kind === 'DISCOUNT' && f.amountCents !== null && f.amountCents !== undefined && f.amountCents > (splitAmounts[f.afterLine - 1] ?? 0)));
  const rows = kept.map((f) => ({
    afterLine: f.afterLine,
    kind: f.kind,
    itemId: f.productServiceId ?? null,
    accountId: f.accountId,
    classId: f.classId ?? null,
    description: f.description ?? '',
    value: f.percentPpm ? percentText(f.percentPpm) : centsToInput(f.amountCents),
  }));
  return { ...layOutPurchase(splitAmounts, rows), lines: kept };
}
