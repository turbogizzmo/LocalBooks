// Help, part 6: topics on Reports (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  reports: {
    help: 'Every standard report: Profit and Loss (accrual or cash, by month, quarter or class, compared with an earlier period), Balance Sheet, previous year comparisons, budget vs. actual, Cash Flows, A/R and A/P aging, open invoices, unpaid bills, sales and expense summaries, job costing (job and item profitability, estimates vs. actuals, Profit and Loss by Job), detail reports you can customize and memorize, inventory valuation and stock status, missing checks, and under Accountant & Taxes the Trial Balance, General Ledger, adjusting entries, past reconciliations, income tax by tax line, collections and open purchase orders. Click an account to open its register; export to CSV, print, or save as PDF.',
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
    body: 'Reports › A/R Aging Summary and A/P Aging Summary show open balances by customer or vendor in current, 1–30, 31–60, 61–90 and 91+ days past due as of any date; A/R and A/P Aging Detail, Open Invoices and Unpaid Bills list each open item, and Customer and Vendor Balance Summary and Detail show what makes up each balance. Credit not yet used shows as a negative amount.',
    keywords: 'aging ar aging ap aging open invoices unpaid bills receivables payables past due overdue customer balance vendor balance',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'custom-reports', area: 'reports', views: ['reports'],
    title: 'Customize a report and memorize it',
    body: 'On any detail report (P&L Detail, Sales by Customer Detail, Check Detail, Journal and the others), Customize… chooses the columns, the subtotals and the filters: accounts, names, transaction types, class, product or service, paid or cleared status, an amount range and memo text; Summary turns it into one row per group, optionally by month, quarter or year. Save as… keeps it under Memorized reports to run again, and a click on a line opens its transaction.',
    keywords: 'custom report transaction detail customize filter columns memorize memorized saved report p&l detail balance sheet detail sales by customer detail purchases by vendor check detail deposit detail journal transaction list 1099 detail',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'job-reports', area: 'reports', views: ['reports', 'expenses/bills', 'expenses/expenses', 'sales/estimates'],
    title: 'See what each job earns and costs',
    body: 'Reports › Jobs shows actual cost, revenue and profit for each customer and job, by product/service or account, estimates against actual revenue, Profit and Loss by Job, Estimates by Job and how much of each estimate has been invoiced; click a row to open what is behind it. Costs count for a job when you choose it in the Customer:Job column of a bill, expense, check or vendor credit line; time is not costed.',
    keywords: 'job costing job profitability summary detail estimates vs actuals item profitability profit and loss by job customer job progress invoices estimates by job billable cost',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'accountant-reports', area: 'reports', views: ['reports'],
    title: 'Trial Balance, General Ledger and Cash Flows',
    body: 'Under Accountant & Taxes, the Trial Balance shows every account’s debit or credit balance as of a date and the General Ledger every transaction by account with running balances; the Statement of Cash Flows explains the change in your bank balances by operating, investing and financing activity.',
    keywords: 'trial balance general ledger gl cash flow statement of cash flows accountant tax',
    go: { label: 'Go to Reports', view: 'reports' },
  },
  {
    id: 'adjusting-entries', area: 'reports', views: ['reports', 'accounting/journal'],
    title: 'Mark and list adjusting journal entries',
    body: 'Tick Adjusting entry when you post a journal entry for a period-end adjustment such as depreciation or an accrual; the Journal Entries list tags it Adjusting and can show only those. Reports › Accountant & Taxes › Adjusting Journal Entries lists them with their lines, including the entry that voids one.',
    keywords: 'adjusting journal entries aje adjustment accountant year end depreciation accrual flag mark',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'previous-reconciliation', area: 'reports', views: ['reports', 'banking/reconcile'],
    title: 'Reprint a past reconciliation, or find changes made after it',
    body: 'Reports › Accountant & Taxes › Previous Reconciliation reprints any completed reconciliation as a Detail (each cleared transaction, and those outstanding or cleared on a later statement) or a Summary. Reconciliation Discrepancy lists cleared transactions changed, voided or deleted afterwards; reconciliations finished with an earlier version are checked by their totals only.',
    keywords: 'previous reconciliation report reconciliation detail summary discrepancy reprint cleared outstanding uncleared bank statement changed after reconciled',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reconciling',
  },
  {
    id: 'income-tax-reports', area: 'reports', views: ['reports'],
    title: 'Total your books by income tax line',
    body: 'On Income Tax Summary, click Tax lines… to give each account the line of your Schedule C, 1065, 1120-S or 1120 its amounts go on; the report then totals each line for the tax year, and Income Tax Detail lists the transactions behind them. Accounts with activity but no line are listed under Unassigned, and lines brought in from QuickBooks that are not matched yet under Other tax lines.',
    keywords: 'income tax summary detail tax line mapping schedule c 1065 1120s 1120 s corporation partnership return preparer taxline',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
  },
  {
    id: 'collections-report', area: 'reports', views: ['reports', 'sales/invoices'],
    title: 'Follow up on overdue invoices',
    body: 'Reports › Accountant & Taxes › Collections Report lists every invoice past due on a date, by customer, with the customer’s company name, phone and email and how many days late each one is. Open Purchase Orders, in the same group, lists the purchase orders not yet billed or closed, by vendor.',
    keywords: 'collections report overdue past due late invoices phone email contact open purchase orders po',
    go: { label: 'Go to Reports', view: 'reports' },
    guide: 'reports',
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
