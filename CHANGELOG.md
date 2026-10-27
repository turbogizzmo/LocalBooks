# Changelog

What changed in each LocalBooks release. A release's section here becomes its release notes on
GitHub and in the app's update window (see `.github/workflows/release.yml`), so write it for the
people using LocalBooks.

## [0.10.2]

- **Early payments.** A customer payment can be dated before its invoice (a customer who paid
  early, or a deposit taken before you billed the work), and a bill can be paid before its date,
  as QuickBooks allows.
- **Check numbers and printing in Pay Bills.** Paying bills from a bank account writes one check
  per vendor, numbered from the account's next check number, and can print them. Write Checks
  continues the same numbering, so a number is never used twice.
- **Attach the vendor's invoice to a bill.** Attach files to a new bill, expense or check before
  you post it, or to any posted bill, expense, invoice, sale or deposit from its row menu. A 📎
  marks rows with files, and attached files now stay with a transaction when you edit it.
- **Click a report to see the details (QuickZoom).** Click an amount on the Profit and Loss or
  Balance Sheet to open the account's transactions for that column's dates. Click a name on an
  aging summary to list its open invoices or bills, or a line on Sales by Customer, Sales by
  Product/Service or Expenses by Vendor Summary to open its detail report.
- **Newest first.** Lists of transactions start with the newest at the top, and each list
  remembers the order you choose.
- **Clearer help when the books won't open.** If LocalBooks can't open your company file at
  launch, it now says in plain words what usually causes it and what to do, with **Try again** and
  **Check for updates** buttons.

## [0.10.1]

- **Mileage tracking.** **Employees › Mileage**: add vehicles and the mileage rates you use (with
  the dates they start), then record trips by odometer or total miles. Bill trips to a customer or
  job on an invoice, and see Mileage by Vehicle and Mileage by Job reports with the deductible
  amount.
- **Settings in the sidebar.** **Settings** now opens like the other groups, with each section
  listed. The Desktop Migration Wizard moved to **Settings › Import & Migration**, and **File ›
  Utilities › Import / Convert…** opens it too.
- **Fixed:** on Windows, **Condense Data** and **Restore from Backup** could stop with "cannot
  remove … being used by another process" and leave your books as they were. They now work
  reliably.

## [0.10.0]

- **Clean Up Books.** **Accounting › Clean Up Books** gathers QuickBooks Accountant's cleanup tools:
  apply leftover credit memos and vendor credits to open invoices and bills, clear payments stuck in
  Undeposited Funds, write off old invoices in a batch, review list changes for a date range, and
  compare inventory with the Balance Sheet. Every fix shows its totals and asks before posting.
- **Graphs.** **Reports › Graphs**: Income and Expense, Net Worth, Accounts Receivable, Accounts
  Payable, Sales, and Budget vs. Actual. Each uses the same numbers as its report; click a bar or
  slice to open the detail.
- **Reminders and To Do.** A **Reminders** panel on the Dashboard (and **Company › Reminders**) shows
  overdue invoices, bills coming due, to-dos, recurring transactions due, emails waiting to send,
  upcoming tax dates and an overdue backup. Choose what it shows in Settings. Add to-dos from
  **+ New › To Do** or from a customer or vendor.
- **Price levels.** Give customers their own prices: a fixed percentage up or down on everything,
  or set prices per item. Set a customer's default level, and sales forms fill in the right rate
  (a rate you type always wins). The **Item Price List** report shows every level. Price levels
  come in from QuickBooks and IIF imports.
- **Condense Data.** **File › Utilities › Condense Data…** keeps your company file lean: it saves
  a verified **archive** with the full history, then replaces closed years with monthly summary
  entries, keeping anything still open (unpaid invoices and bills, uncleared bank lines). Your
  balances, aging and inventory are checked to match before the condensed file is used, and the
  archive opens read-only any time. Admins only; a closing date is required.
- **QuickBooks for Mac 2010–2012 files** import directly in the Desktop Migration Wizard.
- **Export from an installed QuickBooks (preview).** If QuickBooks Desktop still runs on your
  computer, the Desktop Migration Wizard can ask it for your company's data (AppleScript on a Mac,
  Intuit's QuickBooks SDK on Windows). LocalBooks only reads; it never changes anything in
  QuickBooks. This hasn't been tried against every QuickBooks version yet, so tell us how it goes.

## [0.9.0]

- **QuickBooks-style menu bar.** New **Lists, Company, Customers, Vendors, Employees, Banking** and
  **Reports** menus (plus **View**, and more items under **File** and **Edit**) open each screen and
  entry form, and **Reports** lists every report by group, as in QuickBooks Desktop. The classic
  shortcuts work: **Ctrl+I** invoice, **Ctrl+W** write checks (Windows and Linux), **Ctrl+R**
  register, **Ctrl+J** Customer Center, **Ctrl+T** memorized transactions and **Ctrl+F** Find
  (⌘ on a Mac). On Windows, **Alt** reaches the menus from the keyboard. **Window › New Window** is
  now on every platform.
- **New Company setup assistant.** **File › New Company…** asks for your company details and tax
  form, then suggests a chart of accounts for your industry from the same 33 industries QuickBooks
  offers, numbered QuickBooks-style, with equity accounts that follow your tax form and optional
  sales tax and inventory accounts.
- **Find Transactions.** **Accounting › Find Transactions** (or **Edit › Find…**, or **Advanced
  search…** under the search box) filters by text, dates, number, amount, transaction type, customer
  or vendor, and account.
- **Sort any list** by clicking a column heading; click again to reverse it.
- **Saved and emailed forms are named after the customer**, for example `Invoice 1001 · Acme.pdf`,
  and **Save as PDF** in the print dialog suggests the same name.
- **Open a company without restoring windows**: hold **Option** or **Alt** while clicking **Open**,
  or choose **File › Open Company Without Restoring Windows…**. The company opens on a plain Home
  page with other windows closed, which helps when a page won't load.
- Includes 0.8.1: open a QuickBooks `.qbw`, `.qbb` or `.qbm` file directly from **File › Open or
  Restore Company…**.

## [0.8.1]

- **Open QuickBooks files directly.** **File › Open or Restore Company…**, **Open Company File…**
  and **Restore from Backup…** now let you pick a QuickBooks `.qbw` company file, `.qbb` backup or
  `.qbm` portable file, where before only LocalBooks files could be chosen. LocalBooks creates a new
  company named after the file and opens the Desktop Migration Wizard on it. The QuickBooks file is
  only read.
- **Settings › File & Data Controls** has a **Desktop Migration Wizard…** button, so the QuickBooks,
  Quicken and GnuCash import is next to Open and Restore as well as under **Accounting**.
- QuickBooks files named in capitals (such as `Company.QBB`) can be chosen in every file picker,
  including the Desktop Migration Wizard's.

## [0.8.0]

- **Customize reports.** The new detail reports have a **Customize…** button: choose the dates,
  accounts, names, transaction types, class, item and more; pick and order the columns; and group or
  subtotal by account, name, item, class, type or period. Click any line to open its transaction.
  **Save as…** keeps a customized report under its own name (a memorized report) to run again.
- **About 24 new detail reports**, matching QuickBooks: Profit and Loss Detail, Balance Sheet Detail,
  A/R and A/P Aging Detail, Open Invoices, Unpaid Bills, Customer and Vendor Balance Summary and
  Detail, Sales by Customer and by Product/Service Detail, Income by Customer, Expenses by Vendor
  Detail, Purchases by Vendor and by Item, Check Detail, Deposit Detail, 1099 Detail, Transaction
  List by Date and Journal. All of them export to Excel, CSV and PDF.
- **Job costing.** Bill, expense, check, card and vendor credit lines have a new **Customer:Job**
  column, so costs can be charged to a job. **Reports › Jobs** has Job Profitability, Job Estimates
  vs. Actuals, Item Profitability, Item Estimates vs. Actuals, Profit and Loss by Job, Estimates by
  Job, and Progress Invoices vs. Estimates. Time isn't costed yet, because time entries have no cost
  rate.
- **Accountant & Taxes reports.** Mark a journal entry as an **Adjusting entry** and see them in the
  Adjusting Journal Entries report. Reprint any past reconciliation (Previous Reconciliation Detail
  and Summary), and see what changed after it with Reconciliation Discrepancy (for reconciliations
  you finish from now on). Assign **tax lines** to accounts (Schedule C, 1065, 1120-S, 1120) for the
  Income Tax Summary and Detail; QuickBooks IIF imports bring them in where the file names them.
  Also new: the **Collections** report and **Open Purchase Orders**.
- The tester page has a step-by-step **Move your books** guide: pick the program you're leaving and
  see just your steps.

## [0.7.0]

- **Export to Excel.** Every report, register, list and the audit log can now be saved as an Excel
  workbook, laid out like QuickBooks' own export: real numbers and dates, bold totals, frozen
  headers, and indented accounts.
- **Import PDF bank statements.** Bank Feeds can read the transaction table from a PDF bank or card
  statement. You confirm the columns, and a balance check shows whether the statement adds up. PDFs
  that are only scanned images can't be read.
- **Import from GnuCash.** The Desktop Migration Wizard reads GnuCash books (XML or SQLite) into
  LocalBooks, including customers, vendors, invoices and bills.
- **Smarter Bank Feed suggestions.** When no rule matches, LocalBooks suggests a category and payee
  from similar past transactions, with a confidence level and the transaction it looked like.
  Nothing is approved automatically, and nothing leaves your computer.
- **Daily exchange rates (optional).** Turn on **Look up daily exchange rates** (Settings ›
  Preferences Console › Accounting & tax) to have foreign-currency rates filled in from the European
  Central Bank. Only the currency codes and the date are sent. A rate you type, or one already
  saved, is never replaced.
- **Pay bills by ACH file.** In Pay Bills, choose **Pay by ACH file** to record the payments and
  save a NACHA file to upload to your bank. Vendor bank details are encrypted like tax IDs. Check
  your first file with your bank before relying on it.
- **License notices.** Help now lists the open-source components LocalBooks uses. QuickBooks® is a
  registered trademark of Intuit Inc.; LocalBooks is independent and not affiliated with, endorsed
  by, or sponsored by Intuit.
- Behind the scenes: every release is now checked automatically on macOS, Windows and Linux before
  it's offered as an update. The checks cover the update signatures, upgrading company files from
  the previous version, and launching the app.

## [0.6.0]

- **Import QuickBooks reports.** No working QuickBooks? Every QuickBooks (Desktop for Windows,
  Desktop for Mac and Online) can still export its **Journal** or **General Ledger** report and its
  account, customer, vendor and item lists to Excel or CSV, even in view-only mode. Drop those files
  into the Desktop Migration Wizard and LocalBooks rebuilds your books from them. See the guide for
  the exact report settings.
- **Moving from QuickBooks for Mac.** The migration guide now explains how to bring Mac books over:
  export a copy for QuickBooks for Windows, with no password, and import that.
- **Credit card credits.** Enter a vendor's refund onto a credit card (Expenses & Checks › Card credit,
  or + New). Card credits in QuickBooks, IIF and QIF imports now come in as card credits instead of
  journal entries.
- **Finance charges.** Set a rate, minimum charge and grace period (Settings › Preferences Console ›
  Finance charges), then use **Assess Finance Charges** to add late fees to overdue invoices.
- **Change Item Prices.** Raise or lower the prices of many products and services at once, by an
  amount or a percentage, with a preview before you apply. Every change is in the audit trail.
- **Sales reps, Ship Via and custom fields.** Optional lists for sales forms: a sales rep per
  customer and document (with a Sales by Rep Summary report), shipping method and ship date, and up
  to 15 custom fields for customers, vendors and items, which can print on your forms. They come in
  from QuickBooks and IIF imports too. Nothing changes until you add an entry.
- **New reports:** Profit and Loss and Balance Sheet Previous Year Comparison, Balance Sheet Budget
  vs. Actual (budgets can now include balance sheet accounts), Inventory Valuation Summary and
  Detail, Inventory Stock Status by Item, and Missing Checks.
- **Fixed:** on macOS, "Are you sure?" questions (such as making a new recovery key or deleting a
  budget) cancelled themselves without asking. They now ask inside LocalBooks.
- **Fixed:** editing an invoice that had sales tax, after sales tax was turned off, removed its tax.
- **Fixed:** a View only or Standard user could briefly see Admin settings when LocalBooks reopened
  on the Settings page. Changes were always refused.

## [0.5.1]

- **Tax IDs from newer QuickBooks files.** QuickBooks keeps vendor tax IDs encrypted with its own
  key, which LocalBooks can't read. Books imported before 0.5.0 could show that unreadable text as a
  vendor's tax ID (and on the 1099 report). It is now removed automatically when you open the file,
  or at your next sign-in if users are on. Re-enter those IDs from the vendors' W-9 forms. Real tax
  IDs and notes are never touched.

## [0.5.0]

- **Users and passwords (optional).** Turn them on in Settings › Users & Passwords: everyone signs
  in with their own password and a role (Admin, Standard or View only). Save the recovery key you
  are shown; it is the only way back in if every Admin forgets their password. LocalBooks signs
  everyone out after 15 minutes without activity (an Admin can change this), and can remember your
  sign-in on your own Mac. Files without users work exactly as before.
- **Encrypted tax IDs.** With users on, vendor tax IDs (SSNs and EINs) are encrypted and shown as
  `***-**-6789` to anyone not allowed to see sensitive data. The 1099 report masks them on screen;
  its CSV has them in full for people who may see them.
- **Accountability.** The audit trail now records who did what, including changes to users and
  passwords.
- **Fixed:** a foreign-currency payment could be refused after earlier partial payments on the same
  invoice were voided.
- Behind the scenes: much stronger automated testing of the ledger (thousands of random
  transaction sequences checked against the accounting rules) and saved copies of every report to
  catch unintended changes.
