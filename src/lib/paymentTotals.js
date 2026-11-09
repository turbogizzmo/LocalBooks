// Receive Payments: the totals under the invoice grid, as the desktop form shows them. Pure, so it
// is tested with node --test (paymentTotals.test.js).

/**
 * `rows` are the customer's open invoices shown (`totalCents`, `balanceCents`); `applied(inv)` and
 * `discount(inv)` the payment and discount typed on each (cents, 0 when blank); `received` the
 * amount received typed (cents), or null when it is blank.
 *   original   the invoices' original amounts
 *   amountDue  what is open on them
 *   applied    the payments applied
 *   discounts  the discounts taken
 *   unapplied  received − applied when received was typed (negative: applied more than received)
 *   leftDue    amountDue − applied − discounts: what stays open after this payment
 */
export function paymentTotals(rows, applied, discount, received = null) {
  const sum = (f) => rows.reduce((s, inv) => s + f(inv), 0);
  const appliedTotal = sum((inv) => Math.max(0, applied(inv)));
  const discountTotal = sum((inv) => Math.max(0, discount(inv)));
  const amountDue = sum((inv) => inv.balanceCents);
  return {
    original: sum((inv) => inv.totalCents),
    amountDue,
    applied: appliedTotal,
    discounts: discountTotal,
    unapplied: received === null ? null : received - appliedTotal,
    leftDue: amountDue - appliedTotal - discountTotal,
  };
}
