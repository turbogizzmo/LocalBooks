// The money bars' extra segments (gap T61), as the bar shows them and where each row opens.
// No imports: CI runs this module's test without node_modules.

const LOOK = {
  UNBILLED: { icon: '◔', color: '#8d5bd1' },
  ESTIMATES: { icon: '✎', color: '#56615f' },
  PURCHASE_ORDERS: { icon: '▤', color: '#c46a00' },
  CARDS: { icon: '▭', color: '#1c2426' },
};

/** A segment from `get_money_bar_segments` as a bar cell: label, icon, color, total, count and its rows. */
export function segmentItem(segment) {
  const look = LOOK[segment.key] ?? { icon: '•', color: '#56615f' };
  return { label: segment.label, icon: look.icon, color: look.color, cents: segment.cents, count: segment.count, list: segment.items };
}

/** The window a segment's row opens: `[view, action, data]`. */
export function openTarget(item) {
  switch (item.kind) {
    case 'COST':
    case 'TIME':
      return ['sales/bill-costs', 'open', { customerId: item.customerId }];
    case 'ESTIMATE':
      return ['sales/estimates', 'edit', { estimateId: item.id }];
    case 'PURCHASE_ORDER':
      return ['expenses/purchase-orders', 'edit', { purchaseOrderId: item.id }];
    case 'CARD':
      return ['accounting/register', 'open', { accountId: item.id }];
    default:
      return null;
  }
}

/** The row's text after its name: the number, then the memo. */
export function itemDetail(item) {
  return [item.number ? `#${item.number}` : null, item.memo].filter(Boolean).join(' · ');
}
