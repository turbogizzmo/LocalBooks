// Subtotals and the other non-item lines on a sales form, laid out exactly as the ledger lays
// them out (`src-tauri/src/item_types.rs`, `docs/plans/item-types.md`). A Rust test runs the same
// cases through both.

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
  for (let position = 0; position <= items.length; position += 1) {
    formLines.forEach((f, index) => {
      if (f.afterLines !== position) return;
      number += 1;
      let amountCents = 0;
      if (f.kind === 'SUBTOTAL') {
        amountCents = sinceSubtotal;
        sinceSubtotal = 0;
      }
      rows.push({ number, form: index, amountCents });
    });
    if (position < items.length) {
      const item = items[position];
      number += 1;
      const amount = item.amountCents ?? 0;
      sinceSubtotal += amount;
      linesCents += amount;
      if (item.isTaxable) taxableCents += amount;
      rows.push({ number, item: position });
    }
  }
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
