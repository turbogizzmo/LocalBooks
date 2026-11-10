# What's new in LocalBooks

The newest changes first, in plain words, with a link to the steps for each. LocalBooks offers each
new version when it starts (or check in **Settings › Updates**). The full list, including fixes, is in
the [release notes](https://github.com/turbogizzmo/LocalBooks/blob/main/CHANGELOG.md).

## 0.14.0

A large release: everything below, plus over a hundred smaller changes in the release notes.

- **Sales orders.** Record what a customer has ordered before you ship it, then invoice it all at
  once or in parts. An accepted estimate becomes a sales order, and sales orders print and email
  like invoices. [Sales orders](sales-orders.md)
- **Receive items before the bill.** Bring goods in from a purchase order when they arrive and
  enter the bill later; each line's job carries over to the bill.
  [Receive items before the bill](bills.md#receive-items-before-the-bill)
- **Discounts, subtotals and charges on every purchase form and on credit memos.**
  [Discounts and charges](bills.md#discounts-and-charges-on-bills-checks-credits-and-orders)
- **Approvals for bills and purchase orders.** [Approvals](bills.md#approvals-for-bills-and-purchase-orders)
- **Payouts from Stripe, PayPal and Shopify Payments**, with their fees, fetched on a schedule if you
  like. [Payouts from PayPal](bank-feeds.md#payouts-from-paypal)
- **Company Snapshot**: a page of widgets you choose for income, expenses, balances and to-dos.
  [Company Snapshot](reports.md#company-snapshot)
- **Work with your accountant**: an Accountant's Copy with a dividing date, and an External
  Accountant user. [Accountant's Copy](closing-the-books.md#work-with-your-accountant-at-the-same-time-accountants-copy)
- **Merge duplicate customers, vendors, items and accounts.**
  [Merge two customers, vendors or items](customers-and-products.md#merge-two-customers-vendors-or-items)
- **The Document Inbox** for receipts and bills, and the **Attachments Library**.
  [The Document Inbox](attachments.md#the-document-inbox)
- **Encrypted offsite backups**, and a reminder when a backup hasn't left this computer.
  [Offsite backup](backups-and-company-files.md#offsite-backup-encrypted)
- **Go back to the previous version** from Help if a new version gives you trouble.
  [Go back to the previous version](troubleshooting.md#go-back-to-the-previous-version)
- **Safer company-file imports**: payroll, cost of goods sold and discount lines come in from
  QuickBooks Desktop files, and amounts from a damaged file are checked.

## 0.13.0

- **Take a customer's early-payment discount on Receive Payments.** An invoice on terms such as
  *2% 10 Net 30* that's paid by the discount's last day shows the discount it offers, and ticking it
  takes the discount along with the payment. Choose the discount account in Preferences › Payments.
  [Take a customer's early-payment discount](customer-payments.md#take-a-customers-early-payment-discount)
- **Every report is in the Reports menu**, with a **List** submenu for the list reports and
  **Memorized Reports** for Process Multiple Reports. [Reports](reports.md)
- **Ctrl+N, Ctrl+P and Page Up / Page Down on forms** start a new form, print it, or open the
  previous and next one. [Save a form and type faster](getting-started.md#save-a-form-and-type-faster-with-the-keyboard)
- **Fixes:** a customer's or vendor's transactions panel totals one type at a time, the "Saved
  customer" note clears when you move on, and a damaged company file no longer closes LocalBooks
  while unlocking.

## 0.12.0

- **Preferences, laid out by category.** Settings › Preferences lists Accounting, Bills, Checking,
  General, Sales & Customers, Sales Tax and the rest, each with **My Preferences** and **Company
  Preferences**: report basis, sales tax on the cash basis, 1099 boxes, inventory switches, check
  defaults and more. [Find your way around Preferences](getting-started.md#find-your-way-around-preferences)
- **Warnings before saving, with Save anyway**, for a duplicate check or bill number, a date far
  in the past or future, or a customer over their credit limit.
  [Warnings before saving](getting-started.md#warnings-before-saving-company-preferences)
- **Edit an invoice or bill that's already part paid.** Its payments and credits stay applied to
  the corrected version. [Fix a mistake](invoices.md#fix-a-mistake)
- **Bill To, Ship To, P.O. No. and FOB on sales forms**, and a fuller customer record with a credit
  limit. [Credit limits](invoices.md#credit-limits)
- **Subtotals, discounts, other charges and groups** on invoices, sales receipts and estimates.
  [Subtotals, discounts, charges and groups](invoices.md#subtotals-discounts-charges-and-groups)
- **Save & Close, Save & New and date keys** on every form: **t** for today, **+** and **-** for a
  day. [Save a form and type faster](getting-started.md#save-a-form-and-type-faster-with-the-keyboard)
- **Type checks and deposits straight into a register**, print checks and invoices later in a
  batch, record a bounced check and pay down a credit card.
  [Type a check or deposit into a register](expenses-and-checks.md#type-a-check-or-deposit-into-a-register)
- **Terms, Payment Method and customer messages lists**, more reports (phone and contact lists,
  Audit Trail, Process Multiple Reports) and **classes kept when you import from QuickBooks**.
  [Terms](customers-and-products.md#terms)
- **Two defaults changed:** sales receipts now start on Undeposited Funds, and a new bill is due
  after the vendor's terms (10 days if the vendor has none) instead of always 30 days. Both are in
  Preferences. [How payments are applied](customer-payments.md#how-payments-are-applied-preferences--payments)

## 0.11.0

- **A toolbar on every form.** Find, New, Save, Void, Print, Email, Attach File and Memorize across
  the top of invoices, bills, checks and the other forms, and payment-method tiles on Receive
  Payment and sales receipts. [Use the toolbar on a form](getting-started.md#use-the-toolbar-on-a-form)
- **Add a customer, vendor, item or class without leaving the form.** Type a new name and choose
  **Add as a new customer** (or vendor); the item column ends in **＋ Add new item…** and the class
  list in **＋ Add new class…**.
  [Add a customer or vendor](customers-and-products.md#add-a-customer-or-vendor)
- **See a name's transactions, and open anything from a register.** Click a customer or vendor to
  list its transactions; double-click a register row or a list row to open it.
  [See a customer's or vendor's transactions](customers-and-products.md#see-a-customers-or-vendors-transactions)
- **Sort registers and Reconcile**, and **press Enter to move between fields** (Settings ›
  Preferences › General). [Set how typing works](getting-started.md#set-how-typing-works-general-preferences)
- **Six more reports**, including Profit & Loss by Month, Transaction List by Customer and by
  Vendor, and Voided Transactions. [Detail reports](reports.md#detail-reports)

## 0.10.3

- **Windows inside LocalBooks, as in QuickBooks Desktop.** Invoices, bills, registers and reports
  open as windows you can move, resize, minimize and line up (**Window › Cascade**, **Tile
  Vertically**). **View › One Window** shows one screen at a time instead, and **View › Open
  Window List** lists what's open. [Work in more than one window](getting-started.md#work-in-more-than-one-window)
- **Your windows come back.** The windows open when you close a company reopen with it. With
  users on, each person's windows are their own.
  [Reopen your windows with the company](getting-started.md#reopen-your-windows-with-the-company-saved-desktop)
- **Choose the left pane and where you start.** Show the full menu, only the open windows, or no
  left pane, and start on Home, your last windows or a screen you choose.
  [Choose the left pane and where you start](getting-started.md#choose-the-left-pane-and-where-you-start)
- **This page, and a guide to attachments.** [Attach receipts and documents](attachments.md)

## 0.10.2

- **Pay Bills writes checks.** Paying bills from a bank account writes one numbered check per
  vendor, for all their bills together, and can print them. Write Checks carries on from the same
  numbers, so a check number is never used twice.
  [Pay one or more bills](bills.md#pay-one-or-more-bills)
- **Attach the vendor's invoice.** Keep the vendor's invoice with the bill, the receipt with the
  expense, and documents with invoices, sales, deposits and journal entries. A 📎 marks the
  transactions that have files. [Attach receipts and documents](attachments.md)
- **Click a report to see the details (QuickZoom).** Click an amount on the Profit and Loss or
  Balance Sheet to see the transactions behind it, a name on an aging report for its open invoices
  or bills, or a line on a sales or vendor summary for its detail report.
  [See the transactions behind a number](reports.md#see-the-transactions-behind-a-number-quickzoom)
- **Early payments.** A customer payment can be dated before its invoice, and a bill can be paid
  before its date. [Record customer payments](customer-payments.md#pay-one-invoice)
- **Newest first.** Lists of transactions start with the newest at the top, and each list
  remembers the order you choose. [Sort a list](getting-started.md#sort-a-list)
- **Clearer help when the books won't open.** If the company file can't be opened at launch,
  LocalBooks says why in plain words, with **Try again** and **Check for updates**.
  [Troubleshooting](troubleshooting.md#localbooks-couldnt-open-your-company-file)

## 0.10.1

- **Mileage.** Record vehicle trips, set your mileage rates, and bill trips to customers.
  [Track vehicle mileage](mileage.md)
- **Settings in the sidebar.** Expand **Settings** in the sidebar to reach each part directly; the
  migration wizard is **Settings › Import & Migration**.

## 0.10.0

- **Clean Up Books.** The Client Data Review tools: apply unapplied credits, clear Undeposited
  Funds, write off small balances, review list changes. [Clean up the books](cleaning-up-books.md)
- **Graphs.** Income and expense, net worth, receivables, payables and sales charts; click a bar
  for the report behind it. [Graphs](reports.md#graphs)
- **Reminders and To Do.** What needs attention today, and your own to-dos with due dates.
  [Reminders and the To Do list](reminders-and-todos.md)
- **Price levels.** Special prices for some customers, filled in on their sales forms.
  [Price levels](customers-and-products.md#price-levels)
- **Condense Data.** Replace closed years with monthly summaries, keeping a full archive.
  [Condense data](backups-and-company-files.md#condense-data-archive-closed-years)
- **QuickBooks for Mac 2010–2012 files**, and a preview of exporting from a QuickBooks that is
  still installed. [Moving your books](desktop-migration.md)

## 0.9.0

- **Menu bar.** The familiar desktop menus (Lists, Company, Customers, Vendors, Banking, Reports)
  and shortcuts such as Ctrl+I for a new invoice. [Find things in the menu bar](getting-started.md#find-things-in-the-menu-bar)
- **New Company setup.** Pick your industry and get a suggested chart of accounts.
  [Create a new company](getting-started.md#create-a-new-company)
- **Find Transactions** by text, date, number, amount, name or account.
  [Find a transaction](journal-entries-and-voids.md#find-a-transaction)
- **Sort any list** by clicking a column heading. [Sort a list](getting-started.md#sort-a-list)

## 0.8.0 and 0.8.1

- **Customize reports**, and about 24 new detail reports you can filter and memorize.
  [Customize a report](reports.md#customize-a-report)
- **Job costing**: charge costs to a customer or job and see each job's profit.
  [Job costing reports](reports.md#job-costing-reports)
- **Accountant & Taxes reports**, including Adjusting Journal Entries and Income Tax Detail.
  [Accountant & Taxes reports](reports.md#accountant--taxes-reports)
- **Open QuickBooks files directly** from **File › Open or Restore Company…**.
  [Open a QuickBooks company file](backups-and-company-files.md#open-a-quickbooks-company-file-or-backup)
