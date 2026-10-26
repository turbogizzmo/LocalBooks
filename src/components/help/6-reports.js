// Help, part 6: topics on Reports (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  reports: {
    help: 'Every standard report: Profit and Loss (accrual or cash, by month, quarter or class, compared with an earlier period), Balance Sheet, previous year comparisons, budget vs. actual, Cash Flows, A/R and A/P aging, open invoices, unpaid bills, sales and expense summaries, inventory valuation and stock status, missing checks, Trial Balance and General Ledger. Click an account to open its register; export to CSV, print, or save as PDF.',
    guide: 'reports',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'profit-loss', area: 'reports', views: ['reports', 'dashboard'],
    title: 'Run a Profit and Loss',
    body: 'Pick a period such as This month, This fiscal quarter or Last fiscal year, or a custom date range, to see income, cost of goods sold, expenses and net income. Fiscal periods follow the fiscal year start set in Settings.',
    keywords: 'profit loss income statement p&l net income revenue by class department location',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'cash-basis', area: 'reports', views: ['reports'],
    title: 'See profit on a cash basis, by month, or against last year',
    body: 'On the Profit and Loss, set Basis to Cash to count invoices and bills only as they are paid, Columns to Months or Quarters to split the period, or Compare with to show the previous period or previous year beside it with the change in dollars and percent.',
    keywords: 'cash basis accrual monthly quarterly compare previous year prior period comparison columns',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'aging', area: 'reports', views: ['reports', 'sales/invoices', 'expenses/bills'],
    title: 'See who owes you and what you owe',
    body: 'Reports › A/R Aging Summary and A/P Aging Summary show open balances by customer or vendor in current, 1–30, 31–60, 61–90 and 91+ days past due as of any date; Open Invoices and Unpaid Bills list each open item. Credit not yet used shows as a negative amount.',
    keywords: 'aging ar aging ap aging open invoices unpaid bills receivables payables past due overdue customer balance vendor balance',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'accountant-reports', area: 'reports', views: ['reports'],
    title: 'Trial Balance, General Ledger and Cash Flows',
    body: 'Under For my accountant, the Trial Balance shows every account’s debit or credit balance as of a date and the General Ledger every transaction by account with running balances; the Statement of Cash Flows explains the change in your bank balances by operating, investing and financing activity.',
    keywords: 'trial balance general ledger gl cash flow statement of cash flows accountant tax',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'balance-sheet', area: 'reports', views: ['reports', 'dashboard'],
    title: 'Run a Balance Sheet',
    body: 'Choose an as-of date to see assets, liabilities and equity at that moment. Profit from past fiscal years appears as Retained Earnings and this year’s as Net Income.',
    keywords: 'balance sheet assets liabilities equity retained earnings',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'prev-year', area: 'reports', views: ['reports'],
    title: 'Compare this year with last year',
    body: 'Reports › Profit and Loss Prev Year Comparison puts a period beside the same dates a year earlier; Balance Sheet Prev Year Comparison does the same for an as-of date. Both show the change in dollars and percent.',
    keywords: 'previous year comparison prior year last year compare change percent year over year',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'inventory-reports', area: 'reports', views: ['reports', 'sales/products'],
    title: 'Value inventory and check stock levels',
    body: 'Inventory Valuation Summary shows each tracked item’s quantity, average cost and value as of a date, and Inventory Valuation Detail every movement in a period. Inventory Stock Status by Item adds what is on order and how fast each item sells.',
    keywords: 'inventory valuation summary detail stock status on hand average cost asset value reorder purchase order',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'missing-checks', area: 'reports', views: ['reports', 'expenses/expenses'],
    title: 'Find missing or duplicate check numbers',
    body: 'Reports › Missing Checks lists the checks written from a bank account in number order and marks numbers that were skipped or used twice. Voided checks keep their number.',
    keywords: 'missing checks check numbers gap skipped duplicate void check register',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'export', area: 'reports', views: ['reports'],
    title: 'Export, print or save a report as PDF',
    body: 'Use Export on a report to save it as a CSV file for a spreadsheet, or to print it. Choose Save as PDF in the print dialog to keep a copy with your company logo.',
    keywords: 'export csv excel print pdf save share',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'trial-balance', area: 'reports', views: ['dashboard'],
    title: 'Read the trial balance',
    body: 'The Dashboard’s trial balance lists every account with a balance and shows ✓ In balance when total debits equal total credits. Because every entry must balance to post, it always should.',
    keywords: 'trial balance dashboard debits credits in balance',
    go: { label: 'Go to the Dashboard', view: 'dashboard' },
    guide: 'reports',
  },
];
