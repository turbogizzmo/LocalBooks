# Preferences reference

Every setting in **Settings › Preferences Console** (also **Edit › Preferences**), category by
category: what it does, its default, and what it changes in your books. For a tour of the window
itself, see [Find your way around Preferences](getting-started.md#find-your-way-around-preferences).

Each category has up to two tabs:

- **My Preferences** are yours, on this computer. They change how the screens behave for you and
  never change what is posted. Each one takes effect at once, in every LocalBooks window.
  **Default** at the top of a panel puts that panel back as it was.
- **Company Preferences** are everyone's. They are saved in the company file as soon as you leave a
  box or pick a value (a ✓ Saved mark shows beside it), and only an Admin can change them.

A row marked **✓ Always** is something LocalBooks always does, shown where you would look for the
option. Settings that live on their own screen are listed at the bottom of a category under **Set
on their own screens**, with a link to each.

## Accounting

**My Preferences**

| Setting | Default | What it does |
|---|---|---|
| **Autofill memo in journal entries** | On | A line you start below the first one in Make General Journal Entries takes the first line's memo. Change it as you like. |

**Company Preferences: Accounts**

| Setting | Default | What it does |
|---|---|---|
| **First month of fiscal year** | January | Sets fiscal report periods (This Fiscal Year, and so on) and the day net income closes into Retained Earnings. |
| **Tax form** | Form 1040, Schedule C | The federal return your business files. It sets the deadlines in the tax calendar below and which tax lines accounts can use. |
| **Account numbers** | On | Lets you number accounts in the Chart of Accounts. Reports and account lists show the number before the name. |
| **Subaccounts** | Off | **Show lowest subaccount only**: the trial balance and general ledger show *Electric* instead of *Utilities:Electric*. |
| **Account on every line** | ✓ Always | Every line of every transaction needs an account before it can be saved, so nothing posts to an uncategorized account. |
| **Journal entry numbers** | ✓ Always | Each journal entry takes the next number when it posts. Numbers are never reused. |

**Company Preferences: Warnings**

| Setting | Default | What it does |
|---|---|---|
| **Retained Earnings** | On | Warns before you post a transaction straight to Retained Earnings. LocalBooks already moves each year's profit there. |
| **Dates in the past** | 90 days | Warns when a transaction's date is more than this many days before today. 0 turns the warning off. |
| **Dates in the future** | 30 days | Warns when a transaction's date is more than this many days after today. 0 turns the warning off. |

A warning asks before saving. **Save anyway** records the transaction as you entered it.

**Company Preferences: Class tracking**

| Setting | Default | What it does |
|---|---|---|
| **Use class tracking** | Off (on for a company that already has classes) | Adds a Class column to sales, purchase, deposit and journal entry forms. Manage the list in **Accounting › Classes**. |
| **Assign classes** | One to each line | Shown when class tracking is on. Choose **One to each line**, or **One to the entire transaction** for a single Class box at the top of the form. |
| **Warn when no class** | Off | Shown when class tracking is on. Asks before saving income or expense lines that have no class. |

See [Track classes](getting-started.md#track-classes).

**Company Preferences: Closing date and Tax calendar**

The **Closing date** panel sets the date on or before which nothing can be posted or voided, with an
optional passcode, and lists every change to it. See [Close a period](closing-the-books.md). The
**Tax calendar** lists the federal filing deadlines for your **Tax form** and fiscal year; dates that
fall on a weekend move to the next Monday. It doesn't include state deadlines, holidays or
extensions.

## Bills

**Company Preferences: Bills**

| Setting | Default | What it does |
|---|---|---|
| **Bills are due** | 10 days | Days after the bill date a bill is due when its vendor has no terms (0 means on receipt). A vendor's own terms always come first. |
| **Duplicate bill numbers** | On | Warns when a vendor's bill uses a number already on another of that vendor's bills. Two vendors may use the same number. |
| **Open bills** | On | Asks before you save a check or expense to a vendor who has unpaid bills, since a check doesn't pay those bills (Pay Bills does). |
| **Vendor credits** | Off | **Use vendor credits automatically**: Pay Bills uses the vendor's open credits on the bills you tick, oldest credit first, and pays the rest. |
| **Discounts** | Off | **Take early-payment discounts automatically**: Pay Bills takes the discount a bill's terms offer (such as 2% 10 Net 30) when you pay it by the discount date. |
| **Discount account** | None | The expense account discounts you take are credited to, such as Purchase Discounts or Cost of Goods Sold. |

In Pay Bills, **Set Discount** and **Set Credits** change what one bill takes. See
[Enter and pay bills](bills.md).

## Checking

**My Preferences: Open these forms with this account**

| Setting | Default | What it does |
|---|---|---|
| **Write Checks** | The first bank account | The bank account a new check starts with. |
| **Pay Bills** | The first bank account | The bank or credit card account Pay Bills starts with. |
| **Make Deposits** | The first bank account | The bank account a new deposit goes to. |

You can still pick another account on each form.

**Company Preferences: Checks**

| Setting | Default | What it does |
|---|---|---|
| **Duplicate check numbers** | On | Warns when a check number is already used in the same bank account. Voided checks don't count. |
| **Vendor account number** | Off | **Print the vendor's account number in the check memo**: a check saved without a memo gets "Acct #…" from the vendor's record. |
| **Check date** | Off | **Change the check date when a check is printed**: a check waiting in Print Forms, from Write Checks or Pay Bills, takes the date it prints. This is an edit, so the original is voided and kept in the audit log (for a Pay Bills check, each bill payment on it is voided and paid again on the print date). A check in another currency keeps its date, since its rate belongs to it. |

Renaming rules for downloaded payees are in **Banking › Bank Feeds › Rules…** (see
[Bank feeds](bank-feeds.md)).

## Desktop View

**My Preferences** (there are no company preferences here)

| Setting | Default | What it does |
|---|---|---|
| **View** | Multiple Windows | **One Window** shows one window at a time, filling the work area. **Multiple Windows** lets windows float inside LocalBooks, side by side, and minimize to the bottom. Kept with the company file. |
| **Desktop** | Save when closing company | What reopens with this company: **Save when closing company** (the windows open when you close it), **Save current desktop** (the windows open now, every time), or **Don't save the desktop** (the Home page alone). After you save one, it shows as **Keep previously saved desktop** until you choose again. |
| **Left pane** | Full menu | **Full menu** (every page, grouped), **Open windows only** (like the Open Window List) or **Hidden** (View › Show or Hide Left Icon Bar brings it back). |
| **Start on** | Last open windows | What a company opens on: **Last open windows**, the **Home page**, **This screen** (pick any page), or **No windows**. |

Holding Alt (Option on a Mac) while you click Open opens a company with no windows. See
[Work in more than one window](getting-started.md#work-in-more-than-one-window).

## Finance Charge

**Company Preferences: Finance charges**

| Setting | Default | What it does |
|---|---|---|
| **Annual interest rate** | 0% | The yearly rate, charged per day (a year counts as 365 days) on overdue balances. 0 means no finance charges. |
| **Minimum charge** | 0.00 | A smaller charge is raised to this amount. |
| **Grace period** | 0 days | Days past the due date before an invoice is charged. Interest still runs from the date below. |
| **Finance charge account** | None | The income account finance charge invoices credit. |
| **Calculate charges from** | Due date | Whether interest runs from the **Due date** or the **Invoice date**. |
| **Charge on finance charges** | Off | Whether overdue finance charge invoices themselves earn finance charges. |

Charges are assessed in **Sales › Assess Finance Charges**. See
[Charge interest on overdue invoices](customer-payments.md).

## General

**My Preferences**

The left side has **Typing**, **Date for new transactions** and **Sound**; the right side has
**Warnings**, **When the item on a sales line changes** and **Automatically recall information**.

| Setting | Default | What it does |
|---|---|---|
| **Pressing Enter moves between fields** | Off | Enter goes to the next box, as Tab does. Click a button (Save, Post) to record the transaction. In Preferences, Enter always moves on. |
| **Open the list of matches as you type** | On | Name boxes such as Vendor show matching names, and offer to add a new one, as you type. Off: press ↓ to open the list. |
| **Automatically place a decimal point** | Off | Amounts typed without a point are cents: 1234 is 12.34. Typing a point (12.) still means dollars. |
| **Show the full text of clipped entries on hover** | On | Text cut off with "…" in a list shows in full when you point at it. |
| **Date for new transactions** | Today's date | **Today's date**, or **The last date you entered** (handy for a stack of older paperwork). |
| **Beep when recording a transaction** | Off | A short sound each time a transaction is saved. **Test** plays it. |
| **Warn when editing a posted transaction** | On | Asks before saving changes to a posted transaction. Saving voids the original and posts the edited one. |
| **Warn when voiding a transaction** | On | Void Transaction asks before reversing it. |
| **When the item on a sales line changes** | Ask whether to keep it | If you typed your own description or price on a line and then change the item: **Ask whether to keep it**, **Always keep what I typed**, or **Never: use the new item's description and price**. |
| **Automatically recall information** | Don't recall anything | On a new bill, check or expense for a vendor you have paid before: **Recall the last transaction for this name** (accounts, descriptions and amounts), **Pre-fill the accounts used last time** (no amounts), or **Don't recall anything**. |

**Bring back all one-time messages** shows again every message you turned off with "Don't show this
message again".

**Company Preferences: Display**

| Setting | Default | What it does |
|---|---|---|
| **Years** | On | **Always show years as 4 digits** (10/13/2026). Off shows 10/13/26. |
| **Time format** | Minutes (1:30) | How hours show in time tracking and timesheets: **Minutes (1:30)** or **Decimal (1.50)**. You can type either way. |

**Company Preferences: Company information**

| Setting | Default | What it does |
|---|---|---|
| **Company name** | LocalBooks Ledger | Shown on reports, in the sidebar and in the window title. Can't be blank. |
| **Logo** | None | A PNG, JPEG, GIF or WebP picture under 512 KB, printed on invoices, estimates and reports. **Upload logo…**, **Replace…** or **Remove**. |
| **Legal name** | Blank | Your registered name, if it differs from the company name. |
| **EIN** | Blank | Your federal employer identification number (XX-XXXXXXX), used on 1099s and tax forms. |
| **Address** | Blank | Printed on forms, statements and envelopes. |
| **Email** | Blank | Your company's email address. |
| **Phone** | Blank | Your company's phone number. |

**Company Preferences: Automatic backups**

| Setting | Default | What it does |
|---|---|---|
| **Daily backup** | On | **Back up the company file automatically** when LocalBooks starts, at most once a day. |
| **Keep** | 7 | How many daily backups to keep (1 to 60). Older ones are removed. |

See [Back up, restore and open company files](backups-and-company-files.md). Signing out after a
period of no use is in **Settings › Users & Passwords**.

## Items & Inventory

**Company Preferences: Inventory and purchase orders**

| Setting | Default | What it does |
|---|---|---|
| **Inventory** | On | **Inventory and purchase orders are active**. Off hides Purchase Orders, Receive Items and stock tracking on products. Items already tracked keep their stock. |
| **Item categories** | On | **Group items into categories**. Off hides the category tree on Products & Services and the category filters on forms and item reports. Items keep their categories. |
| **Not enough stock** | Warn | When an invoice or sales receipt would take a tracked item below zero: **Warn**, **Don't allow the sale**, or **Say nothing**. |
| **Purchase order numbers** | ✓ Always | Each purchase order takes the next number, so no two share one. |

Units of measure aren't in LocalBooks: each item has one unit. See [Track inventory](inventory.md).

## Jobs & Estimates

**Company Preferences**

| Setting | Default | What it does |
|---|---|---|
| **Estimates** | On | **Create estimates**. Off hides Estimates from the menus, the sidebar and New. Saved estimates are kept. |
| **Estimate numbers** | ✓ Always | Each estimate takes the next number, so no two share one. |
| **Job status names** | Pending, Awarded, In progress, Closed, Not awarded | How each of the five job statuses reads on jobs and in the customer list. Rename them to suit your work. |

Progress invoicing (invoicing an estimate in parts) isn't available yet: an estimate becomes one
invoice, which you can change before saving. See
[Sales receipts and estimates](sales-receipts-and-estimates.md).

## Multiple Currencies

**Company Preferences: Currencies**

| Setting | Default | What it does |
|---|---|---|
| **Home currency** | USD | The currency every report and balance is in. It locks once a foreign-currency transaction exists. |
| **Currency symbol** | $ | How amounts are shown. Display only: amounts are never converted. |
| **Exchange rates** | Off | **Look up daily exchange rates**: suggests the European Central Bank's rate for a foreign-currency transaction's date. Only the two currency codes and the date are sent; rates are saved in the company file, so each date is looked up once. You can always type your own rate. |

The **Currency List** (Lists › Currency List) opens here too. See
[Revalue foreign-currency invoices](currency-revaluation.md).

## Payments

**Company Preferences: Receive Payments**

| Setting | Default | What it does |
|---|---|---|
| **Apply payments** | An exact match, else oldest first | How an amount received is spread over the customer's open invoices: **Automatically: an exact match, else oldest first**, **Automatically: oldest invoices first**, or **Not automatically: I tick the invoices**. Auto Apply on the form always applies. |
| **Calculate the amount** | On | The amount received becomes the total of the invoices you tick. |
| **Deposit to** | On | **Use Undeposited Funds as the usual deposit account**. Off: payments and sales receipts go to the first bank account unless you choose another. |

**Company Preferences: Early-payment discounts**

| Setting | Default | What it does |
|---|---|---|
| **Discounts** | Off | **Take customers' early-payment discounts automatically** when an invoice is paid in full by its discount date. Off: type the discount on the invoice's row. |
| **Discount account** | None | Where discounts customers take are debited, usually an income account such as Sales Discounts. Without one, Receive Payments takes no discounts. |

See [Record customer payments](customer-payments.md).

## Reminders

Reminders has no settings of its own here. Which reminders show, how many days ahead each one
looks, and whether they open with the company are set with **Customize** in the Reminders panel
(Dashboard › Insights). See [Choose your reminders](reminders-and-todos.md#choose-your-reminders).

## Reports & Graphs

**My Preferences**

| Setting | Default | What it does |
|---|---|---|
| **Show the report's options before running it** | Off | A report you open from the list first asks for its dates (and basis, for Profit and Loss); click **Run report** to see it. Reports opened from a graph or a link run at once. |

**Company Preferences: Report defaults**

| Setting | Default | What it does |
|---|---|---|
| **Summary reports basis** | Accrual | The basis Profit and Loss and the Income & Expense graph open on. Each report can still switch. The Balance Sheet is always accrual. |
| **Aging reports** | Age from due date | Whether A/R and A/P Aging count days from the due date or the transaction date. |
| **Items** | Name only | How items show in Sales by Item, Inventory Valuation and Stock Status: **Name only**, **Description only**, or **Name and description**. |

**Company Preferences: Statement of Cash Flows**

Lists every balance sheet account with the section its change goes in: **Operating** (receivables,
current assets, payables, credit cards and current liabilities), **Investing** (fixed and other
assets) or **Financing** (long-term liabilities and equity). Change an account's section from its
drop-down; **Revert to defaults** puts them all back. Bank accounts are the cash itself, and income
and expense accounts are in net income. See [Run, export and print reports](reports.md).

## Sales & Customers

**Company Preferences: Sales forms**

| Setting | Default | What it does |
|---|---|---|
| **Usual shipping method** | None | Filled in as Ship Via on new invoices, sales receipts and estimates. Add methods in **Sales › Reps, Ship Via & Fields**. |
| **Usual F.O.B.** | Blank | Filled in as F.O.B. on new invoices, sales receipts and estimates. Change it on any form. |
| **Default markup** | 0% | The markup **Change Item Prices** starts with when it works new prices out from cost. |
| **Invoice numbers** | ✓ Always | Each invoice takes the next number, so no two share one. |

**Company Preferences: Pricing, Warnings, Estimates to invoices and Sales orders**

| Setting | Default | What it does |
|---|---|---|
| **Price levels** | On | **Use price levels**. Off hides price levels on customers and forms, and every form uses standard prices. Levels you made are kept. |
| **Overdue customers** | Off | Asks before saving a new invoice or sales receipt for a customer or job with invoices past due. |
| **Open invoices** | On | Asks before saving a sales receipt for a customer with open invoices, since a receipt doesn't pay them (Receive Payments does). |
| **Zero-amount lines** | On | **Don't put zero-amount lines on the invoice**: an estimate line priced at 0.00 (an included extra) is left off the invoice the estimate becomes. Off: it comes over and must be priced. |
| **Sales orders** | On | **Use sales orders**. Off hides Sales Orders from the menus and the sidebar. Saved orders and their invoices are kept. |
| **Sales order numbers** | ✓ Always | Each sales order takes the next number, so no two share one. |

See [Create and send an invoice](invoices.md) and [Sales orders](sales-orders.md).

## Sales Tax

**Company Preferences: Sales tax**

| Setting | Default | What it does |
|---|---|---|
| **Owe sales tax** | As of invoice date (accrual) | When you owe the tax: **As of invoice date (accrual)** or **Upon receipt of payment (cash)**. The sales tax report and what it says you owe follow this. |
| **File returns** | Quarterly | **Monthly**, **Quarterly** or **Yearly**. Sets the sales tax report's period and a reminder on the 20th after each period ends (check your state's due date). |
| **Taxable lines** | On | **Mark taxable amounts with a "T" when printing** on invoices, sales receipts and estimates. |

Turning sales tax on, rates, combined rates and agencies are in **Sales › Sales Tax**. See [Sales tax](sales-tax.md).

## Send Forms

**Company Preferences: Sending forms**

| Setting | Default | What it does |
|---|---|---|
| **New customers** | None | A new customer's preferred delivery: **None**, **Email (marked Email later)** or **Print (marked Print later)**. It ticks Email later or Print later on their new forms; change it on each customer. |

How forms are sent is in **Settings › Email**; how they look, and each form's email subject and
message, in **Settings › Custom Forms**. See [Design your invoices and estimates](custom-forms.md).

## Tax: 1099

**Company Preferences: 1099 forms**

| Setting | Default | What it does |
|---|---|---|
| **Do you file 1099s?** | On | **Yes, track payments for 1099 forms**. Off hides the 1099 checkbox on vendors, the 1099 reports and the 1099 deadline. |
| **Accounts to boxes** | Nothing mapped | Which 1099 box each expense account's payments count toward. With nothing mapped, every payment to a 1099 vendor counts as nonemployee compensation (1099-NEC box 1). |

Thresholds follow the payment year. See
[1099 boxes and thresholds](getting-started.md#1099-boxes-and-thresholds).

## Time & Expenses

**Company Preferences: Time tracking**

| Setting | Default | What it does |
|---|---|---|
| **First day of work week** | Monday | The day weekly timesheets start on. |
| **Billable time** | Off | **Mark all time entries as billable**: new time entries and timesheet rows start with Billable ticked. |

**Company Preferences: Billable expenses**

| Setting | Default | What it does |
|---|---|---|
| **Billable expenses** | Off | **Mark all expenses as billable**: expense, check and bill lines given a customer start with Bill? ticked. |
| **Reimbursed expenses** | Off | **Track reimbursed expenses as income**. Off: billing a cost to the customer credits the expense account back. On: it is income, in the account below or the one named on the expense account. |
| **Reimbursement income** | None | The income account reimbursements go to when they count as income. |
| **Default markup** | 0% | Percent added to billed costs. Bill Costs can change it each time. |
| **Markup account** | None | The income account the markup is credited to. |
| **On invoices** | On | **Ask about billable costs and time when I choose a customer**: a new invoice offers the customer's or job's unbilled costs and time. Off: no question; **Add billable costs…** on the invoice still adds them. |

See [Bill billable expenses to a customer](getting-started.md#bill-billable-expenses-to-a-customer)
and [Track time](time-tracking.md).
