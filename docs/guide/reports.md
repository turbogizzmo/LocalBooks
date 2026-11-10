# Run, export and print reports

## Find a report in the report center

The list on the left of **Reports** has every report, grouped by what it is about, with a line
under each name saying what it shows.

- **Preview**: rest the pointer on a report's name (or move to it with Tab) and a small picture of
  it appears beside the list: the report itself, from your books, as it would open. It shows what
  the report looks like before you open it; nothing in the picture can be clicked. A report with
  nothing to show yet in your books shows its empty layout.
- **Find a report**: type part of a name or of what a report shows (`aging`, `cash`, `who owes`);
  the list narrows to the reports with every word you typed.
- **Favorites**: point at a report and click its ☆ to star it; starred reports are listed under
  **Favorites** at the top. Click ★ to take one off.
- **Recent**: the last eight reports you opened, newest first.
- **Open reports on these dates**: tick it and choose a period (or From and To) above the list.
  Profit and Loss, the sales summaries and the other reports that cover a period open on those
  dates; the Balance Sheet and the A/R and A/P aging open as of the To date. Other reports open as
  usual.

Favorites, Recent and the date choice are kept on this computer.

## Email a report, list or register as a PDF

Every report, list and register with an **Export** menu has **Email as PDF…** and **Save as PDF…**.
The PDF holds what the Excel export would: the report's heading, columns and rows, page after
page, with the column headings repeated on each page.

**Email as PDF…** opens a message with the PDF attached. When sending from LocalBooks is set up
(**Settings › Email**), **Send** sends it from there; otherwise, or if you choose **Open in my mail
app instead**, a new message opens in your mail app for you to check and send. The **Reminders**
list has **Email as PDF…** too.

To password-protect the PDF, tick **Password-protect the PDF** and type a password. The PDF then
opens only with that password (it is encrypted with 256-bit AES, which every current PDF reader
opens). LocalBooks doesn't keep the password, so give it to the recipient another way, for example
by phone, never in the same email.

## Scheduled reports

**Reports › Scheduled Reports…** (under Memorized reports) sends chosen reports on a schedule, for
example the month's Profit and Loss and Balance Sheet to your accountant on the first of every
month.

1. Click **New schedule** and name it.
2. Add the reports: Profit and Loss, Profit and Loss by Class, the Balance Sheet, any report from
   the report builder, or a memorized report. For each, choose the dates it covers, counted from
   the day it runs: **Last month**, **Quarter to date**, **Last year** and so on. A report as of
   one date (the Balance Sheet, aging) is as of the last day.
3. Choose **How often** (every day, week, month or quarter), the day it **Starts**, and if you
   like the day it **Ends**.
4. Choose **Email them**, with the addresses to send to, or **Save them in a folder** on this
   computer, and whether they go **As** PDF or Excel spreadsheets.

Email goes only through the email server set up in **Settings › Email** (it must be turned on),
and only to the addresses on the schedule. **Run now** on a schedule sends it for today without
changing its next run; the list shows when each runs next and how its last run went.

Schedules run only while LocalBooks is open: it checks shortly after it starts and every few
minutes after that. If a run fell due while LocalBooks was closed, it happens once, soon after
LocalBooks next opens, for the latest date that was due; earlier missed dates are not sent one by
one.

### Password-protect scheduled PDFs

When a schedule sends or saves PDFs, tick **Password-protect the PDFs** and type the password they
should open with. The password is kept in this computer's credential store (Keychain on a Mac,
Credential Manager on Windows), never in the company file, so a copy or backup of the company file
doesn't carry it. To change it, open the schedule and type a new one; leave the box empty to keep
the saved one. If the schedule is run on another computer, the password must be entered there
again: until then its runs stop and say why.

## Company Snapshot

**Reports › Company Snapshot** (also **Reports › Company & Financial › Company Snapshot**) is a
page of widgets you choose, each a summary of one report:

| Widget | Shows | Same figures as |
|---|---|---|
| Account balances | Each bank and credit card account; the total is what's in the bank | Balance Sheet |
| Who owes you | The five customers owing most, with the overdue part, then Others | A/R Aging Summary |
| Vendors to pay | The five vendors you owe most, with the overdue part, then Others | A/P Aging Summary |
| Income and expense trend | Income and expense columns by month, and the net | Profit and Loss |
| Top customers | The five largest by income, then Others | Sales by Customer Summary |
| Top items | The five best-selling products and services, then Others | Sales by Product/Service Summary |
| Top vendors | The five you spent most with, then Others | Expenses by Vendor Summary |
| Income breakdown | The five largest income accounts, then Others; the total is all income | Profit and Loss |
| Expense breakdown | The five largest cost of sales and expense accounts, then Others | Profit and Loss |
| Year over year | Income, expenses and net income, beside the same dates a year earlier and the change | Profit and Loss for each period |
| To-dos | Your open to-dos (yours and unassigned ones, as your reminders show them) due by the period's end, and those with no date, soonest first; the total is how many | To Do Notes |
| Note | Text you type and keep on the page (it saves when you click away). Everyone who opens the company sees the same page and notes | — |

- **Add a widget…** puts it at the end of the page; **✕** removes it; **↑** and **↓** move it.
- Each widget has its own period: today (the balance widgets show balances as of the period's
  last day; "today" also means the year to date for the others), this or last month, quarter or
  fiscal year, or the last 12 months.
- **Open …** under a widget opens its report for the same dates, with every line.

The figures are worked out exactly as their reports work them out (the same calculation, dates and
basis), so a widget's total always matches the report's. The page is kept in the company file, so
everyone who opens it sees the same widgets.

## Money bars on Sales and Bills

The bar at the top of **Sales › All Sales** shows open invoices, overdue and paid in the last 30
days, then two more segments:

- **Unbilled**: billable costs and time not on an invoice yet, the same as **Sales › Bill Costs**.
- **Estimates**: estimates still pending or accepted and not turned into an invoice.

The bar on **Vendors › Bills** adds:

- **Purchase orders**: open purchase orders.
- **Card balances**: what each credit card account owes today, as on the Balance Sheet.

Click a segment to list its transactions under the bar; click it again to close the list. Click a
row (or press Enter on it) to open it: unbilled costs open Bill Costs on that customer, an estimate
or purchase order opens on its form, and a card opens its register.

## Profit and Loss

1. Click **Reports**. **Profit and Loss** is the first report in the list.
2. Pick a **Report period** (This month, Last month, This fiscal quarter, Fiscal year to date, This
   fiscal year, Last fiscal year), or **Custom** with **From** and **To** dates.

It shows income, cost of goods sold, gross profit, expenses and net income. Fiscal periods follow
**First month of fiscal year** in Settings.

## Balance Sheet

1. Click **Balance Sheet** in the list.
2. Set **As of** to the date you want.

Assets, liabilities and equity are shown at that date. Profit from earlier fiscal years appears as
Retained Earnings, and this year's as Net Income.

## Cash Flow Forecast

**Reports › Company & Financial › Cash Flow Forecast** (also under **Business overview**) shows,
week by week, the cash you have and the money your open invoices and bills will move:

- **Cash on hand**: your bank accounts' balance, plus payments received but not yet deposited, on
  the **From** date.
- **Money in**: each open invoice's balance in the week it's due (unused customer credit counts
  against it). **Money out**: each open bill's balance in the week it's due (less unused vendor
  credit). Click either row to see the documents.
- **Past due** (first column) is money already overdue, counted first; **Later** (last column) is
  due after the last week.
- **Net** and **Cash at the end** of each week follow.

Choose 4, 8, 13 or 26 **Weeks**. Its money in adds up to the A/R Aging Summary's total, and its
money out to the A/P Aging Summary's, as of the same date. The Dashboard's **Insights** tab shows
the next eight weeks as a chart; click it to open the report.

## See the transactions behind a number (QuickZoom)

Click an amount to see where it came from:

- **Profit and Loss, Balance Sheet and the other statements:** click an account or one of its
  amounts to open the account's register for the report's dates. In a report split by month or
  quarter, or a comparison, clicking an amount opens that column's dates.
- **A/R and A/P Aging Summary:** click a customer or vendor to list their open invoices, bills and
  credits under the name, each in its aging column with how many days past due it is. Click again
  to hide them.
- **Sales by Customer, Sales by Product/Service and Expenses by Vendor Summary:** click a line to
  open the matching detail report (Income by Customer Detail, Sales by Product/Service Detail or
  Expenses by Vendor Detail) for that name or item and the same dates. Click a transaction there to
  open it.

## QuickReport, Transaction History and Transaction Journal

- **QuickReport** (**Ctrl+Q**) shows everything about the thing you have selected, as a report you
  can customize, memorize and export like any other:
  - in an account register, or on an account in the Chart of Accounts: the account's lines with a
    running balance (for the register's dates);
  - on a customer or vendor in the Customers or Vendors list: every transaction with them;
  - on a product or service in Products & Services: every line it is on, with quantity and rate.

  It is also in each row's **Actions** menu and in **Reports › QuickReport**, which acts on what is
  selected in the list or register in front. A **QuickReport** button sits on registers and on a
  name's transactions panel.
- **Transaction History** (**Ctrl+H**) lists the transactions linked to the one you clicked in a
  register or a name's transactions panel: an invoice and the payments and credits applied to it, a
  payment and its invoice and deposit, a deposit and the payments in it, a bill and its bill
  payments and credits. Double-click one to open it. Checks, expenses, transfers and journal
  entries have nothing linked.
- **Transaction Journal** (**Ctrl+Y**) shows the debits and credits that transaction posted, in a
  Journal report of its own.

Both also work on the transaction open on a form: a saved invoice, bill, check, expense, card credit
or journal entry, and a saved sales receipt, credit memo, vendor credit, deposit or transfer. The
form's toolbar has **History** and **Journal** buttons for them; on a new, unsaved form they are off.
In the journal (**Accounting › Journal**), click an entry and press **Ctrl+H** or **Ctrl+Y**.

These keys use **Ctrl** on a Mac too, since ⌘Q quits and ⌘H hides the app.

## Previous year comparisons

- **Profit and Loss Prev Year Comparison** shows the period you choose beside the same dates one
  year earlier, with the change in dollars (**$ Change**) and percent (**% Change**). It is the
  Profit and Loss with **Compare with** set to **Previous year**, so the Basis and Class choices work
  here too.
- **Balance Sheet Prev Year Comparison** shows balances as of the date you choose beside the same
  date a year earlier, with the change in dollars and percent.

**% Change** is left blank where the earlier amount is zero.

## Budget vs. Actual

Set up a budget first under **Accounting › Budgets**:

1. Choose **New budget…**, name it, and pick the fiscal year.
2. With **Accounts** set to **Profit and Loss**, enter what you expect each income and expense
   account to bring in or cost each month. **Copy across** repeats the first month.
3. For a balance sheet budget, set **Accounts** to **Balance Sheet** and enter the **balance** you
   expect each account to have at the end of each month (not the change).
4. Choose **Save**.

Then, under **Reports › Budgets**:

- **Profit and Loss Budget vs. Actual** compares actual income and expenses with the budget for the
  months you choose.
- **Balance Sheet Budget vs. Actual** compares actual balances at the end of the month you choose
  with the budgeted balances. The budgeted **Net Income** is the budget's income less its expenses
  for the year to that month.

Both show **$ Over Budget** (actual less budget) and **% of Budget**.

## Inventory reports

These cover products you track under **Sales › Products & Services** (see
[Track inventory](inventory.md)). Costs are moving average costs.

- **Inventory Valuation Summary**: each item's quantity on hand, average cost, asset value and share
  of the total, and its value at the sales price (retail value), as of a date. The total agrees with
  your inventory asset accounts on that date.
- **Inventory Valuation Detail**: for a period, each item's starting quantity and value, then every
  purchase, sale, adjustment and void with the running quantity, average cost and value. Click a
  line to open its transaction.
- **Inventory Stock Status by Item**: quantity on hand at the end of the period, quantity on open
  purchase orders (**On PO**) with the earliest expected date (**Next deliv**), and units sold in
  the period and per week. Voided sales are not counted.

## Missing Checks

**Reports › Banking › Missing Checks** lists the checks written from a bank account in number order,
so you can spot gaps and mistakes.

1. Choose the **Bank account**.
2. Optionally set **From** and **To** dates. Leave them empty for all checks.

- A `*** Missing numbers ***` row marks numbers between two checks that no check used.
- `*** Duplicate number ***` marks a number used more than once.

Checks, bill payments, sales tax payments and customer refunds paid from the account are included
when their check number (or reference) is a whole number. A voided check keeps its number, so it is
not missing. One check that pays several bills from the same vendor counts once. Payments with
other references, such as `EFT`, are not listed; the count at the top tells you how many there are.

## Job costing reports

**Reports › Jobs** shows how much each customer and job has earned and cost, the way the Jobs, Time
& Mileage reports do in QuickBooks Desktop. A job is a customer under another customer
(`Customer:Job`, for example `Kestrel Homes:Lakeview Remodel`).

To charge a cost to a job, choose it in the **Customer:Job** column of a bill, expense, check, card
charge, card credit or vendor credit line. Sales count for the customer or job on the invoice, sales
receipt or credit memo.

| Report | Shows |
|---|---|
| **Job Profitability Summary** | For each customer and job: actual cost, actual revenue and the difference. A customer with jobs shows its own amounts as *(other)* and a total |
| **Job Profitability Detail** | The same for each job, broken down by product/service or by account (**Break down by**) |
| **Job Estimates vs. Actuals Summary** and **Detail** | Each job's estimated revenue against its actual revenue, with the difference in dollars and percent, and its actual cost; the detail is by product/service |
| **Item Profitability** | Actual cost, revenue and difference for each product or service |
| **Item Estimates vs. Actuals** | Estimated against actual revenue for each product or service |
| **Profit and Loss by Job** | The Profit and Loss with a column for each customer and job, like Profit and Loss by Class |
| **Estimates by Job** | Every estimate, grouped by customer and job, with its status and amount |
| **Progress Invoices vs. Estimates** | Each estimate's total, how much of it has been invoiced, and the share |

1. Choose the **Report period**. The job reports start at **All dates**; Profit and Loss by Job starts
   at **Fiscal year to date**.
2. Optionally choose one **Customer:Job** (with its jobs), or **No customer/job**.
3. Click a row to open the report beneath it: a job opens its detail, and a detail row opens the
   transactions behind it. **‹ Back** goes back up. Click a transaction to open it.

What counts:

- **Actual revenue** is income: invoices and sales receipts, less credit memos and discounts.
- **Actual cost** is expense and cost of goods sold: bill, expense, check and card lines charged to the
  job, less card credits and vendor credits, plus the cost of stock sold to the job. A bill line with
  a job and another without, on the same account, are shared out exactly.
- A customer payment's exchange gain or loss counts for that customer.
- Everything else (journal entries, deposits, lines with no customer or job) is in
  **No customer/job**, so every report adds up to the Profit and Loss for the same dates: the
  **TOTAL** difference is net income.
- **Time isn't costed.** A time activity has a billing rate but no cost rate, so hours don't add to a
  job's cost. Record what you pay for labor as a bill, check or expense line charged to the job.
- **Estimates have no cost**, so the estimate reports compare revenue only. Rejected estimates are
  left out; an estimate discount counts against the estimate's revenue.
- An estimate turns into one invoice for its whole amount, so **Progress Invoices vs. Estimates**
  shows each estimate as either invoiced in full (with the invoice number; click to open it) or not
  yet invoiced. An edited invoice is followed to its replacement; a voided one counts as not invoiced.
- Lines entered before job costing was added have no customer or job.

## Detail reports

These reports list transactions one line at a time, with subtotals. They are all built on the same
report engine, so each one can be customized (see below). Click a line to open its transaction, or
an account heading to open its register.

| Report | Shows |
|---|---|
| **Profit & Loss Detail** | Every income and expense line in the period, by account, with a running balance |
| **Net Income Detail** | The same lines with income positive and expenses negative (credits minus debits), so the TOTAL row is net income |
| **Profit & Loss by Month** | Each income and expense account with a column per month and a total. Income is positive, expenses negative, and the TOTAL row is net income |
| **Balance Sheet Detail** | Every asset, liability and equity line in the period, by account, from each account's beginning balance |
| **A/R Aging Detail** · **A/P Aging Detail** | Open invoices and credit memos (bills and vendor credits), grouped by how far past due they are |
| **Open Invoices** · **Unpaid Bills** | The same open items, grouped by customer or vendor |
| **Transaction List by Customer** · **Transaction List by Vendor** | One row per transaction that names a customer (or vendor), grouped by name, with its total |
| **Customer Balance Summary** and **Detail** · **Vendor Balance Summary** and **Detail** | What makes up each customer's receivable or each vendor's payable balance, as of a date |
| **Sales by Customer Detail** · **Sales by Product/Service Detail** | Each sale, by customer or by product or service, with quantity and rate |
| **Sales by Rep Detail** | Each product and service line of invoices and sales receipts, by the rep on the sale; the subtotals are the Sales by Rep Summary |
| **Income by Customer Summary** and **Detail** | Income less expenses for each customer (credits minus debits) |
| **Expenses by Vendor Detail** | Each expense line, by vendor |
| **Expenses by Vendor by Month** | Each vendor's expenses with a column per month; voided transactions are left out |
| **Purchases by Vendor Summary** and **Detail** · **Purchases by Product/Service Summary** and **Detail** | Product and service lines on bills, checks, expenses and vendor credits |
| **Check Detail** · **Deposit Detail** | Each check and bill payment (or deposit) with the lines it paid for, debits minus credits |
| **1099 Detail** | The checks, expenses and bill payments from bank accounts behind the 1099 Summary, by vendor, each split over the accounts it paid and the 1099 box each account is mapped to (Preferences › Tax: 1099) |
| **Transaction List by Date** | One row per transaction, with its total |
| **Transaction Detail by Account** | Every line in the period, by account, with debits, credits and a running balance from zero (the General Ledger starts from each account's beginning balance instead) |
| **Voided Transactions** | Each voided transaction and the entry that voids it. The total counts both, so it is twice what was voided |
| **Journal** | Every transaction with all of its debit and credit lines |
| **Adjusting Journal Entries** | Only the journal entries marked **Adjusting entry**, with all of their lines |

The numbers agree with the other reports: account lines and balances with the General Ledger, the
aging details with the A/R and A/P Aging Summary, the Sales by Customer and Product/Service Detail
with their summaries, the Sales by Rep Detail with its summary, the Net Income Detail with the
Profit and Loss, and the 1099 Detail with the 1099 Summary.

Things to know:

- **Open Balance** and **Aging** are figured as of the report's end date, after the payments and
  credits dated on or before it. Credit not yet used is negative.
- Voided transactions show with the entry that voids them, so together they come to zero. Choose
  **Leave out voided transactions** under **Customize…** to hide both (the 1099 Detail does).
- The name filters list customers and vendors. LocalBooks has no payroll, so no transaction names an
  employee.
- **Balance Sheet Detail** shows the equity accounts' own lines; this year's net income is not a
  line of its own, as it is on the Balance Sheet.
- On ledger lines, **Item**, **Qty** and **Rate** are filled in when the line was posted from a single
  item line of its invoice, sales receipt, credit memo or bill. The reports by product or service
  read the documents' item lines instead, in your home currency.

## Accountant & Taxes reports

The **Accountant & Taxes** group in the report list holds what an accountant asks for at year end:
the **Trial Balance**, **General Ledger**, **Transaction List by Date**, **Transaction Detail by
Account**, **Voided Transactions**, **Journal** and **Adjusting Journal Entries**, and the reports below. Each one exports and prints like any other,
and a click on a line opens its transaction.

### Adjusting Journal Entries

Tick **Adjusting entry** when you post a journal entry (see
[Journal entries and voiding](journal-entries-and-voids.md)) to mark it as a period-end adjustment,
such as depreciation, an accrual or a correction your accountant gives you. **Adjusting Journal
Entries** lists only those entries, opening on the fiscal year to date. When an adjusting entry is
voided, the entry that voids it is listed too, so together they come to zero. It is a detail report,
so **Customize…** and **Save as…** work on it.

### Previous Reconciliation

Reprints a completed reconciliation. Choose the **Account** and the **Statement ending** date, then
**Detail** or **Summary**:

- **Detail** lists the beginning balance, then each cleared transaction (checks and payments, then
  deposits and credits) with a running balance, the cleared balance, and the transactions dated on or
  before the statement that were not cleared. Their **Status** says whether a later statement
  cleared them (**Cleared** and its date) or they are still **Outstanding**. The last line is the
  register balance on the statement date.
- **Summary** shows the same thing as counts and totals, with the statement's ending balance.

For a credit card the two halves are **Charges and Cash Advances** and **Payments and Credits**.
See [Reconcile an account](reconciling.md).

### Reconciliation Discrepancy

Lists cleared transactions that were changed, voided or deleted after their reconciliation was
completed, by account and statement, with the amount when it was reconciled, the amount now and the
effect on the reconciled balance. Pick one **Account** or all of them.

LocalBooks itself never lets a reconciled transaction change, so in normal use this report is empty.
Anything it finds means the company file was changed outside LocalBooks (another program, or a file
restored from elsewhere). See [Reconcile an account](reconciling.md#check-for-changes-after-reconciling)
for what is checked, and why reconciliations finished with an earlier version are checked by their
totals only.

### Income Tax Summary and Income Tax Detail

These total your books by the lines of your income tax return. They use the **Tax form** set in
**Settings › Preferences › Accounting** (Schedule C, Form 1065, Form 1120-S or Form 1120); you can pick another one
on the report.

First give each account its tax line:

1. Open **Income Tax Summary** and click **Tax lines…**.
2. For each income and expense account, choose the line its amounts go on, such as **8 ·
   Advertising** or **26 · Wages**. Your choice is saved straight away.
3. For a partnership or corporation, tick **Show balance sheet accounts** to give asset, liability
   and equity accounts a **Sch. L** (balance sheet) line too.

Then choose the **Tax year** (Last fiscal year to start) or a custom range:

- **Income Tax Summary** shows one amount per tax line: income lines as credits less debits,
  deductions as debits less credits, and Schedule L lines as each account's balance at the end of the
  year. Click a line to see its transactions.
- **Income Tax Detail** lists the transactions behind each line, or one line you choose under
  **Tax line**.
- **Unassigned** lists the income and expense accounts with activity but no tax line, so nothing is
  missed. **Other tax lines** lists accounts with a line from another form, or a line brought in
  from QuickBooks that LocalBooks hasn't matched.

Moving from QuickBooks Desktop: an IIF file's account tax lines come across when they are named
(such as `Sch C: Advertising`). A company file (`.qbw`, `.qbb`) and most IIF files store only
QuickBooks' own number for the line; LocalBooks keeps it as **QuickBooks tax line** and its number,
so accounts that shared a line stay together under **Other tax lines** until you choose the
LocalBooks line under **Tax lines…**.

The amounts come from your books on the accrual basis. They are a starting point for your return,
not the return itself: your tax preparer may adjust them, for example for meals or depreciation.

#### Export to tax software (TXF)

With **Schedule C** as the tax form, **Export TXF…** on the Income Tax report writes the tax year's
totals as a TXF (Tax Exchange Format) file, which personal tax programs import. The panel lists
each Schedule C line with its TXF code and amount before you click **Save TXF file…**; in the file,
income is positive and expenses negative, as tax programs expect. Each account on line **27a ·
Other expenses** goes in as its own entry, described by the account's name.

Under **Not in the file** the panel lists what the file leaves out, with the amount for you to
enter by hand: accounts with no tax line or another form's line, and the two Schedule C lines TXF
has no code for (13 · Depreciation, which goes through Form 4562, and 30 · Business use of home,
Form 8829). TXF has no codes for a partnership's or corporation's own return, so the export is
offered for Schedule C only; for Forms 1065, 1120-S and 1120, export the Income Tax Summary to
Excel or CSV for your preparer instead.

### Collections Report

Every invoice past its due date and still open on the **As of** date, grouped by customer. Each
customer's heading shows how to reach them: the **Company name** (as the contact), **Phone** and
**Email** from the customer's record. **Aging** is the number of days past due. Credit memos and
invoices not yet due are left out; the A/R Aging reports show those.

### Open Purchase Orders

Purchase orders dated on or before the **As of** date that haven't been billed or closed, grouped by
vendor, with the expected date and amount. Purchase orders don't post to the books, so this report
doesn't change any balance.

## Lists and other reports

These reports read the lists and records outside the transaction lines. They export and print like
the others, and a click on a line opens its transaction or account where it has one.

| Report | Group | Shows |
|---|---|---|
| **Balance Sheet Summary** | Business overview | The Balance Sheet with each group of accounts (Bank Accounts, Accounts Receivable, Fixed Assets, ...) on one line, as of a date. The totals are the Balance Sheet's |
| **Profit & Loss YTD Comparison** | Business overview | The Profit and Loss for the report period beside the fiscal year to the period's end |
| **Average Days to Pay Summary** | Who owes you | For each customer, the invoices paid off in the period and the average days from the invoice date to the payment or credit that paid each one off |
| **Unpaid Bills by Job** | What you owe | Bills with a balance as of a date, under the customer or job on their lines. A bill for several jobs is split by its lines' amounts; lines with no job are under **No customer or job** |
| **Open Purchase Orders Detail** | Expenses and vendors | Every line of the purchase orders still open, by vendor (the **Open Purchase Orders** report has one row per order) |
| **Unbilled Costs by Job** | Jobs | Billable lines of checks, expenses and bills not yet on an invoice, under each customer or job, with a total per job (**Customers › Bill Costs** puts them on an invoice) |
| **Open Purchase Orders by Job** | Jobs | The same open lines under the customer or job on each line, lines for no one last; its total matches Open Purchase Orders Detail |
| **Time by Job Summary** and **Detail** · **Time by Name** · **Time by Item** | Jobs | Hours from the timesheet in the period, by customer or job, by employee or by service item, with the billable hours |
| **Physical Inventory Worksheet** | Inventory | Every tracked item with today's quantity on hand and a blank **Physical Count** column to print and fill in |
| **Inventory Stock Status by Vendor** | Inventory | Each tracked item under its preferred vendor, with its reorder point, quantity on hand and quantity on open purchase orders; **Order** marks the items at or below the reorder point even counting what is on order |
| **Inventory Turnover by Item** | Inventory | For each tracked item over the period: inventory value at the start and end, their average, the cost of goods sold, how many times the average stock **turned over** (cost of goods sold ÷ average value) and the **days on hand** that implies. The ending values match Inventory Valuation |
| **Invoice Profitability by Customer** and **Detail** | Sales and customers | Each customer's invoices (or each invoice, under its customer): the income it posted, the cost of goods it sold, gross profit and margin. The totals tie out to the income and Cost of Goods Sold those invoices posted. A product or service whose stock isn't tracked carries no cost here |
| **Audit Trail** | Accountant & Taxes | Every record of the audit trail made in the period: when, which user, what was posted, voided or changed, and the amount. The note under it says whether the trail's chain checks out |
| **Closing Date Exceptions** | Accountant & Taxes | Entries dated on or before the closing date that were entered after their date was closed: made while the closing date was moved back, with when each was entered and since when its date had been closed |
| **Customer Phone List** · **Vendor Phone List** | Lists | Each active customer (jobs as *Customer:Job*) or vendor with their phone number |
| **Customer Contact List** · **Vendor Contact List** | Lists | Company, address, phone, email, terms (and 1099 for vendors) with today's balance; the balances add up to Accounts Receivable (or Payable) |
| **Item Listing** | Lists | Each active product or service with its description, type, SKU, income account, price and quantity on hand |
| **Account Listing** | Lists | Each active account with its type, description and, for balance sheet accounts, today's balance |
| **To Do Notes** | Lists | Every to-do, open ones first by due date: done or not, what it says, who or what it is about and who it is assigned to |
| **Budget Overview** | Budgets | Each budget for the fiscal year holding the date: every account's amount by month and its total, income first, with net income, as entered in **Accounting › Budgets** |
| **Sales by Ship To Address** | Sales and customers | Invoices and sales receipts grouped by their **Ship To** address (those without one together); each amount is the income the sale posted, before sales tax, so the total matches the sales in Profit and Loss |
| **Sales Tax Revenue Summary** | What you owe | For each tax rate: total, non-taxable and taxable sales, the same figures as the Sales Tax Liability report |
| **Memorized Transaction Listing** | Accountant & Taxes | Every memorized (recurring) transaction: its type, how often, the next date, when it ends and whether it is paused |
| **Realized Gains & Losses** | Accountant & Taxes | Gains and losses from paying foreign-currency invoices and bills, by transaction, as posted to **Exchange Gain or Loss** |
| **Unrealized Gains & Losses** | Accountant & Taxes | Home currency adjustments (**Accounting › Currency Revaluation**) and their reversals, as posted to **Unrealized Exchange Gain or Loss** |
| **Terms Listing** | Lists | Every term in the Terms List with its days until due, its early-payment discount and whether it's active |

These reports can't be customized or memorized.

## Comment on a figure

To explain a number to your accountant, a partner or yourself next month, comment on it:

1. Open the report (Profit and Loss, Balance Sheet, a detail report, an Accountant & Taxes report
   and most others) and click **Comment** at the top.
2. Click a figure. It's outlined while commenting is on; the click opens a comment box instead of
   the transactions.
3. Type the comment and press **Enter** or **Save**.

The first comment keeps a copy of the report as it is on screen, with its title and dates, under
**Reports › Accountant & Taxes › Commented Reports**. Later comments in the same report go on the
same copy. Each commented figure shows a number, like **[1]**, and printing the report (or the
kept copy) lists the comments under it by number, with the line, the figure, who wrote it and
when.

**Commented Reports** lists every kept report, the most recently commented first. Open one to read
its comments, add more (click a figure), remove one, **Print…** it, or **Delete** it. The kept copy
doesn't change when the books do: it shows the figures as they were when the comments were made.
Run the report again for today's figures.

## Collapse and expand

A report with headings (accounts, names, jobs, vendors, sections of the Balance Sheet Summary) has a
▾ beside each heading. Click it to show only that heading's total; click ▸ to show its lines again.
**Collapse all** and **Expand all** above the report do every heading at once. A report prints the
way it is shown; Export to Excel and CSV always have every line.

## Find in a report

Type a word, a name or an amount in **Find in report**, beside **Export** above the report. Every
cell that matches is shaded, and the box says how many there are. Press **Enter** (or ↓) to go to
the next one, which is shaded darker and scrolled into view; **Shift+Enter** (or ↑) goes back, and
**Esc** clears the search. An amount matches however the report writes it: `1250` finds
`1,250.00` and `(1,250.00)`. When nothing matches, the box says **No matches**. The shading
follows the report when you change its dates or options, and it is never printed.

## Customize a report

1. Open any detail report and click **Customize…**.
2. Under **Display**, choose **Detail** (one row per line) or **Summary** (one row per group):
   - **Subtotal by** (or **Rows**, in a summary): account, name, product or service, class,
     transaction type, rep, transaction, month, quarter, year or aging period.
   - **Columns over time** (summary only): a column for each month, quarter or year, and a total.
   - **Columns by name** (summary only, instead of columns over time): a column for each **Class**,
     **Customer** (its jobs added in), **Customer:Job** or **Vendor** the lines carry, then one for
     lines without one (*Unclassified*, *No customer*, *No vendor*), and a total. For example, a
     summary by account of income and expense accounts with columns by Customer:Job is a profit and
     loss by job. The customer or vendor is the transaction's (the invoice's customer, the bill's
     vendor). Each row's columns add up to its total.
   - **Columns, in order**: tick the columns to show (Type, Date, Num, Name, Memo, Account, Split,
     Class, Item, Qty, Rate, Debit, Credit, Amount, Balance, Open Balance, Due Date, Aging, Rep) and
     move them with the arrows.
   - **Amount**: on each account's normal balance side, debits minus credits, or credits minus
     debits.
3. Under **Filters**, narrow the lines by accounts (with their subaccounts) or account types,
   customers and vendors, transaction types, classes, products and services, reps, paid status,
   cleared status, an amount range (either side) and text in the memo. The report updates as you go.
4. Change the **Title** if you like, and click **Done**.

**Reset to preset** puts a report back the way it came.

In a summary, click a row to see the lines behind it; **‹ Back to summary** returns.

## Memorize a report

1. Customize a report and click **Save as…**.
2. Name it and click **Save**. A name already in use asks before it is replaced.

Saved reports are listed under **Memorized reports** at the bottom of the report list. They keep the
filters, columns, grouping and report period (such as **This month**), not the numbers, so they show
current figures each time. To change one, open it, customize it and **Save as…** the same name; to
remove it, click **Delete…**. Memorized reports are saved in the company file.

### Process Multiple Reports

To run several memorized reports together, choose **Process Multiple Reports…** at the end of
**Memorized reports**:

1. Tick the reports to run (the box in the heading ticks them all).
2. Under **Report period**, keep **Each report's own dates** or choose one period for all of them.
   Reports as of a date use the period's last day.
3. Click **Display** to show them one after another, **Print** to print them in one go (each starts
   a new page), or **Export to Excel** to save one workbook with a worksheet for each report.

## Graphs

**Reports › Graphs** draws six of your reports as charts. Each graph uses the same numbers as its
text report for the same dates, so a bar or slice always matches what that report shows.

| Graph | What it shows | Click a bar or slice to open |
|---|---|---|
| **Income and Expense Graph** | Income and expenses by month, and a pie of expenses (or income, under **Pie shows**) by account | The month's **Profit and Loss**; a slice opens the account's register for the period |
| **Net Worth Graph** | Total assets and total liabilities at each month end, with net worth (assets less liabilities) as a line | The **Balance Sheet** as of that month end |
| **Accounts Receivable Graph** | What customers owe by days past due, and the customers with the largest balances | The **A/R Aging Summary** as of the same date |
| **Accounts Payable Graph** | The same for what you owe vendors | The **A/P Aging Summary** |
| **Sales Graph** | Sales by month, and a pie of the top customers, products/services or reps (**By**) | The **Sales by Customer**, **Product/Service** or **Rep Summary** for that month or the period |
| **Budget vs. Actual Graph** | Actual against budget by fiscal month, for net income, income or expenses (**Show**) | **Profit and Loss Budget vs. Actual** for that month |

To use a graph:

1. Choose it under **Reports › Graphs**, then the period (or **As of** date, or budget).
2. Hover over a bar or slice to see its amounts. Click it (or Tab to it and press Enter) to open the
   report behind it.
3. Choose **Table** at the top right to see the same numbers as a table.

Pies and rankings show the seven largest names or accounts and add the rest up as **Other**. Income
and Expense counts cost of goods sold and other expenses as expenses, like the Profit and Loss. The
graphs cover at most 36 months at a time.

**Export ▾** on a graph has **Save as picture (PNG)** (in the Chart view), **Print / Save to PDF**
(the charts and their tables on one page), and **Export to Excel** or **Export to CSV** for the
numbers behind the chart.

## Export or print

1. Click **Export ▾** on the report.
2. Choose **Export to Excel**, **Export to CSV** or **Print / Save to PDF**.
3. In the print dialog, choose **Save as PDF** to keep a copy. Your company name and logo are on it.

**Export to Excel** saves an `.xlsx` workbook that opens in Excel, Numbers, Google Sheets or
LibreOffice, laid out like QuickBooks' *Export to Excel*:

- The company name, report title and date range (and the basis, where there is one) are in the top
  rows. The column headings are bold and stay in view while you scroll.
- Amounts are numbers in your company's currency format, so you can add them up or chart them.
  Negative amounts are red. Dates are real dates, so you can sort and filter by them.
- Subaccounts keep their indent under their parent, and section, subtotal and total rows are bold
  with the same lines above them as on screen.

**Export to CSV** saves plain text that any program can read: amounts are plain numbers such as
`1234.56`, and subaccounts are indented with spaces.

Registers, the Chart of Accounts, the Customers, Vendors and Products & Services lists, and the
audit log have an **Export ▾** button too, with the same Excel and CSV choices. Each exports what
the screen shows: the register's date range, the list's search and **Show inactive** choice, the audit
log's event filter. Audit log times are in this computer's time zone.

> **Tip:** The **Dashboard** shows a live trial balance. **✓ In balance** means total debits equal
> total credits, which is always true because every entry must balance to post.
