// Pay Bills: which open bills the list shows, and in what order (Filter By and Sort By above the
// list, as the desktop form has them). Pure, so it is tested with node --test (payBillsView.test.js).

/** Sort By choices: [id, label]. */
export const PAY_BILLS_SORTS = [
  ['due', 'Due date'],
  ['discount', 'Discount date'],
  ['vendor', 'Vendor'],
  ['amount', 'Amount due'],
];

/**
 * `bills` are the open bills (`vendorId`, `vendorName`, `dueDate`, `discountUntil`, `balanceCents`,
 * `id`). `vendorId` null shows every vendor's. Ties go by due date, then vendor, then id, so the
 * order is stable. Bills without a discount sort after those with one under Discount date; the
 * largest amount comes first under Amount due.
 */
export function payBillsView(bills, { vendorId = null, sortBy = 'due' } = {}) {
  const shown = (bills ?? []).filter((b) => vendorId === null || b.vendorId === vendorId);
  const byDue = (a, b) => a.dueDate.localeCompare(b.dueDate) || a.vendorName.localeCompare(b.vendorName) || a.id - b.id;
  const compare = {
    due: byDue,
    discount: (a, b) => {
      if (a.discountUntil && b.discountUntil) return a.discountUntil.localeCompare(b.discountUntil) || byDue(a, b);
      if (a.discountUntil) return -1;
      if (b.discountUntil) return 1;
      return byDue(a, b);
    },
    vendor: (a, b) => a.vendorName.localeCompare(b.vendorName) || byDue(a, b),
    amount: (a, b) => b.balanceCents - a.balanceCents || byDue(a, b),
  }[sortBy] ?? byDue;
  return [...shown].sort(compare);
}

/** The vendors with open bills, by name, for Filter By. */
export function payBillsVendors(bills) {
  const seen = new Map();
  for (const b of bills ?? []) if (!seen.has(b.vendorId)) seen.set(b.vendorId, b.vendorName);
  return [...seen].map(([id, name]) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name));
}
