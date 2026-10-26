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
