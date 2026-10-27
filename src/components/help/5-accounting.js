// Help, part 5: topics on Accounting & audit (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'accounting/register': {
    help: 'Every transaction in one account, in date order, with a running balance, the other account, and whether it is cleared or reconciled. Click a row to see the whole transaction.',
  },
  'accounting/chart-of-accounts': {
    help: 'Every account in your books, grouped into assets, liabilities, equity, income and expenses, with subaccounts under their parents. Add, rename, renumber or inactivate accounts here.',
  },
  'accounting/classes': {
    help: 'Classes tag income and expenses by department, location or line of business, separately from your accounts. Add classes here, choose one on each line, then run Profit and Loss by Class.',
  },
  'accounting/reclassify': {
    help: 'Move posted income and expense lines to another class. Each move posts a reclassifying journal entry on the line’s own date, which you can void to undo it.',
  },
  'accounting/clean-up': {
    help: 'Client Data Review tools in one place: apply credits left on credit memos and vendor credits, clear payments stuck in Undeposited Funds, write off old invoices, review list changes, and compare the Balance Sheet with the inventory valuation. Every fix shows its totals and asks first.',
    guide: 'cleaning-up-books',
  },
  'accounting/journal': {
    help: 'Every posted entry in date order, including voids and their reversals. Post a manual journal entry here for adjustments.',
    guide: 'journal-entries-and-voids',
  },
  'accounting/find': {
    help: 'Find posted transactions by text, dates, type, customer or vendor, account, amount and number together, like Edit › Find in desktop accounting software. Amounts are each transaction’s total. Click a row to open it.',
    guide: 'journal-entries-and-voids',
  },
  'accounting/recurring': {
    help: 'Memorized transactions that repeat on a schedule: rent, subscriptions, monthly invoices and accruals. Enter everything that is due in one step, or pause a schedule.',
  },
  'accounting/todos': {
    help: 'Your own tasks, with due dates, about a customer, vendor or employee and optionally one of their invoices or bills. Tick Done to complete one; due ones also show in Reminders on the Dashboard.',
    guide: 'reminders-and-todos',
  },
  'accounting/budgets': {
    help: 'Expected income and expenses for each account, month by month for a fiscal year. Compare a budget with your books in Reports › Budget vs. Actuals.',
  },
  'accounting/audit': {
    help: 'A history of every posting, void and reconciliation that cannot be edited, and a check that every posted entry still matches what was recorded.',
    guide: 'ledger-integrity',
  },
  'accounting/fx-revaluation': {
    help: 'Restate open foreign-currency invoices at a period-end rate. The unrealized gain or loss is posted on that date and reversed the next day.',
    guide: 'currency-revaluation',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'reminders', area: 'company', views: ['dashboard', 'accounting/todos'],
    title: 'See what needs attention: Reminders',
    body: 'The Reminders panel on the Dashboard counts overdue invoices, bills due soon, to-dos due, recurring transactions to enter, forms to email, upcoming tax dates and an overdue backup; click a line to see its items. Customize chooses which show, how many days each looks ahead, and whether Reminders open with the company; the choices are each user’s own.',
    keywords: 'reminders alerts due overdue bills to pay invoices past due to do tax dates backup startup customize look ahead',
    go: { label: 'Go to the Dashboard', view: 'dashboard' },
    guide: 'reminders-and-todos',
  },
  {
    id: 'todos', area: 'company', views: ['accounting/todos', 'sales/customers', 'expenses/vendors'],
    title: 'Keep a To Do list',
    body: 'Add a to-do from + New › To Do, the To Do List, or New to-do… on a customer or vendor, with a due date, a note, who or which invoice or bill it is about, and who it is assigned to when users are on. Filter the list by status, due date and assignee, and tick Done to complete one; a View-only user can complete only their own.',
    keywords: 'to do todo task list follow up call assign due date note done complete',
    go: { label: 'Go to the To Do List', view: 'accounting/todos', action: 'todo' },
    guide: 'reminders-and-todos',
  },
  {
    id: 'register', area: 'accounting', views: ['accounting/register', 'accounting/chart-of-accounts', 'reports'],
    title: 'Open an account register',
    body: 'Accounting › Account Registers shows every transaction in one account with a running balance; double-clicking an account in the Chart of Accounts or clicking an account on a report opens it too. Click a row to see the whole transaction, or void it from the ▾ menu.',
    keywords: 'register account register ledger running balance transactions drill down quickreport',
    go: { label: 'Go to Account Registers', view: 'accounting/register' },
  },
  {
    id: 'chart-of-accounts', area: 'accounting', views: ['accounting/chart-of-accounts', 'accounting/journal', 'dashboard'],
    title: 'Find and edit the chart of accounts',
    body: 'Accounting › Chart of Accounts lists every account by type, with subaccounts indented under their parent and balances for asset, liability and equity accounts. Use New account to add one, double-click an account to rename or renumber it, and make unused accounts inactive so they leave the pickers while their history stays.',
    keywords: 'chart of accounts coa account list new account subaccount rename number inactive category ledger accounts',
    go: { label: 'Go to Chart of Accounts', view: 'accounting/chart-of-accounts' },
  },
  {
    id: 'classes', area: 'accounting', views: ['accounting/classes', 'sales/invoices', 'expenses/bills', 'expenses/expenses', 'accounting/journal', 'reports'],
    title: 'Track departments or locations with classes',
    body: 'Turn on Use class tracking under Accounting › Classes (or Settings › Class tracking) and add classes and subclasses; a Class column then appears on invoices, sales receipts, estimates, bills, expenses, checks, deposits, bank feed approvals, time and journal entries. Settings can assign one class to a whole transaction instead, or warn before saving lines without a class, and Reports › Profit and Loss by Class shows each class side by side with parent totals.',
    keywords: 'class classes class tracking department location division region segment profit center',
    go: { label: 'Go to Classes', view: 'accounting/classes' },
  },
  {
    id: 'reclassify', area: 'accounting', views: ['accounting/reclassify', 'accounting/classes', 'reports'],
    title: 'Fix the class on posted transactions',
    body: 'Accounting › Reclassify Transactions lists posted income and expense lines by date, account and current class; tick lines, choose the new class and choose Reclassify. Each move posts a reclassifying journal entry on the line’s own date, so reports are corrected for that period, and voiding that entry undoes it.',
    keywords: 'reclassify wrong class change class fix class move class',
    go: { label: 'Go to Reclassify Transactions', view: 'accounting/reclassify' },
  },
  {
    id: 'clean-up-books', area: 'accounting', views: ['accounting/clean-up', 'banking/transfers', 'sales/credits', 'expenses/credits', 'accounting/audit'],
    title: 'Clean up the books before closing a year',
    body: 'Accounting › Clean Up Books has five tools. Unapplied credits applies credit left on credit memos and vendor credits to open invoices and bills, oldest first. Undeposited Funds matches a payment stuck there to the deposit that already banked it, which removes the income counted twice. Write off invoices credits old balances to a bad debt or discount account. List changes shows accounts, items, customers and vendors added, renamed, edited or made inactive in a date range. Inventory compares the inventory asset accounts with the Inventory Valuation and lists the transactions that cause any difference.',
    keywords: 'client data review cleanup clean up unapplied credits undeposited funds double deposit write off bad debt list changes inventory valuation balance sheet accountant year end',
    go: { label: 'Go to Clean Up Books', view: 'accounting/clean-up' },
    guide: 'cleaning-up-books',
  },
  {
    id: 'journal-entry', area: 'accounting', views: ['accounting/journal'],
    title: 'Post a manual journal entry',
    body: 'Choose New journal entry and enter at least two lines whose debits equal credits; LocalBooks will not post an entry that does not balance. Use it for adjustments such as depreciation or opening balances.',
    keywords: 'journal entry adjust adjustment debit credit manual opening balance',
    go: { label: 'New journal entry', view: 'accounting/journal', action: 'journal-entry' },
    guide: 'journal-entries-and-voids',
  },
  {
    id: 'void', area: 'accounting', views: ['accounting/journal', 'sales/invoices', 'sales/all', 'expenses/expenses', 'expenses/bills', 'banking/transfers'],
    title: 'Undo a transaction (void)',
    body: 'Posted transactions are never deleted: Void Transaction (in the row’s menu) posts an exact reversing entry, and both stay visible in Journal Entries. Reconciled transactions and anything dated on or before the closing date cannot be voided.',
    keywords: 'delete undo reverse mistake cancel void correction',
    go: { label: 'Go to Journal Entries', view: 'accounting/journal' },
    guide: 'journal-entries-and-voids',
  },
  {
    id: 'ledger-discrepancies', area: 'accounting', views: ['accounting/audit'],
    title: 'Check and correct the ledger',
    body: 'Verify ledger integrity compares every posted entry with the snapshot taken when it was posted and lists anything changed, deleted or out of balance. Review corrections proposes a balanced correcting entry where one can fix the problem and posts only after you approve; a change that left debits and credits unequal needs a restore from a backup.',
    keywords: 'audit tamper heal auto-heal mismatch integrity forensic snapshot corrupt verify',
    go: { label: 'Go to the Audit Log', view: 'accounting/audit' },
    guide: 'ledger-integrity',
  },
  {
    id: 'audit-log', area: 'accounting', views: ['accounting/audit'],
    title: 'Read the audit log',
    body: 'Every posting, void and reconciliation lock is recorded automatically by the database, with its time, and cannot be edited or deleted. Filter by event type to see just posts, voids or reconciliations.',
    keywords: 'audit history trail log events',
    go: { label: 'Go to the Audit Log', view: 'accounting/audit' },
    guide: 'ledger-integrity',
  },
  {
    id: 'fx-revaluation', area: 'accounting', views: ['accounting/fx-revaluation', 'sales/invoices', 'reports'],
    title: 'Revalue foreign-currency invoices at period end',
    body: 'Enter period-end rates under Accounting › Currency Revaluation to restate what customers owe in other currencies. The unrealized gain or loss posts on that date and reverses the next day, so payments still realize the right gain or loss.',
    keywords: 'currency revaluation unrealized exchange gain loss foreign period end rate',
    go: { label: 'Go to Currency Revaluation', view: 'accounting/fx-revaluation' },
    guide: 'currency-revaluation',
  },
];
