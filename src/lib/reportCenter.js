// The report center's list: a one-line description of each report, search over names and
// descriptions, and the Favorites and Recent groups. Pure, so `node --test` covers it; Reports.jsx
// draws the list and keeps the favorites and recent reports on this computer.

/** What each built-in report shows, in a line. */
export const REPORT_DESCRIPTIONS = Object.freeze({
  pl: 'Income and expenses for a period, and the net income they leave.',
  plc: 'Profit and Loss split into a column for each class.',
  'pl-py': 'Profit and Loss beside the same period a year earlier, with the change.',
  bs: 'What the business owns and owes, and its equity, on one date.',
  'bs-py': 'Balance Sheet beside the same date a year earlier, with the change.',
  cf: 'Where cash came from and went: operations, investing and financing.',
  bva: 'Income and expenses against the budget for the period, line by line.',
  'bs-bva': 'Balance sheet accounts against their budgeted balances.',
  ar: 'What each customer owes, by how many days it is past due.',
  ap: 'What you owe each vendor, by how many days it is past due.',
  'sales-customer': 'Total sales to each customer for a period.',
  'sales-product': 'Quantity and sales of each product or service for a period.',
  'sales-rep': 'Sales credited to each sales rep for a period.',
  'item-price-list': 'Every item with its standard price and its price at each price level.',
  'sales-tax': 'Sales tax collected and owed to each agency.',
  'expenses-vendor': 'Total spent with each vendor for a period.',
  1099: 'Payments to each 1099 vendor, by the box they are reported in.',
  '1099-detail': 'The payments behind each 1099 vendor’s totals.',
  '1099-nec': 'The year’s 1099-NEC forms, what each still needs, the 1096 totals, and printing them.',
  'inv-val': 'Quantity on hand, average cost and value of each inventory item.',
  'inv-detail': 'Each purchase, sale and adjustment that moved an inventory item’s value.',
  'stock-status': 'On hand, on order and reorder point for each inventory item.',
  'missing-checks': 'Check numbers in a bank account, with gaps and repeats marked.',
  commented: 'Reports kept with comments on their figures, as they were when commented on.',
  snapshot: 'Widgets you choose: balances, who owes, bills to pay, the income and expense trend and your top customers, items and vendors.',
  tb: 'Every account’s debit or credit balance on one date; the columns should agree.',
  gl: 'Every posted line, account by account, with running balances.',
  'prev-rec': 'A past bank reconciliation, as it was finished.',
  'rec-discrepancy': 'Cleared transactions changed or removed after a reconciliation.',
  'income-tax-summary': 'Totals for each tax form line the accounts are mapped to.',
  'income-tax-detail': 'The transactions behind each tax form line.',
  collections: 'Overdue invoices by customer, with contact details for following up.',
  'open-pos': 'Purchase orders with items still to be received.',
  'job-profit': 'Income, costs and profit for each job.',
  'job-profit-detail': 'The income and cost lines behind each job’s profit.',
  'job-est': 'Each job’s estimated cost and revenue against what actually happened.',
  'job-est-detail': 'Estimates against actuals for each job, item by item.',
  'item-profit': 'Revenue, cost and margin for each item across jobs.',
  'item-est': 'Estimated against actual quantities and amounts for each item.',
  'pl-job': 'Profit and Loss with a column for each job.',
  'est-by-job': 'Every estimate, grouped by the job it is for.',
  progress: 'How much of each estimate has been invoiced so far.',
  'mileage-vehicle': 'Miles driven and their value for each vehicle.',
  'mileage-vehicle-detail': 'Each trip logged for each vehicle.',
  'mileage-job': 'Miles driven for each job, and what can be billed.',
  'mileage-job-detail': 'Each trip logged against each job.',
  'graph-ie': 'Income and expenses by month as bars, with the biggest categories.',
  'graph-nw': 'Assets, liabilities and net worth over time.',
  'graph-ar': 'What customers owe, by age and by customer.',
  'graph-ap': 'What you owe vendors, by age and by vendor.',
  'graph-sales': 'Sales by month, by item and by customer.',
  'graph-bva': 'Budget against actual by month as bars.',
  'process-multiple': 'Run several memorized reports at once, for the same dates.',
});

/** A description for any report in the list, including lists, presets and memorized reports. */
export function describeReport(id, name = '') {
  if (REPORT_DESCRIPTIONS[id]) return REPORT_DESCRIPTIONS[id];
  if (id.startsWith('list:')) return `The ${name || 'list'} from your company file, as a report you can print or export.`;
  if (id.startsWith('saved:')) return 'A report you memorized, with the columns, filters and dates you chose.';
  if (id.startsWith('preset:')) {
    if (/detail/i.test(name)) return 'Every transaction behind the totals; customize its columns and filters.';
    if (/summary/i.test(name)) return 'Totals for a period; double-click a row to see the transactions behind it.';
    if (/list/i.test(name)) return 'Each transaction of this kind in a period, one per line.';
    return 'A transaction report you can customize, filter and memorize.';
  }
  return '';
}

/** Lower case with runs of spaces made one, and "&" read as "and", so "a/r aging" or "p & l" match. */
const fold = (text) =>
  String(text ?? '')
    .toLowerCase()
    .replace(/&/g, ' and ')
    .replace(/\s+/g, ' ')
    .trim();

/**
 * The groups narrowed to reports whose name or description holds every word typed (in any
 * order). Groups left empty are dropped. An empty query keeps everything.
 */
export function searchReports(groups, query) {
  const words = fold(query).split(' ').filter(Boolean);
  if (words.length === 0) return groups;
  return groups
    .map((g) => ({
      ...g,
      reports: g.reports.filter(([id, name]) => {
        const text = `${fold(name)} ${fold(describeReport(id, name))} ${fold(g.title)}`;
        return words.every((w) => text.includes(w));
      }),
    }))
    .filter((g) => g.reports.length > 0);
}

/** Stars or unstars a report: removing keeps the others' order; a new favorite goes last. */
export function toggleFavorite(favorites, id) {
  return favorites.includes(id) ? favorites.filter((f) => f !== id) : [...favorites, id];
}

/** The reports run most recently, newest first, without repeats, at most `max`. */
export function pushRecent(recent, id, max = 8) {
  return [id, ...recent.filter((r) => r !== id)].slice(0, max);
}

/**
 * The list with Favorites and Recent groups on top, holding only reports still in the list (a
 * deleted memorized report or a hidden 1099 report drops out). Empty groups are left off.
 */
export function withFavoritesAndRecent(groups, favorites, recent) {
  const known = new Map(groups.flatMap((g) => g.reports).map((r) => [r[0], r]));
  const pick = (ids) => ids.map((id) => known.get(id)).filter(Boolean);
  const top = [
    { title: 'Favorites', reports: pick(favorites), kind: 'favorites' },
    { title: 'Recent', reports: pick(recent), kind: 'recent' },
  ].filter((g) => g.reports.length > 0);
  return [...top, ...groups];
}

/** The reports whose dates the center can set before they run, and how each takes them. */
export const DATED_REPORTS = Object.freeze({
  pl: 'range',
  bs: 'as-of',
  ar: 'as-of',
  ap: 'as-of',
  'sales-customer': 'range',
  'sales-product': 'range',
  'sales-rep': 'range',
});

/** The report's opening options for the dates chosen in the center: a range, or its end as the as-of date. */
export function datesFor(id, start, end) {
  const kind = DATED_REPORTS[id];
  if (!kind || !start || !end || start > end) return null;
  return kind === 'as-of' ? { asOf: end } : { startDate: start, endDate: end };
}

/** The report center's tools, which open screens of their own rather than a report. */
export const CENTER_TOOLS = ['process-multiple', 'scheduled-reports'];

/** Whether a report center entry gets a preview on hover: every report, not the tools. */
export const previewable = (id) => typeof id === 'string' && id !== '' && !CENTER_TOOLS.includes(id);
