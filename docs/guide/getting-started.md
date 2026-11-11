# Set up your company

LocalBooks keeps your books in a single company file on your computer. These steps set up the
details that appear on invoices and reports and decide how your fiscal year works.

## Made for US businesses

LocalBooks is built for businesses in the United States. You can use it elsewhere for plain
double-entry books, but these parts follow US rules and formats only:

- **Sales tax** is US-style: a rate (and the agency it's owed to) on each invoice or receipt, added
  on top of the price. There is no VAT or GST: no tax-inclusive prices, no input tax credits and no
  VAT or GST return.
- **1099s and W-9s** are the IRS forms (1099-NEC and 1096). The **tax calendar**, tax-line mapping
  and **TXF export** cover US federal returns only (Schedule C, 1065, 1120-S, 1120).
- **Payroll:** LocalBooks calculates no pay or withholding in any country. Payroll import posts the
  journal your payroll service exports.
- **Bank and payment files:** ACH payment files use the US NACHA format and nine-digit US routing
  numbers, and printed checks follow US check layouts with the amount in English words.
- **Dates and numbers** show the US way: dates as month/day/year (10/13/2026) and amounts as
  1,234.56. There is no setting for day/month/year dates or a decimal comma, and the screens are in
  English only.
- **Currency:** dollars by default. You can choose another home currency (Settings ›
  Preferences Console › Multiple Currencies), but the tax features above still follow US rules.

## Create a new company

**File › New Company…** opens a short setup assistant, modeled on QuickBooks' own:

1. **Company information.** Enter the company name, and if you like the legal name, Federal ID
   (EIN) and address. Choose the **Income tax form used** (sole proprietorship, single- or
   multi-member LLC, partnership, corporation, S corporation, nonprofit, or other) and the
   **First month of fiscal year**.
2. **Industry and accounts.** Choose the industry closest to yours from the 33 listed.
   LocalBooks lists the accounts it suggests for that industry, with five-digit account numbers
   (10100 Checking, 11000 Accounts Receivable, 60000 Advertising and Promotion…).
   The ones most businesses like yours need are already checked; check any others you want and
   uncheck the rest. **Default** puts back the suggested set. The equity accounts follow your tax
   form, for example Owners Draw and Owners Equity for a sole proprietor, or Capital Stock and
   Shareholder Distributions for an S corporation.
3. **Expert settings.** Choose whether customers are charged sales tax and whether you track
   inventory. Each adds the accounts it needs.

Click **Save As…** to choose where to save the company file; it opens straight away. If you'd
rather start from LocalBooks' standard chart of accounts and set things up yourself, click
**Skip — use the standard chart** instead. Either way, you can add, rename or remove accounts
later in the Chart of Accounts.

### Merge two accounts

To merge an account you don't need into another of the same type, open it with **Edit…** in the
Chart of Accounts, change its name to the other's (they must sit at the same level), and click
**Save**; LocalBooks asks whether to merge them. Posted transactions keep the account they posted
to, so LocalBooks moves the merged account's balance with one journal entry dated today, and points
items, sales tax agencies, budgets, subaccounts, bank feed rules, settings, unposted estimates and
purchase orders and memorized transactions at the account you kept. The merged account becomes
inactive. Bank and credit card accounts, and LocalBooks' own accounts (such as Accounts Receivable),
can't be merged. Merging can't be undone; voiding its journal entry moves the balance back but
doesn't restore the lists.

## Enter your company details

1. In the left sidebar, open **Settings › Preferences Console** (or choose **Edit › Preferences**).
   Click **General** in the list on the left, then the **Company Preferences** tab.
2. Under **Company information**, fill in **Company name**, **Legal name**, **Address**, **Email**, **Phone**
   and **EIN**. Each field saves as soon as you click away from it; a green **✓ Saved** confirms it.
3. Click **Upload logo…** to add your logo. It prints on invoices, estimates and reports.

## Choose your accounting settings

1. In Preferences, click **Accounting**, then **Company Preferences**. Set **First month of fiscal
   year**. Reports such as "This fiscal year" and the Balance Sheet's Retained Earnings follow this.
2. Under **Multiple Currencies › Company Preferences**, set **Currency symbol** (for example `$` or
   `€`) and **Home currency** (for example `USD`). The home currency locks once you record a
   foreign-currency transaction.
3. Back in **Accounting**, choose your **Tax form** (Schedule C, 1065, 1120-S, 1120 or 990). The **Tax calendar** below then
   lists your federal due dates.

> **Tip:** The tax calendar is a reminder, not tax advice. Weekends move to Monday, but holidays,
> extensions and state deadlines are not included.

## Design your forms and set up email

1. Open **Settings › Custom Forms** to choose how invoices and estimates look: title, font, colors,
   logo placement, columns, and the message and terms printed on them. See
   [Design your invoices and estimates](custom-forms.md).
2. Emailed forms open in your own mail app with the PDF attached. To send them from LocalBooks
   instead, enter your email provider's SMTP settings in **Settings › Email**.

## Check your chart of accounts

A company set up with the assistant starts with the accounts you chose for your industry. One
created with **Skip** starts with LocalBooks' standard chart of accounts (bank, receivables,
payables, equity, income and common expenses). You choose these accounts on every form, so you can
start recording straight away. Accounts a feature needs, such as Sales Tax Payable or Inventory
Asset, are added the first time you use that feature.

## Make sure backups are on

1. In **Preferences › General › Company Preferences**, under **Automatic backups**, keep **Daily backup** turned on. A backup is taken when LocalBooks
   starts, at most once a day.
2. Set **Keep** to how many daily backups to keep (1–60).

## Track classes

Classes sort income and expenses by department, location, property or any other division you
choose, so you can run a Profit and Loss by class. Class tracking is off until you turn it on.

1. In **Settings › Preferences Console › Accounting › Company Preferences**, under **Class tracking**, tick **Track classes on transactions**.
2. Choose **Assign classes**: **One to each line** (a **Class** column on each line), or **One to
   the entire transaction** (one **Class** box at the top of the form).
3. Optionally tick **Warn me when a transaction is not assigned a class**.

Invoices, sales receipts, estimates, credit memos, bills, checks, expenses, purchase orders, vendor
credits, deposits and journal entries then have a Class column (or box). To add a class while you
enter a transaction, choose **＋ Add new class…** at the bottom of the class list, type the name
and press Enter; type `Parent:Name` to add it under an existing class. Manage the whole list in
**Accounting › Classes**.

Importing a QuickBooks Desktop company file, an IIF file or a report with a **Class** column
brings its classes and turns class tracking on (see [Classes](desktop-migration.md#classes)).

## Find your way around Preferences

**Settings › Preferences Console** (or **Edit › Preferences**) looks like the desktop product's
Preferences window: a list of categories on the left (Accounting, Bills, Checking, Desktop View,
Finance Charge, General, Items & Inventory, Jobs & Estimates, Multiple Currencies, Payments,
Reminders, Reports & Graphs, Sales & Customers, Sales Tax, Send Forms, Tax: 1099, Time & Expenses),
and two tabs for each:

- **My Preferences** are yours, on this computer, and take effect at once.
- **Company Preferences** are everyone's and are saved in the company file as soon as you leave a
  box. Only an Admin can change them.

Some settings live on their own screen (sales tax rates, reminders, email, forms). The category
lists them under **Set on their own screens**, with a link to each. LocalBooks remembers the
category you used last.

The defaults for selling, getting paid and paying bills are in these categories' Company
Preferences:

- **Sales & Customers:** the usual Ship Via on new sales forms, the default markup for
  pricing from cost, price levels on or off, and a warning before selling to a customer with
  overdue invoices.
- **Jobs & Estimates:** estimates on or off, and the names of the five job statuses.
- **Payments:** how a received payment is applied, whether ticking invoices works out the amount,
  and whether payments go to Undeposited Funds.
- **Bills:** how many days after the bill date a bill is due when the vendor has no terms, and
  whether Pay Bills uses vendor credits and takes early-payment discounts.
- **Send Forms:** how new customers' forms are sent (Email later, Print later or neither).

## Set how typing works (General preferences)

**Preferences › General › My Preferences** sets how typing works for you. **Default** puts these
choices back.

- **Pressing Enter moves between fields.** Enter goes to the next box, as Tab does. Record the
  transaction with its button (**Post**, **Save**). Off by default. In Preferences itself, Enter
  always saves the box you typed in and moves to the next one.
- **Open the list of matches as you type.** Name boxes such as **Vendor** show matching names while
  you type, and offer to add a name that isn't on the list. Turn it off to open the list only when
  you press ↓.
- **Show the full text of clipped entries on hover.** Text cut off with "…" shows in full when you
  point at it.
- **Date for new transactions.** New bills, checks, invoices, sales receipts, deposits and journal
  entries start on **Today's date**, or on **The last date you entered**, which helps when you
  enter a stack of older paperwork.
- **Automatically place a decimal point.** Amounts typed without a point are in cents: `1234`
  becomes 12.34 and `5` becomes 0.05, which is quick on a number pad. Typing a point still means
  dollars (`12.` is 12.00). Off by default.
- **Beep when recording a transaction.** A short tone when a bill, check, invoice, payment, deposit
  or journal entry is saved. **Test** plays it.
- **Warn when editing a posted transaction.** Saving changes to a posted transaction voids the
  original and posts the edited one in its place; this asks first. On by default.
- **Warn when voiding a transaction.** **Void Transaction** asks before reversing it. On by default.
- **Bring back all one-time messages.** Messages you turned off with **Don't show this message
  again** appear again.
- **When the item on a sales line changes.** If you typed your own description or price on a line
  and then pick a different item, LocalBooks can **Ask whether to keep it** (the default), **Always
  keep what I typed**, or **Never** keep it and use the new item's description and price.

**Preferences › Accounting › My Preferences** has **Autofill memo in journal entries**: a line you
start below the first one takes the first line's memo, which you can still change. On by default.

## Do arithmetic in an amount or quantity box

Every amount, rate and quantity box on a form works as a calculator. Type a number, then `+`, `-`,
`*` or `/` and the next number: a small tape opens under the box with each number you typed and the
running total. Press **Enter** (or leave the box) to put the answer in it; **Esc** closes the tape
and leaves what you typed.

- `100+25.50*2` gives **151.00**: multiplying and dividing come first, as on paper, then adding and
  subtracting from left to right.
- The arithmetic is exact until the end, then rounded once to the cent, half away from zero:
  `10/3` is 3.33 and `0.05/2` is 0.03. A quantity box keeps four decimals: `10/3` is 3.3333.
- Numbers in the arithmetic are taken as typed, even with **Automatically place a decimal point**
  on: `12*3` is 36.00.
- `-` in front of the first number is a minus sign, so `-12` is just a negative amount.

## Warnings before saving (company preferences)

These ask before a transaction is saved. Click **Save anyway** to record it as you entered it, or
**Cancel** to go back and change it. They never stop you saving.

| Category (Company Preferences) | Warning | Default |
|---|---|---|
| **Checking** | A check number already used by another check from the same bank account (voided checks don't count) | On |
| **Bills** | A bill number already used by another bill from the same vendor | On |
| **Bills** | A new check or expense to a vendor that has unpaid bills: it doesn't pay them, so use **Pay Bills** for a bill | On |
| **Sales & Customers** | A new sales receipt from a customer (or job) that has open invoices: it doesn't pay them, so use **Receive Payments** for an invoice | On |
| **Accounting** | A date more than **N** days before today (0 turns it off) | 90 days |
| **Accounting** | A date more than **N** days after today (0 turns it off) | 30 days |
| **Accounting** | A transaction that posts to Retained Earnings, which LocalBooks updates itself at year end | On |

The date warnings cover bills, checks, expenses, invoices, sales receipts, estimates, purchase
orders and journal entries; they catch a slip like 2062 for 2026.

Some things LocalBooks always does, so they have no switch: every line needs an account; journal
entries, invoices, estimates and purchase orders get the next number when saved, so no two share
one.

**General › Company Preferences** also sets how dates and hours show for everyone: **Always show
years as 4 digits** (turn it off for 10/13/26), and **Time format**, **Minutes (1:30)** or
**Decimal (1.50)** in Time Tracking.

## Report and display preferences

**Settings › Preferences Console › Reports & Graphs** sets how reports open for everyone (Company
Preferences) and for you (My Preferences). **Accounting › Company Preferences** sets how accounts
are named in reports.

| Category | Option | What it changes | Default |
|---|---|---|---|
| **Reports & Graphs** | Summary reports basis | Profit and Loss and the Income & Expense graph open on accrual or cash; each report can still switch. The Balance Sheet is always accrual. | Accrual |
| **Reports & Graphs** | Aging reports | A/R and A/P Aging count days from the due date, or from the invoice or bill date | Due date |
| **Reports & Graphs** | Items | Sales by Item, Inventory Valuation and Stock Status show items by name, description or both (an item without a description shows its name) | Name only |
| **Reports & Graphs** | Statement of Cash Flows | Which section (operating, investing or financing) each balance sheet account goes in; **Revert to defaults** puts every account back | By account type |
| **Reports & Graphs** (My) | Show the report's options before running it | Profit and Loss, Balance Sheet, A/R and A/P Aging and the sales summaries ask for their dates (and basis) before they run; reports opened from a graph or link run at once | Off |
| **Accounting** | Show account numbers in reports | "6300 Rent" or just "Rent" in the financial statements, trial balance and general ledger | On |
| **Accounting** | Show lowest subaccount only | "Electric" instead of "Utilities:Electric" in the trial balance and general ledger | Off |

By default the Statement of Cash Flows puts current assets and liabilities (receivables, payables,
credit cards) in operating, fixed and other assets in investing, and long-term liabilities and
equity in financing. Bank accounts are the cash itself, so they can't be moved. **Classify
accounts…** on the report opens the list.

## Sales tax rates, combined rates and the sales tax return

If you charge sales tax, turn it on in **Sales › Sales Tax** and add each rate you charge with the
agency you pay it to. Where a state, county and city all tax one sale, a combined rate adds them up
and posts each agency's share to its own account. The same screen shows what you owe, the sales tax
return by agency for a period, and records your payments: see [Sales tax](sales-tax.md).

**Adjust…** (next to Pay sales tax) corrects what is owed on one rate without a sale: a discount
the agency gives for filing on time, a penalty or interest, rounding on the return, or tax owed on
a sale that wasn't taxed. Choose the rate, the date, **Reduce** or **Increase**, the amount and the
account on the other side (an income account for a reduction, an expense account for an
increase). It posts to the rate's liability account, and the sales tax return shows it in that
rate's tax, marked *adj.*, under its agency for the period it is dated in. A combined rate is
adjusted one part at a time, so each agency's figure is right. To undo one, choose **Void** on it
in the Adjustments list.

**Pay sales tax** for a combined rate splits the payment among its rates in proportion to their
percentages (any rounding cent going to the largest) and pays down each rate's liability account,
all in one entry from the account you pay from; the return lists each share under its rate's
agency. To pay agencies different amounts, or on different dates, pay each agency's own rate
instead. (A combined-rate payment recorded in version 0.14.0 went entirely to its first rate's
account and is listed under that rate's agency; void it and record it again to spread it.)

## Sales tax, time, 1099 and inventory preferences

These company preferences (Admins only) change how LocalBooks works out tax, time and stock.

| Category (Company Preferences) | Option | What it does | Default |
|---|---|---|---|
| **Sales Tax** | **Owe sales tax** as of invoice date (accrual) or upon receipt of payment (cash) | The Sales Tax Liability report and **Pay sales tax** follow it. On cash basis an invoice counts as it is settled: in proportion to each payment, and to each credit memo applied to it, less bounced checks, on their dates. Sales receipts count when made, and credit memos on their own date on both bases (so a credit applied to an invoice nets to zero over time) | Accrual |
| **Sales Tax** | **File returns** monthly, quarterly or yearly | The Sales Tax screen and report open on the last period that ended (the return to file now), and **Reminders › Tax dates** shows that return, due on the 20th of the next month (check your state's date) | Quarterly |
| **Sales Tax** | **Mark taxable amounts with a "T" when printing** | Taxable lines on printed and emailed invoices, sales receipts and estimates show a T | On |
| **Finance Charge** | **Calculate charges from** the due date or the invoice date | Where interest starts. An invoice is overdue by its due date and grace days either way | Due date |
| **Time & Expenses** | **First day of work week** | Weekly timesheets start on it | Monday |
| **Time & Expenses** | **Mark all time entries as billable** | New time and timesheet rows start with Billable ticked | Off |
| **Time & Expenses** | **Mark all expenses as billable** | Expense, check and bill lines given a Customer:Job start with **Bill?** ticked | Off |
| **Time & Expenses** | **Track reimbursed expenses as income**, the income account, **Default markup** and markup account | How **Sales › Bill Costs** invoices billable expenses (see below) | Off, 0% |
| **Tax: 1099** | **Do you file 1099s?** | Off hides the 1099 box on vendors, the 1099 reports and the 1099 deadline | On |
| **Tax: 1099** | **Accounts to boxes** | Which 1099-NEC or 1099-MISC box each expense account's payments go in | None mapped |
| **Items & Inventory** | **Inventory and purchase orders are active** | Off hides Purchase Orders and **Track stock**; items already tracked keep their stock | On |
| **Items & Inventory** | **Not enough stock**: warn, don't allow the sale, or say nothing | When an invoice or sales receipt would take a tracked item below zero | Warn |
| **Checking** | **Print the vendor's account number in the check memo** | A check left without a memo gets "Acct #…" from the vendor's **Account no.** | Off |
| **Checking** | **Change the check date when a check is printed** | A check waiting in **File › Print Forms › Checks** takes the date it prints | Off |

**Checking › My Preferences** sets the account **Write Checks**, **Pay Bills** and **Make
Deposits** open with, on this computer.

**General › My Preferences › Automatically recall information**: once you pick a vendor you have
paid before on a new bill, check or expense, its lines start like the last one, with the amounts
(**Recall the last transaction**) or without (**Pre-fill the accounts used last time**). Off by
default.

### Bill billable expenses to a customer

1. On a check, expense or bill, give the line a **Customer:Job** and tick **Bill?**.
2. Open **Sales › Bill Costs**, pick the customer, tick the costs and check the **Markup %**.
3. Click **Create invoice**. Each cost becomes a line, crediting its expense account back (or the
   income account, if reimbursed expenses count as income), with one **Markup** line on top.

Voiding that invoice puts the costs back on the list; editing it keeps them billed. When you edit
it, a reimbursed line can move to another expense account (to correct where the cost was booked),
but not to a bank, asset or liability account.

A cost on an expense or check in another currency is billed in your home currency, at the
exchange rate the expense was entered with, so the reimbursement takes off exactly what the expense
put on. **Bill Costs** shows the original amount under the home amount, and the invoice line says
it (for example *EUR 200.00*).

When reimbursed expenses count as income, each expense account can send its reimbursements to an
income account of its own: open the expense account in the **Chart of Accounts** and choose it
under **Reimbursements billed to customers go to**. Accounts left on the default use the income
account in **Preferences › Time & Expenses**.

### Bill time to a customer

Time entered on the timesheet as **billable**, for a customer or job and with an hourly rate, is
listed at the top of **Sales › Bill Costs** for that customer: the date, who did it, the service
item, the hours, the rate and the amount. Tick the entries to bill (they start ticked) and click
**Create invoice**: each becomes a line for its service item, at its hours and rate, crediting the
item's income account. Time has no markup. An entry without a service item can't be billed until it
has one on the timesheet. As with costs, voiding the invoice puts the time back on the list and
editing it keeps the time billed.

### 1099 boxes and thresholds

Map each expense account to a box under **Tax: 1099 › Company Preferences**, for example
Contractors to 1099-NEC box 1 and Rent to 1099-MISC box 1. **Reports › 1099 Summary** then shows
each vendor's total per box, and **1099 Detail** each payment's share per account; a bill payment
is split over the bill's accounts. Accounts left as *Not reported* are left out. With nothing
mapped, every payment counts as nonemployee compensation, as before.

A box is reported once the year's payments reach its threshold, which goes by the year paid:
$600 before 2026 and $2,000 from 2026 for nonemployee compensation, rents, other income and
medical payments; $10 for royalties; $600 for attorneys' gross proceeds.

### W-9s, 1099-NEC forms and the 1096

Keep each contractor's Form W-9 on the vendor: open the vendor (**Expenses › Vendors › Edit…**)
and click **W-9…** beside **Track payments for a 1099**. Enter the name on line 1 (as on their tax
return), any business name, the tax classification, whether the TIN is an **SSN or ITIN** or an
**EIN**, the TIN itself, and the date the W-9 came in. LocalBooks rejects numbers the IRS never
issues (all-zero parts, SSNs starting 666, EIN prefixes it doesn't use) without repeating the
number in the message. Once users are on, the TIN is encrypted in the company file, and only users
who can see sensitive data see it in full; the audit trail notes only that it changed.

**Reports › 1099-NEC and 1096** lists, for a calendar year, each vendor paid at least the
threshold in box 1 (nonemployee compensation), with what is still missing to file: the TIN, its
type, the W-9 date or an address. Click **Edit W-9** to fill a gap. Your company's name, EIN and
address (Settings › Company) are the payer's. **Print forms…** prints:

- a **filer's worksheet** with the Form 1096 totals (number of forms, total reported) and every
  recipient's full TIN, name, address and box 1 amount, to enter in the IRS's free IRIS Taxpayer
  Portal, or to copy onto the official red-ink Copy A and Form 1096 if you file on paper (Copy A
  printed on plain paper can't be filed); keep it safe;
- **Copy B** for each recipient, on plain paper, with their TIN shown as its last four digits, as
  the IRS allows on recipient copies. Send them by January 31.

Filing electronically straight from LocalBooks, in the IRS's IRIS format, isn't built yet: the
IRS gives its schema only to registered software developers.

**Units of measure** (selling by the each, case or pallet) are not in LocalBooks: each item has one
unit.

## Sort a list

Click a column heading in a list (invoices, bills, estimates, checks, customers, products, the
journal, account registers, Reconcile and others) to sort by it; click it again to reverse the
order. An arrow shows which column is sorted.

Lists of transactions start with the newest at the top. LocalBooks remembers the order you choose
for each list on this computer; click the sorted heading a third time to go back to newest first.
Registers and Reconcile start, and go back to, date order (oldest first). While a register is
sorted by anything else, its **Balance** column is dimmed, because it is still the running balance
in date order.

## Make the text larger or smaller

Each kind of window keeps its own text size: lists (customers, items, the chart of accounts and the
like), registers, forms (invoices, bills, checks, journal entries and the other entry forms) and
reports. With one open in front:

- Click **A↑** or **A↓** at the right of the toolbar, choose **View › Larger Text** or
  **View › Smaller Text**, or press **Cmd +** and **Cmd −** on a Mac (**Ctrl +** and **Ctrl −** on
  Windows and Linux).
- The size shows beside the buttons when it isn't the normal one; click it, choose
  **View › Actual Text Size**, or press **Cmd 0** (**Ctrl 0**) to go back.

Every window of that kind takes the new size at once, and LocalBooks keeps it on this computer for
next time. The Home page and Settings keep the normal size.

## Give a register a color

In an account register, choose a **Color** (blue, green, yellow, orange, pink, purple or gray) to
tint it, so the checking register is easy to tell from savings or a credit card at a glance. The
color is kept on this computer for that account in that company file; **Default** takes it away.

## Put a list in your own order, flat or as a tree

The chart of accounts, the item list and the customer list can be put in any order you like:
drag a row up or down and drop it where you want it. A row moves among the rows beside it (the
accounts under the same parent, a customer's jobs, the sub-items of an item), and takes its
subaccounts, jobs or sub-items with it. LocalBooks keeps the order in the company file, so everyone
who opens it sees the same order.

- **Tree** shows subaccounts, jobs and sub-items (an item named `Parent:Child`) indented under
  their parent, with ▾ to fold one away; **Expand all** and **Collapse all** open or fold them all.
- **Flat** shows every row at the same level with its full name (`Checking:Payroll`).
- **Usual order** forgets the order you dragged rows into.

Rows can't be dragged while you are searching a list or have sorted it by a column heading; click
the sorted heading until the arrow goes away to get back to your own order.

## Move around a list from the keyboard

In a list (invoices, bills, customers, vendors, items, accounts, a register and the others), press
**Tab** to reach the first row, **↑** and **↓** to move between rows (**Home** and **End** for the
first and last), and **Enter** to open the row, as a double-click does. On the customer, vendor,
item and account lists, the row with focus is also the selected one, so **Ctrl+Q** and the
information panel follow it. The row's **▾** button is one more **Tab** away.

## Choose a list's columns

The customer and vendor lists, the item list, the chart of accounts and account registers (one
line per transaction) have a **Columns ▾** button. Tick a column to show it or untick it to hide
it, and use ▲ and ▼ to move it earlier or later; the table changes as you go. Some columns are off
until you turn them on, such as **Terms**, **Tax**, **Preferred delivery** and **Job status** on
the customer list. The first column (the name, the account or the date) always shows.
**Default columns** puts a table back the way it came.

**Reconcile** has the same button above its lines (the amounts and the cleared tick always show).
The line grids on forms have a smaller **Columns ▾** link: in the **Description** heading on
invoices, sales receipts, estimates and credit memos, where it hides or reorders **Service Date**,
**Price level**, **Class** and **Income account** (off until you turn it on) and moves **Qty** and
**Rate** among them (the item
and description stay first and the amount last, so subtotals and groups line up); and beside
**＋ Add split row** on bills, checks, expenses, vendor credits, purchase orders and deposits, for
every column before the amount. A hidden column still keeps what was in it; show it again to
change it. The sales forms share one layout, and so do the purchase and banking forms.

LocalBooks keeps each table's columns on this computer for you, in **My Preferences**, so they stay
the same the next time you open it.

## Open a transaction from a register

In an account register, click a row to see all of the transaction's lines underneath it.
Double-click the row (or choose **Open** in its **Actions** menu) to open the transaction itself:
bills, checks, expenses, credit card credits and invoices open in their own form, and anything else
opens in the journal with the entry expanded.

## Work in more than one window

LocalBooks handles windows the way QuickBooks Desktop for Windows does. Forms, registers and reports
open as windows *inside* the LocalBooks window, so you can keep a report beside the invoice you're
entering, all in one program window. The menus use QuickBooks' names.

### Windows inside LocalBooks

- Choosing a page in the sidebar or the menu bar opens it in a window, or brings forward the window
  that already shows it. **Edit…** on an invoice, bill, check or expense, or a report chosen in the
  Reports list, opens it in a window of its own. Opening the same one again brings its window forward.
- Each window has a title bar with **Minimize**, **Maximize** (or **Restore Down**) and **Close**.
  Drag the title bar to move it; drag an edge or corner to resize it; double-click the title bar to
  maximize or restore it.
- A **maximized** window fills the work area. Its buttons move up to the right end of the toolbar,
  where QuickBooks puts them on the menu bar, and a new window opens maximized too.
- **Minimized** windows wait as small title bars along the bottom of the work area. Click one to bring
  it back.
- Every window shows the same books: when you save a change in one, the others update straight away.
  If a window has a form with entries you haven't saved, LocalBooks asks before it closes.

| Keys | What they do |
|---|---|
| **Tab** and **Shift+Tab** | Move through the active window's fields and buttons; past the last one, back to its first (they don't move into another window) |
| **Ctrl+F6** or **Ctrl+Tab** | Go to the next window (add **Shift** for the one before) |
| **Esc** or **Ctrl+F4** | Close the active window (⌘W on a Mac) |

### One Window or Multiple Windows

As in QuickBooks, choose **View › One Window** or **View › Multiple Windows** (also in **Edit ›
Preferences › Desktop View**, under **View**):

- **Multiple Windows** (the default, as in QuickBooks): windows float in the work area, side by side
  or overlapping.
- **One Window**: one window shows at a time, filling the work area; the others wait behind it.
  Switch with the **Window** menu or the **Open Window List**.

The choice is kept with each company.

### The Window menu and the Open Window List

- **Window › Close All** closes every window (each asks first if it has unsaved entries).
  **Cascade**, **Tile Vertically** and **Tile Horizontally** arrange the open windows in the work area;
  **Arrange Icons** lines the minimized ones up along the bottom.
- Below those, the **Window** menu lists the open windows, numbered, the active one ticked. Choose one
  to bring it forward.
- **View › Open Window List** shows the open windows in a pane beside the sidebar, the active one
  highlighted (minimized ones in italics). Click a window to bring it forward. Choose the menu item
  again (or **✕** on the pane) to hide it.

### A second LocalBooks window

**Window › New Window** (**Ctrl+Shift+N**, ⇧⌘N on a Mac) opens a second LocalBooks window on the same
company, for example on a second screen. It starts on the page you're looking at and has windows of
its own inside it.

### Reopen your windows with the company (saved desktop)

When you open a company, LocalBooks reopens the windows you had inside it: their screens, forms,
registers and reports, where they were, which were minimized or maximized, and where the LocalBooks
window itself was. Choose how in **Edit › Preferences › Desktop View**, under **Desktop**, the same
choices as QuickBooks:

| Choice | What reopens |
|---|---|
| **Save when closing company** (the default) | The windows open when you close the company, quit, or open another company |
| **Save current desktop** | Saves the windows open now; they reopen every time, whatever is open when you close |
| **Don't save the desktop** | Nothing: the company opens on the Home page |
| **Keep previously saved desktop** | Shown after you save a desktop: keeps it as it is |

On a Mac, **Window › Workspace** has the same choices in QuickBooks for Mac's words: **AutoSave**
(save when closing), **Manual** (keep the saved workspace) and **Save Workspace**.

The desktop is kept for each company file and, when the file has users, for each user, on this
computer. If a saved screen no longer exists, or a transaction in it was deleted, that window is
left out and the rest still open.

To open a company once without its windows, hold **Alt** (**Option** on a Mac) while you click
**Open**, or choose **File › Open Company Without Restoring Windows…**. The company opens with no
windows; your saved desktop is kept for next time unless you then close the company with **Save when
closing company** chosen.

### Choose the left pane and where you start

Also in **Edit › Preferences › Desktop View**, for this computer:

| Setting | Choices |
|---|---|
| **Left pane** | **Full menu** (the default): every page, grouped as Sales, Expenses, Banking and so on. **Open windows only**: just the windows open now, as in the Open Window List; open pages from the menu bar or the search box, and click **Menu** at the top of the pane to bring the full menu back. **Hidden**: no left pane, for the most room. **View › Show or Hide Left Icon Bar** switches between the full menu and hidden. |
| **Start on** | **Last open windows** (the default): the desktop chosen under **Desktop**, or the Home page when none is saved. **Home page**: the Home page alone. **This screen**: one screen you pick, such as Invoices or a register. **No windows**: an empty desktop; choose a page to begin. |

**Start on** takes effect the next time a company opens. Holding **Alt** (**Option** on a Mac) while
clicking **Open** still opens with no windows, whatever you chose.

## Start from the Home page

The **Dashboard** opens on its **Home** tab: a map of the everyday work, so you can see what comes
next and open it with one click.

- **Customers**, **Vendors** and **Employees** each show their forms joined by arrows in the order
  the work goes: an estimate becomes an invoice, the invoice is paid, the payment is deposited; a
  purchase order becomes a bill, the bill is paid, the check is printed.
- Click a step to open its form or list (**Invoices** opens a new invoice, **Pay Bills** opens Pay
  Bills). Tab moves between the steps and Enter opens one.
- A red number on a step says how much is waiting there: invoices overdue, bills coming due,
  payments not yet deposited and to-dos due. They are the same counts as in
  [Reminders](reminders-and-todos.md#see-your-reminders).
- Steps for features you have turned off are left out: Estimates when estimates are off, Purchase
  Orders when inventory is off (Preferences).
- **Banking** and **Company** on the right are shortcuts to checks, transfers, bank feeds,
  reconciling, registers, the chart of accounts, items, journal entries, the To Do list and
  reports.
- **Balances** lists your bank, credit card, receivable and payable accounts with their balances.
  Click one to open its register.

The **Insights** tab has the totals, income and expense for the last 12 months (by month, on the
basis set in Preferences, the same figures as the Income and Expense Graph), the cash forecast, the
assets and liabilities chart, the trial balance and Reminders. LocalBooks remembers which tab you last chose.

## Use the toolbar on a form

Invoices, sales receipts, estimates, credit memos, bills, checks and expenses, vendor credits,
purchase orders, deposits, transfers, journal entries and Receive Payment each have a row of large
buttons across the top. A form shows only the buttons it can use; a button it can't use right now
is greyed out, and hovering over it (or moving to it with the keyboard) says why.

| Button | What it does |
|---|---|
| **Find** | Opens **Find Transactions**. |
| **Previous** / **Next** | Opens the saved one before or after the one on the form: invoices by number, the others by date and then number. With nothing open, **Previous** opens the latest one. Save or clear your changes first. On invoices, bills, checks, expenses, card credits and journal entries, the saved one opens in the form to edit; a voided or paid invoice opens locked, and voided bills, checks, expenses and journal entries are skipped. Sales receipts, estimates, credit memos, refund receipts, vendor credits, purchase orders, deposits and transfers open read-only in the form's place, shown as they print, with their own Void, Print, Email and Attach File (and Apply, Refund, Create Invoice, Create Bill or Close PO where they fit); **New** goes back to a blank form. You can also open one from its list: double-click it, or choose **Open** in its row menu. |
| **New** | Clears the form for a new one. While you're editing a saved transaction, it closes the edit instead. |
| **Save** | Saves and posts, then clears the form for the next one (the same as **Save & New**). |
| **Void** | On a saved transaction that's open on the form, voids it with a reversing entry after you confirm. **Ctrl+D** presses it. |
| **Print ▾** | Prints or saves a PDF of the saved form. On a new form, **Post and print…** (or **Save and print…**) saves it first. On a saved check, **Print** prints the check. On a saved bill, expense, card credit, journal entry, credit memo, refund receipt, vendor credit, deposit or transfer, it prints a voucher: the header and the accounts it posted to, with debits and credits, and **Save as PDF…** saves that voucher (a check's too) as a PDF file. Saved sales receipts, estimates and purchase orders print as their own forms. |
| **Email ▾** | Emails the saved form, or marks it **Email later**. On a new form, **Post and email…** saves it first. On a saved voucher (bill, check, expense, credit, deposit, journal entry), **Email** sends its PDF to the vendor or customer named on it, by your mail app or from LocalBooks, as forms are sent. |
| **Attach File** | On a new form, picks files to attach when it posts. On a saved transaction that posted (anything but an estimate or purchase order), shows its attachments, where you can add more. |
| **Memorize** | Saves the form as a recurring transaction. **Ctrl+M** presses it. |

Press **Tab** to move through the buttons, or the **Left** and **Right** arrow keys to move along the
row. On **Print ▾** and **Email ▾**, press **Enter** or **Down** to open the menu.

## Work without a mouse

Every list, form and dialog can be used from the keyboard. **Tab** moves through a form's fields
and buttons in reading order, and through a line grid's columns in the order you chose (see
"Choose a list's columns"). In a list, each row's **▾** button opens its menu (**Open**,
**Edit…**, **Void**, **QuickReport** and the rest): Tab to it and press **Enter** or **Space**,
then use **↑** and **↓** (or **Home** and **End**) to choose and **Enter** to do it. **Esc** closes
a menu and puts you back on its button. The toolbar's menus (Print, Email, More) work the same way.

## Save a form and type faster with the keyboard

Every transaction form (invoice, sales receipt, estimate, credit memo and refund, bill, check,
expense and card credit, vendor credit, purchase order, deposit, transfer and journal entry) ends
in three buttons:

| Button | Key | What it does |
|---|---|---|
| **Save & Close** | **Alt+S** | Saves it and closes the form's window. A form you opened from inside a list (with **+ New** on the Bills list, say) closes back to that list. |
| **Save & New** | **Alt+N** | Saves it and leaves the form open and blank for the next one. After editing a saved transaction, you get a new blank form. |
| **Clear** | | Empties the form without saving. While editing a saved transaction, it closes the edit. |

If something is missing (no customer, lines out of balance), nothing is saved and the form says
what to fix. On a Mac, **Alt** is the **Option** key.

While a form is open:

| Shortcut | What it does |
|---|---|
| **Ctrl+D** | Void (or delete) the transaction on the form, after you confirm, as the toolbar's **Void** does. |
| **Ctrl+M** | Memorize the transaction (save it as a recurring one). |
| **Ctrl+Insert** | Insert a blank line above the line you're on. Outside the lines, it adds one at the end. |
| **Ctrl+Delete** | Delete the line you're on. |
| **Ctrl+N** | Start a new form of the same kind, as the toolbar's **New** does. |
| **Ctrl+P** | Print the form, on forms that print. |
| **Page Up** / **Page Down** | Open the previous or next one, as the toolbar's **Previous** and **Next** do. In a list or a text box with more lines they scroll it instead. On a saved transaction shown read-only (a sales receipt, estimate, credit, purchase order, deposit or transfer) they work too, as do **Ctrl+N**, **Ctrl+P** and **Ctrl+D**. |
| **Ctrl+L** | Open the list of the box you're in: a customer or vendor box, or an account, item or other drop-down. |
| **Ctrl+Alt+Y** | Copy Line: copy the line you're on. |
| **Ctrl+Alt+V** | Paste Line: insert a copy of the copied line above the line you're on (at the end from outside the lines). A line copied from an item grid pastes into item grids, an expense line into expense grids and a journal line into journal entries. |
| **Ctrl+G** | Go To Transfer: on a saved transfer, open the other account's register at the transfer. |
| **Ctrl+O** | Duplicate Transaction: on any saved transaction, start a new one copied from it (see below). |

Delete or Void, Memorize, Insert Line, Delete Line, Copy Line, Paste Line, Go To Transfer and
Duplicate Transaction are also in the **Edit** menu. **Ctrl+A** stays Select All. On a Mac, **⌘** works too for D, Insert and
Delete (Fn+Delete is Delete); **⌘M**, **⌘N**, **⌘P**, **⌘E**, **⌘G**, **⌘L** and **⌘O** stay the system's (⌘O opens
a company), so use **Control** with those letters (**Control+Option** for Copy Line and Paste Line).

### Duplicate a transaction

To enter one much like one you already have, open the saved one and press **Duplicate** on the
form's toolbar (**Ctrl+O**, or **Edit › Duplicate Transaction**), or choose **Duplicate** on its row
in the list. The form then holds a new, unsaved copy: the same name, lines, memo and terms, dated
today. An invoice and a journal entry get the next number when you save; a check takes the next
check number; a bill's own number is left blank for the vendor's, and its due date is as many days
after today as the original's was after its date. Change what you need and save it. The original
isn't changed. Duplicate is off while the form has changes you haven't saved.

Duplicate works on invoices, bills, checks, expenses, card credits and journal entries, and on
sales receipts, estimates, credit memos and refund receipts, vendor credits, purchase orders,
deposits and transfers. Those last ones open read-only; **Duplicate** on their toolbar (or row menu)
puts the copy in a new form. A sales receipt keeps its deposit account and payment method but not
its check number; a vendor credit leaves the vendor's reference blank for the new one; a deposit
brings its other lines and cash back, while the payments waiting in Undeposited Funds are ticked
afresh (each payment is deposited once).

In a register, click a transaction, then press **Ctrl+E** to open it on its form to edit, or
**Ctrl+G** to go to the other side of it: the register of the one other account it posted to (a
transfer's other bank account, a check's expense account). A transaction with several other
accounts (**-Split-**) says so instead.

### Date keys

Every date box, on forms and reports, takes these keys. Typing a date works as always.

| Key | Date |
|---|---|
| **t** | Today |
| **+** (or **=**) / **-** | The next day / the day before (hold it down to keep going) |
| **w** / **k** | The first / last day of the week (Sunday to Saturday) |
| **m** / **h** | The first / last day of the month |
| **y** / **r** | The first / last day of the year |

The week, month and year are those of the date in the box. In a box where a date can't be before
another (an ACH effective date, say), the keys stop at that date.

### Check numbers

In the **Check no.** box on **Write Checks** and **Pay Bills**, press **+** for the next number and
**-** for the one before. Leading zeros and letters in front stay as they are (A-0099 becomes A-0100).

## Find things in the menu bar

The menu bar is laid out the way desktop accounting programs usually are, so the commands you know
are where you expect them. [The menu bar](menus.md) lists every command and where it goes. Each one opens the screen, and often the form, in a window inside LocalBooks (or brings forward the window that already shows it):

| Menu | What's in it |
|---|---|
| **File** | New, open, close and back up company files; **Utilities › Import / Convert…** (the Desktop Migration Wizard), **Utilities › Export** (lists to an IIF file, names and addresses to a text file) and Condense Data; print and send forms |
| **Edit** | Undo, cut, copy and paste; **Delete or Void Transaction**, **Memorize Transaction**, **Insert Line**, **Delete Line**, **Copy Line**, **Paste Line**, **Go To Transfer** and **Duplicate Transaction** on the open form; Use Register; **Find…** (Find Transactions); Preferences |
| **View** | Show or hide the left icon bar (the left pane); Open Window List; One Window or Multiple Windows |
| **Lists** | Chart of Accounts, Item List, Sales Tax Codes, Classes, customers and vendors, the profile lists (sales reps, types, terms, messages, payment methods, Ship Via), price levels, vehicles, **Currency List** (the home currency and exchange rates, in Preferences › Multiple Currencies), form templates, memorized (recurring) transactions |
| **Company** | Home Page, To Do List, reminders, users and passwords, closing date, **Email Setup**, **AI Access (MCP)**, bank feeds, budgets, general journal entries, reclassify, clean up books, **Audit Log**, currency adjustment |
| **Customers** | Customer Center, estimates, invoices, sales receipts, statements, finance charges, **Invoice for Time & Expenses**, receive payments, credit memos and refunds, Change Item Prices |
| **Vendors** | Vendor Center, enter and pay bills, vendor credits, sales tax, purchase orders, 1099s |
| **Employees** | Weekly timesheet and single time activity |
| **Banking** | Write checks, credit card charges, registers, deposits, transfers, reconcile, bank feeds |
| **Reports** | Report Center and each report, grouped: Company & Financial, Customers & Receivables, Sales, Jobs, Vendors & Payables, Purchases, Inventory, Banking, Accountant & Taxes, Budgets, List; **Memorized Reports** (the Report Center's list, and Process Multiple Reports); **QuickReport** for the account, name or item selected in the list or register in front; **Transaction History** and **Transaction Journal** for the transaction open on a form. Every report in the Report Center is on this menu |

The classic shortcuts work from anywhere:

| Shortcut | Opens |
|---|---|
| **Ctrl+I** (⌘I) | Create Invoices |
| **Ctrl+W** | Write Checks (Windows and Linux; on a Mac ⌘W closes the window) |
| **Ctrl+R** (⌘R) | Use Register |
| **Ctrl+J** (⌘J) | Customer Center |
| **Ctrl+T** (⌘T) | Memorized Transaction List (Recurring Transactions) |
| **Ctrl+F** (⌘F) | Find Transactions |

**Ctrl+A** stays Select All, so open the Chart of Accounts from **Lists**. On Windows, press **Alt**
to reach the menus from the keyboard (for example **Alt+U** for Customers).

Next: [Create and send an invoice](invoices.md) · [Back up, restore and open company files](backups-and-company-files.md)
