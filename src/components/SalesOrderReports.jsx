import { TableReport } from './MoreReports.jsx';
import { plainAmount } from './ReportParts.jsx';
import { api, useLedgerQuery } from '../lib/api.js';
import { formatCents, formatQuantity } from '../lib/money.js';
import { formatDate, todayIso } from '../lib/dates.js';

/** Quantities for CSV and Excel: no thousands separators. */
const plainQty = (e4) => formatQuantity(e4).replace(/,/g, '');
const asOf = () => `As of ${formatDate(todayIso())}`;
const itemLabel = (l) => l.itemName ?? l.description;
const sum = (lines, key) => lines.reduce((s, l) => s + l[key], 0);

/** Open lines grouped by `key`, groups by name, lines keeping their order (oldest order first). */
function groupBy(lines, key, label) {
  const groups = new Map();
  for (const l of lines) {
    const k = key(l);
    if (!groups.has(k)) groups.set(k, { label: label(l), lines: [] });
    groups.get(k).lines.push(l);
  }
  return [...groups.values()].sort((a, b) => a.label.localeCompare(b.label));
}

const QTY = ['Ordered', 'Invoiced', 'Left', 'Backordered'];
/** Group column, then the line's own: what (item or customer), order, dates, quantities, open amount. */
const columns = (group, first) => [
  { label: group },
  { label: first },
  { label: 'Order' },
  { label: 'Date' },
  { label: 'Expected' },
  ...QTY.map((label) => ({ label, align: 'right' })),
  { label: 'Open amount', align: 'right' },
];
const WIDTH = 10;
const lineCells = (l, first) => [
  '',
  first(l),
  `#${l.orderNumber}`,
  formatDate(l.orderDate),
  l.expectedDate ? formatDate(l.expectedDate) : '',
  formatQuantity(l.quantityE4),
  formatQuantity(l.invoicedQtyE4),
  formatQuantity(l.leftQtyE4),
  l.backorderedQtyE4 ? formatQuantity(l.backorderedQtyE4) : '',
  formatCents(l.openCents),
];
const csvLine = (group, l, first) => [
  group,
  first(l),
  String(l.orderNumber),
  l.orderDate,
  l.expectedDate ?? '',
  plainQty(l.quantityE4),
  plainQty(l.invoicedQtyE4),
  plainQty(l.leftQtyE4),
  plainQty(l.backorderedQtyE4),
  plainAmount(l.openCents),
];
const section = (text) => ({ cells: [text, ...Array(WIDTH - 1).fill('')], kind: 'section', span: WIDTH });
/** A total row: label first, the open amount last, and optionally the quantity left. */
const totalRow = (label, openCents, kind, leftE4 = null) => {
  const cells = [label, ...Array(WIDTH - 1).fill('')];
  if (leftE4 !== null) cells[7] = formatQuantity(leftE4);
  cells[WIDTH - 1] = formatCents(openCents);
  return { cells, kind };
};

/** Open Sales Orders by Customer: each open line under its customer, with subtotals. */
export function OpenSalesOrdersByCustomer() {
  const { data, error } = useLedgerQuery(api.getOpenSalesOrders);
  const rows = groupBy(data?.lines ?? [], (l) => l.customerId, (l) => l.customerName).flatMap((g) => [
    section(g.label),
    ...g.lines.map((l) => ({ cells: lineCells(l, itemLabel) })),
    totalRow(`Total ${g.label}`, sum(g.lines, 'openCents'), 'total'),
  ]);
  if (data) rows.push(totalRow('Total', data.totalOpenCents, 'grand'));
  return (
    <TableReport
      title="Open Sales Orders by Customer"
      subtitle={asOf()}
      columns={columns('Customer', 'Item')}
      rows={rows}
      error={error}
      footer="Left: ordered less invoiced. Backordered: the part of what is left that stock on hand can't cover, oldest orders first. Open amount: left × the order's rate, before tax."
      onCsvRows={() => (data?.lines ?? []).map((l) => csvLine(l.customerName, l, itemLabel))}
    />
  );
}

/** Open Sales Orders by Item: each open line under its item, with on hand, on sales orders and available. */
export function OpenSalesOrdersByItem() {
  const { data, error } = useLedgerQuery(api.getOpenSalesOrders);
  const stock = new Map((data?.items ?? []).map((i) => [i.productServiceId, i]));
  const rows = groupBy(data?.lines ?? [], (l) => l.productServiceId ?? `d:${l.description}`, itemLabel).flatMap((g) => {
    const item = stock.get(g.lines[0].productServiceId);
    const tracked = item && item.onHandE4 !== null;
    return [
      section(tracked ? `${g.label} · on hand ${formatQuantity(item.onHandE4)} · on sales orders ${formatQuantity(item.onSalesOrderE4)} · available ${formatQuantity(item.availableE4)}` : g.label),
      ...g.lines.map((l) => ({ cells: lineCells(l, (x) => x.customerName) })),
      totalRow(`Total ${g.label}`, sum(g.lines, 'openCents'), 'total', sum(g.lines, 'leftQtyE4')),
    ];
  });
  if (data) rows.push(totalRow('Total', data.totalOpenCents, 'grand'));
  return (
    <TableReport
      title="Open Sales Orders by Item"
      subtitle={asOf()}
      columns={columns('Item', 'Customer')}
      rows={rows}
      error={error}
      footer="Available: on hand less what is left on open sales orders; below zero, that much is backordered. Stock leaves only when an order is invoiced."
      onCsvRows={() => (data?.lines ?? []).map((l) => csvLine(itemLabel(l), l, (x) => x.customerName))}
    />
  );
}

/** Pending Sales: what customers have ordered and not yet been invoiced for. */
export function PendingSalesReport() {
  const { data, error } = useLedgerQuery(api.getPendingSales);
  const rows = (data?.rows ?? []).map((r) => ({ cells: [r.customerName, String(r.orders), formatCents(r.openCents)] }));
  if (data) rows.push({ cells: ['Total', '', formatCents(data.totalCents)], kind: 'grand' });
  return (
    <TableReport
      title="Pending Sales"
      subtitle={asOf()}
      columns={[{ label: 'Customer' }, { label: 'Open orders', align: 'right' }, { label: 'Not yet invoiced', align: 'right' }]}
      rows={rows}
      error={error}
      footer="Sales agreed on sales orders and not yet invoiced, before tax. Nothing here has posted; each sale posts when its invoice does."
      onCsvRows={() => (data?.rows ?? []).map((r) => [r.customerName, String(r.orders), plainAmount(r.openCents)])}
    />
  );
}
