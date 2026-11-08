// The Dashboard's Home tab: the day-to-day forms drawn as a map of the order the work goes in
// (an estimate becomes an invoice, the invoice is paid, the payment is deposited). Each panel is a
// small grid; `col` and `row` place a step, `flows` join two steps with an arrow. Pure data and
// geometry, so it is tested without React (homeFlow.test.js).
//
// Every step opens a screen the side bar already has (`view`), and with `action` the form the
// New menu opens there (QuickCreateModal's ids). `when` names what must be switched on.

/** Size of one grid cell, and of the icon tile drawn at the top of it, in pixels. */
export const CELL = { w: 108, h: 84 };
export const TILE = 40;
const TILE_TOP = 6;

export const PANELS = [
  {
    id: 'customers',
    title: 'Customers',
    cols: 4,
    rows: 3,
    steps: [
      { id: 'estimates', label: 'Estimates', icon: 'HomeEstimate', view: 'sales/estimates', action: 'estimate', col: 0, row: 0, when: 'estimates' },
      { id: 'invoices', label: 'Invoices', icon: 'HomeInvoice', view: 'sales/invoices', action: 'invoice', col: 1, row: 0, badge: 'overdueInvoices' },
      { id: 'receive-payments', label: 'Receive Payments', icon: 'ReceivePayment', view: 'sales/invoices', action: 'receive-payment', col: 2, row: 0 },
      { id: 'deposits', label: 'Make Deposits', icon: 'HomeDeposit', view: 'banking/transfers', action: 'bank-deposit', col: 3, row: 0, badge: 'toDeposit' },
      { id: 'billable', label: 'Billable Time & Costs', icon: 'HomeBillable', view: 'sales/bill-costs', col: 0, row: 1 },
      { id: 'sales-receipts', label: 'Sales Receipts', icon: 'HomeSalesReceipt', view: 'sales/all', action: 'sales-receipt', col: 2, row: 1 },
      { id: 'finance-charges', label: 'Finance Charges', icon: 'HomeFinanceCharge', view: 'sales/finance-charges', col: 0, row: 2 },
      { id: 'statements', label: 'Statements', icon: 'HomeStatement', view: 'sales/statements', col: 1, row: 2 },
      { id: 'credits', label: 'Credits & Refunds', icon: 'HomeRefund', view: 'sales/credits', action: 'credit-memo', col: 3, row: 2 },
    ],
    flows: [
      ['estimates', 'invoices'],
      ['invoices', 'receive-payments'],
      ['receive-payments', 'deposits'],
      ['billable', 'invoices'],
      ['sales-receipts', 'deposits'],
      ['finance-charges', 'statements'],
    ],
  },
  {
    id: 'vendors',
    title: 'Vendors',
    cols: 4,
    rows: 2,
    steps: [
      { id: 'purchase-orders', label: 'Purchase Orders', icon: 'HomePurchaseOrder', view: 'expenses/purchase-orders', action: 'purchase-order', col: 0, row: 0, when: 'inventory' },
      { id: 'bills', label: 'Enter Bills', icon: 'HomeBill', view: 'expenses/bills', action: 'bill', col: 1, row: 0 },
      { id: 'pay-bills', label: 'Pay Bills', icon: 'HomePayBills', view: 'expenses/bills', action: 'pay-bills', col: 2, row: 0, badge: 'billsDue' },
      { id: 'print-checks', label: 'Print Checks', icon: 'Print', view: 'expenses/print-checks', col: 3, row: 0 },
      { id: 'vendor-credits', label: 'Vendor Credits', icon: 'HomeVendorCredit', view: 'expenses/credits', action: 'vendor-credit', col: 1, row: 1 },
      { id: 'sales-tax', label: 'Sales Tax', icon: 'HomeSalesTax', view: 'sales/tax', col: 3, row: 1 },
    ],
    flows: [
      ['purchase-orders', 'bills'],
      ['bills', 'pay-bills'],
      ['pay-bills', 'print-checks'],
      ['vendor-credits', 'pay-bills'],
    ],
  },
  {
    id: 'employees',
    title: 'Employees',
    cols: 4,
    rows: 1,
    steps: [
      { id: 'time', label: 'Enter Time', icon: 'HomeTime', view: 'employees/time', action: 'time-activity', col: 0, row: 0 },
      { id: 'timesheet', label: 'Weekly Timesheet', icon: 'HomeTimesheet', view: 'employees/time', action: 'weekly-timesheet', col: 1, row: 0 },
      { id: 'mileage', label: 'Mileage', icon: 'HomeMileage', view: 'employees/mileage', action: 'mileage-trip', col: 2, row: 0 },
    ],
    flows: [],
  },
];

/** The side column: shortcuts with no order between them. */
export const SIDE_PANELS = [
  {
    id: 'banking',
    title: 'Banking',
    steps: [
      { id: 'write-checks', label: 'Write Checks', icon: 'PayCheck', view: 'expenses/expenses', action: 'check' },
      { id: 'expenses', label: 'Card & Cash Expenses', icon: 'PayCard', view: 'expenses/expenses', action: 'expense' },
      { id: 'transfer', label: 'Transfer Funds', icon: 'HomeTransfer', view: 'banking/transfers', action: 'transfer' },
      { id: 'bank-feeds', label: 'Bank Feeds', icon: 'HomeBankFeed', view: 'banking/feeds' },
      { id: 'reconcile', label: 'Reconcile', icon: 'HomeReconcile', view: 'banking/reconcile' },
      { id: 'registers', label: 'Registers', icon: 'HomeRegister', view: 'accounting/register' },
    ],
  },
  {
    id: 'company',
    title: 'Company',
    steps: [
      { id: 'accounts', label: 'Chart of Accounts', icon: 'HomeAccounts', view: 'accounting/chart-of-accounts' },
      { id: 'items', label: 'Products & Services', icon: 'HomeItems', view: 'sales/products' },
      { id: 'journal', label: 'Journal Entries', icon: 'Accounting', view: 'accounting/journal', action: 'journal-entry' },
      { id: 'todos', label: 'To Do List', icon: 'HomeTodo', view: 'accounting/todos', badge: 'todos' },
      { id: 'reports', label: 'Reports', icon: 'Reports', view: 'reports' },
    ],
  },
];

/**
 * The panels with only the steps the company uses: `on` says which `when` features are on. Flows
 * that lose an end are dropped, and a panel left empty is dropped.
 */
export function visiblePanels(panels, on) {
  return panels
    .map((p) => {
      const steps = p.steps.filter((s) => !s.when || on[s.when]);
      const ids = new Set(steps.map((s) => s.id));
      return { ...p, steps, flows: (p.flows ?? []).filter(([a, b]) => ids.has(a) && ids.has(b)) };
    })
    .filter((p) => p.steps.length > 0);
}

/** Centre of a step's icon tile. */
function tileCentre(step) {
  return { x: step.col * CELL.w + CELL.w / 2, y: step.row * CELL.h + TILE_TOP + TILE / 2 };
}

const GAP = 6;

/**
 * The SVG path of the arrow from step `a` to step `b`. Along a row it runs tile edge to tile edge;
 * otherwise it leaves `a` sideways at its own row and turns up or down into `b` at `b`'s column,
 * stopping short of `b`'s label (going up) or tile (going down).
 */
export function flowPath(a, b) {
  const from = tileCentre(a);
  const to = tileCentre(b);
  const half = TILE / 2 + GAP;
  if (a.row === b.row) {
    const dir = Math.sign(to.x - from.x);
    return `M${from.x + dir * half},${from.y} H${to.x - dir * half}`;
  }
  if (a.col === b.col) {
    const dir = Math.sign(to.y - from.y);
    const startY = dir < 0 ? from.y - half : from.y + CELL.h - TILE_TOP - TILE / 2 - GAP;
    const endY = dir < 0 ? b.row * CELL.h + CELL.h - GAP : to.y - half;
    return `M${from.x},${startY} V${endY}`;
  }
  const dir = Math.sign(to.x - from.x);
  const endY = b.row < a.row ? b.row * CELL.h + CELL.h - GAP : to.y - half;
  return `M${from.x + dir * half},${from.y} H${to.x} V${endY}`;
}

/** Account types listed under Balances, in order. */
export const BALANCE_TYPES = [
  ['BANK', 'Bank accounts'],
  ['CREDIT_CARD', 'Credit cards'],
  ['ACCOUNTS_RECEIVABLE', 'Owed to you'],
  ['ACCOUNTS_PAYABLE', 'You owe'],
];

/** The active accounts of each balance type, grouped, skipping types with none. */
export function balanceGroups(accounts) {
  return BALANCE_TYPES.map(([type, label]) => ({
    type,
    label,
    accounts: (accounts ?? []).filter((a) => a.accountType === type && a.isActive !== false),
  })).filter((g) => g.accounts.length > 0);
}

/** The counts shown on steps, from the reminders and the payments waiting to be deposited. */
export function badgeCounts(reminders, undeposited) {
  const of = (key) => (reminders ?? []).find((g) => g.key === key)?.items.length ?? 0;
  return {
    overdueInvoices: of('OVERDUE_INVOICES'),
    billsDue: of('BILLS_DUE'),
    todos: of('TODOS'),
    toDeposit: (undeposited ?? []).length,
  };
}
