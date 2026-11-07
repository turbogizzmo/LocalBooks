// Subtotals and the other non-item lines on a sales form, laid out exactly as the ledger lays
// them out (`src-tauri/src/item_types.rs`, `docs/plans/item-types.md`). A Rust test runs the same
// cases through both.

import { mulDivRound } from './money.js';

/**
 * Merges item lines ({ amountCents, isTaxable }) and form lines ({ afterLines, kind, ... }) into
 * line order. Returns { rows: [{ number, item } | { number, form, amountCents }], linesCents,
 * taxableCents } or { error } with the ledger's error code.
 */
export function layOut(items, formLines = []) {
  if (formLines.some((f) => f.afterLines > items.length)) return { error: 'BAD_FORM_LINE' };
  const rows = [];
  let number = 0;
  let sinceSubtotal = 0;
  let linesCents = 0;
  let taxableCents = 0;
  // The amount of the line above, when a discount may apply to it.
  let base = null;
  // The open group: its heading's row, the position after its last item, and its total so far.
  let group = null;
  let error = null;
  for (let position = 0; position <= items.length && !error; position += 1) {
    for (const [index, f] of formLines.entries()) {
      if (f.afterLines !== position || error) continue;
      number += 1;
      if (group) {
        error = 'INSIDE_GROUP';
        break;
      }
      let amountCents = 0;
      if (f.kind === 'SUBTOTAL') {
        amountCents = sinceSubtotal;
        sinceSubtotal = 0;
        base = amountCents;
      } else if (f.kind === 'DISCOUNT') {
        const above = base;
        base = null;
        if (above === null || above <= 0) {
          error = 'DISCOUNT_NEEDS_BASE';
          break;
        }
        const ppm = f.percentPpm ?? null;
        const cents = f.amountCents ?? null;
        let off;
        if (ppm !== null && cents === null && ppm >= 1 && ppm <= 1_000_000) off = mulDivRound(above, ppm, 1_000_000) ?? 0;
        else if (ppm === null && cents !== null && cents > 0) off = cents;
        else {
          error = 'BAD_DISCOUNT_AMOUNT';
          break;
        }
        if (off < 1) error = 'DISCOUNT_ROUNDS_TO_ZERO';
        else if (off > above) error = 'DISCOUNT_TOO_LARGE';
        if (error) break;
        sinceSubtotal -= off;
        linesCents -= off;
        if (f.isTaxable) taxableCents -= off;
        amountCents = -off;
      } else if (f.kind === 'GROUP') {
        const count = f.groupLineCount ?? 0;
        if (count < 1 || position + count > items.length || (f.groupQuantityE4 ?? 1) <= 0) {
          error = 'BAD_GROUP';
          break;
        }
        group = { row: rows.length, end: position + count, total: 0 };
        base = null;
      } else if (f.kind === 'CHARGE') {
        const above = base;
        base = null;
        if (above === null || above <= 0) {
          error = 'CHARGE_NEEDS_BASE';
          break;
        }
        const ppm = f.percentPpm ?? null;
        if (ppm === null || ppm < 1 || ppm > 1_000_000) {
          error = 'BAD_CHARGE_PERCENT';
          break;
        }
        const on = mulDivRound(above, ppm, 1_000_000) ?? 0;
        if (on < 1) {
          error = 'CHARGE_ROUNDS_TO_ZERO';
          break;
        }
        sinceSubtotal += on;
        linesCents += on;
        if (f.isTaxable) taxableCents += on;
        amountCents = on;
      }
      rows.push({ number, form: index, amountCents });
    }
    if (error) break;
    if (position < items.length) {
      const item = items[position];
      number += 1;
      const amount = item.amountCents ?? 0;
      sinceSubtotal += amount;
      linesCents += amount;
      if (item.isTaxable) taxableCents += amount;
      base = amount;
      rows.push({ number, item: position });
      if (group) {
        group.total += amount;
        if (position + 1 === group.end) {
          rows[group.row].amountCents = group.total;
          base = group.total;
          group = null;
        }
      }
    }
  }
  if (!error && taxableCents < 0) error = 'TAXABLE_NEGATIVE';
  if (error) return { error };
  return { rows, linesCents, taxableCents };
}

/**
 * A stored document's lines (item views with `lineNumber`, and its `formLines`) back in line
 * order: [{ item }] and [{ form }] entries.
 */
export function documentOrder(items, formLines = []) {
  return [
    ...items.map((item, i) => ({ number: item.lineNumber ?? i + 1, item })),
    ...(formLines ?? []).map((form) => ({ number: form.lineNumber, form })),
  ].sort((a, b) => a.number - b.number);
}

/** "10%" or "2.5 %" → parts per million (0.0001% to 100%), else null. */
export function parsePercentPpm(text) {
  const m = /^\s*(\d{1,3})(?:\.(\d{1,4}))?\s*%\s*$/.exec(String(text ?? ''));
  if (!m) return null;
  const ppm = Number(m[1]) * 10_000 + Number((m[2] ?? '').padEnd(4, '0'));
  return ppm >= 1 && ppm <= 1_000_000 ? ppm : null;
}

/** "10%" from parts per million. */
export function percentText(ppm) {
  const whole = Math.trunc(ppm / 10_000);
  const frac = ppm % 10_000;
  return frac === 0 ? `${whole}%` : `${whole}.${String(frac).padStart(4, '0').replace(/0+$/, '')}%`;
}

/** What to tell the user when the lines can't be laid out (the ledger's error, in words). */
export function layoutMessage(code) {
  return (
    {
      DISCOUNT_NEEDS_BASE: 'A discount must come right after an item or a subtotal, and that line must be above zero.',
      BAD_DISCOUNT_AMOUNT: 'Enter each discount as an amount above zero or a percentage such as 10%.',
      DISCOUNT_ROUNDS_TO_ZERO: 'A percentage discount rounds to 0.00; use a larger percentage or an amount.',
      DISCOUNT_TOO_LARGE: 'A discount is more than the line above it.',
      CHARGE_NEEDS_BASE: 'A percent charge must come right after an item or a subtotal, and that line must be above zero.',
      BAD_CHARGE_PERCENT: 'Enter each percent charge as a percentage such as 3%.',
      CHARGE_ROUNDS_TO_ZERO: 'A percent charge rounds to 0.00; use a larger percentage or a fixed charge.',
      INSIDE_GROUP: 'A group holds only its items; move the subtotal, discount or charge below the group.',
      BAD_GROUP: 'A group must be followed by its items.',
      TAXABLE_NEGATIVE: 'The taxable discounts are more than the taxable lines; untick Tax on a discount.',
    }[code] ?? 'The lines don’t add up; check the subtotals and discounts.'
  );
}
