# Run, export and print reports

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
| **Balance Sheet Detail** | Every asset, liability and equity line in the period, by account, from each account's beginning balance |
| **A/R Aging Detail** · **A/P Aging Detail** | Open invoices and credit memos (bills and vendor credits), grouped by how far past due they are |
| **Open Invoices** · **Unpaid Bills** | The same open items, grouped by customer or vendor |
| **Customer Balance Summary** and **Detail** · **Vendor Balance Summary** and **Detail** | What makes up each customer's receivable or each vendor's payable balance, as of a date |
| **Sales by Customer Detail** · **Sales by Product/Service Detail** | Each sale, by customer or by product or service, with quantity and rate |
| **Income by Customer Summary** and **Detail** | Income less expenses for each customer (credits minus debits) |
| **Expenses by Vendor Detail** | Each expense line, by vendor |
| **Purchases by Vendor Summary** and **Detail** · **Purchases by Product/Service Summary** and **Detail** | Product and service lines on bills, checks, expenses and vendor credits |
| **Check Detail** · **Deposit Detail** | Each check and bill payment (or deposit) with the lines it paid for, debits minus credits |
| **1099 Detail** | The checks, expenses and bill payments from bank accounts behind the 1099 Summary, by vendor |
| **Transaction List by Date** | One row per transaction, with its total |
| **Journal** | Every transaction with all of its debit and credit lines |
| **Adjusting Journal Entries** | Only the journal entries marked **Adjusting entry**, with all of their lines |

The numbers agree with the other reports: account lines and balances with the General Ledger, the
aging details with the A/R and A/P Aging Summary, the Sales by Customer and Product/Service Detail
with their summaries, and the 1099 Detail with the 1099 Summary.

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
the **Trial Balance**, **General Ledger**, **Transaction List by Date**, **Journal** and
**Adjusting Journal Entries**, and the reports below. Each one exports and prints like any other,
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
**Settings › Preferences** (Schedule C, Form 1065, Form 1120-S or Form 1120); you can pick another one
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

### Collections Report

Every invoice past its due date and still open on the **As of** date, grouped by customer. Each
customer's heading shows how to reach them: the **Company name** (as the contact), **Phone** and
**Email** from the customer's record. **Aging** is the number of days past due. Credit memos and
invoices not yet due are left out; the A/R Aging reports show those.

### Open Purchase Orders

Purchase orders dated on or before the **As of** date that haven't been billed or closed, grouped by
vendor, with the expected date and amount. Purchase orders don't post to the books, so this report
doesn't change any balance.

## Customize a report

1. Open any detail report and click **Customize…**.
2. Under **Display**, choose **Detail** (one row per line) or **Summary** (one row per group):
   - **Subtotal by** (or **Rows**, in a summary): account, name, product or service, class,
     transaction type, rep, transaction, month, quarter, year or aging period.
   - **Columns over time** (summary only): a column for each month, quarter or year, and a total.
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
