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
