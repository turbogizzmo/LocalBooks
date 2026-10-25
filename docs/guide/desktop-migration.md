# Desktop Migration Wizard: import from QuickBooks or Quicken

The wizard brings in books from:

- a QuickBooks Desktop company file (`.qbw`) or backup (`.qbb`)
- QuickBooks Desktop for Mac, through a backup saved for QuickBooks for Windows (see **From
  QuickBooks Desktop for Mac** below)
- a QuickBooks **IIF export** (`.iif`)
- a Quicken **QIF export** (`.qif`), which many banks and other programs also produce
- QuickBooks **report exports** (`.csv` or `.xlsx`) from QuickBooks Desktop for Windows or Mac, or
  QuickBooks Online: the Journal (or General Ledger), with the Account Listing, contact lists and
  Item List

> **Read this first:** newer company files, IIF and QIF exports and report exports come in as the full ledger:
> accounts, customers, vendors, items and every transaction. Some older `.qbw` files can only be
> read for invoice line amounts, which post as income against an offset account you choose.
> Always compare the result with a trial balance from the original program before posting.

## Import an IIF or QIF export

**IIF files** usually come from programs that feed QuickBooks Desktop: payment processors,
point-of-sale and payroll tools, or your accountant. QuickBooks itself exports only lists to IIF
(**File › Utilities › Export › Lists to IIF Files**), not transactions. To move QuickBooks Desktop
books, choose the `.qbw` company file or a `.qbb` backup instead.

**From Quicken:** use **File › Export › QIF File**. Choose **All accounts**, and include
**Transactions**, **Account list** and **Category list**.

Then choose the file in the wizard, as for a company file below. LocalBooks reads it like this:

- **Accounts and categories** become accounts. Subaccounts (`Utilities:Electric`) keep their
  parent. An account the file uses but never declares gets a type from its name and how it is
  used. Check the account types in step 2.
- **IIF lists:** customers (with jobs), vendors (with tax ID and 1099 flag) and items come in.
  So do the Ship Via list (`SHIPMETH`), each customer's rep (`REP`), and custom field values
  (`CUSTFLD1`…) on customers, vendors and items. Labels come from `CUSTNAMEDICT` and
  `CUSTITEMDICT`; a value without a label comes in as *Custom field N*. Invoices and sales
  receipts keep their `REP`, `SHIPVIA` and `SHIPDATE`.
- **IIF transactions:** a full import makes invoices, credit memos, sales receipts, payments,
  deposits, bills, vendor credits, item receipts, bill payments, checks, card charges, card credits, transfers,
  estimates and purchase orders. Anything else posts as a journal entry. IIF doesn't record which
  invoice a payment paid, so, like QuickBooks, LocalBooks applies each payment to that customer's
  oldest open invoices, and each bill payment to that vendor's oldest open bills. A deposit is
  matched to undeposited payments of the same amount.
- **QIF transactions:** a full import makes checks (when they have a check number), expenses, card
  charges, card credits, deposits and transfers. Opening balances post against Opening Balance Equity.
  - A transfer appears in both accounts of a Quicken export; LocalBooks brings it in once.
  - A blank category goes to **Uncategorized Income** or **Uncategorized Expense**.
  - Investment accounts are skipped, because LocalBooks doesn't track securities.
- **Dates:** IIF dates are always month first. QIF dates are read month first unless a date in the
  file can only be day first (such as `25/04/2024`).
- **Lists only:** a file with lists but no transactions (such as Intuit's "Import First - List
  Declarations" IIF) imports too. Step 2 shows every account in the file. Step 3 becomes **Add lists
  to your books…**, which matches or creates the accounts, customers, vendors and items and posts
  nothing. **Reverse added lists…** deactivates what it created; **Start over…** puts back the
  books from just before the lists were added.

## From QuickBooks Desktop for Mac

LocalBooks can't open a QuickBooks for Mac company file (`.qb2024`, `.qb2023` and so on) or a Mac
backup (`.qbmb`) directly, because QuickBooks encrypts them. Save a copy for QuickBooks for Windows
instead, and import that:

1. In QuickBooks for Mac, open the company file.
2. Choose **File › Export › To QuickBooks for Windows** (or **File › Save File for Accountant**,
   which makes the same file).
3. Click **Verify** when asked. If verification fails, rebuild the company file in QuickBooks first.
4. Choose a name and location, and click **Save**.
5. When QuickBooks asks for a password, **leave both fields blank** and click **OK**. LocalBooks
   can't read a password-protected backup.
6. In LocalBooks, choose the new `.qbb` file in the wizard, as for a company file below.

Exporting to IIF from QuickBooks for Mac (**File › Export › Lists to IIF File**) brings in lists
only, not transactions, so use the Windows backup to move your books. If you can't make the
Windows backup (for example, your QuickBooks for Mac only opens files in view-only mode), export
its reports instead: see [Import QuickBooks reports](#import-quickbooks-reports).

## Import QuickBooks reports

Every QuickBooks product can export its reports to Excel or CSV, so this works even when LocalBooks
can't read the company file, and needs no file decryption. Export these reports, then choose them
**all together** in step 1 (hold **⌘** or **Ctrl** to pick several, or drag them onto the box):

- the **Journal**: every transaction with all its lines. This is the one that matters.
- the **Account Listing** (QuickBooks Online: **Account List**): gives each account its type, and
  its balance, which step 2 shows next to the imported total so you can compare.
- the **Customer Contact List** and **Vendor Contact List**: addresses, phones, emails, terms, and
  vendors' tax IDs and 1099 status.
- the **Item List** (QuickBooks Online: **Product/Service List**).

Only the Journal is needed; the lists add detail. Lists can also be imported on their own.

### No QuickBooks license any more?

- **QuickBooks Desktop for Mac:** an expired copy still opens company files in **view-only mode**
  for about a year, and view-only mode can run and export reports. If yours no longer opens at all,
  install the **30-day trial of QuickBooks Desktop for Mac 2024** from Intuit, open the company file
  in it (it may ask to update a copy of the file to the 2024 format), and export the reports below.
- **QuickBooks Desktop for Windows:** an expired copy opens in a similar read-only mode; run the
  reports from there.
- **QuickBooks Online:** a cancelled subscription keeps read-only access for a year. Sign in and
  export the reports.

### QuickBooks Desktop for Windows

1. Go to **Reports › Accountant & Taxes › Journal**.
2. Click **Customize Report**. On **Display**, set **Dates** to **All**.
3. Under **Columns**, tick **Trans #**, **Type**, **Date**, **Num**, **Name**, **Memo**,
   **Account**, **Class**, **Debit** and **Credit**. Click **OK**.
4. Click **Excel › Create New Worksheet**. Choose **Create a comma separated values (.csv) file**
   (or **in a new workbook** for `.xlsx`), then click **Export** and save the file.
5. For the lists, use **Reports › List**: **Account Listing**, **Customer Contact List**, **Vendor
   Contact List** and **Item Listing**. Export each the same way. In the Vendor Contact List you can
   add the **Tax ID**, **Eligible for 1099** and **Terms** columns with **Customize Report**.

### QuickBooks Desktop for Mac

1. Go to **Reports › Accountant & Taxes › Journal**.
2. Click **Customize** (in older versions, **Options**). Set the dates to **All**.
3. Under **Columns**, tick **Trans #**, **Type**, **Date**, **Num**, **Name**, **Memo**,
   **Account**, **Class**, **Debit** and **Credit**.
4. Click **Export** on the report's toolbar (or **File › Export › Report to Excel**). Save as an
   Excel workbook (`.xlsx`) or as comma-separated values (`.csv`).
5. Export the lists from **Reports › Lists**: **Account Listing**, **Customer Contact List**,
   **Vendor Contact List** and **Item List**.

### QuickBooks Online

1. Go to **Reports**, search for **Journal** and open it.
2. Set **Report period** to **All Dates**.
3. Click **Customize** (or the **Columns** / **⚙ Change columns** control) and show **Date**,
   **Transaction Type**, **Num**, **Name**, **Memo/Description**, **Account**, **Class**, **Debit**
   and **Credit**. Click **Run report**.
4. Click the **Export** icon and choose **Export to Excel** (or **Export to CSV**).
5. Export the lists the same way: **Account List**, **Customer Contact List**, **Vendor Contact
   List** (add **Tax ID** and **Track 1099** under **Change columns**) and **Product/Service List**.

### If the Journal won't export: the General Ledger

The **General Ledger** (or **Transaction Detail by Account**) works too. Set the dates to **All**
and show the **Type**, **Date**, **Num**, **Name**, **Memo**, **Split**, **Debit** and **Credit**
columns (and **Trans #** where QuickBooks offers it). It lists each line under its account, so
LocalBooks rebuilds each transaction by pairing its lines through the **Split** column. A line it
can't pair is listed as not read completely and isn't posted. Use either the Journal or the
General Ledger, not both: each holds every transaction.

### What LocalBooks does with them

- **Finding the data:** the title rows, blank and spacer columns, account headings, total rows and
  the footer are skipped. Columns are found by their heading, whatever the product calls them, in
  any order. Step 1 says which product and report each file was read as. A file that isn't one of
  these reports is refused, with the reason.
- **Amounts** come only from the **Debit** and **Credit** columns, as written: `1,234.56`,
  `(45.00)` and `-$0.29` all read. A report with only an **Amount** column is refused, because
  that column's sign depends on the account. An amount that doesn't read is never guessed: its
  transaction is listed as not read completely, and isn't posted.
- **Dates:** Excel dates, Excel serial numbers and text dates all read. Text dates are month first
  unless a date in the file can only be day first (such as `25/04/2024`).
- **Transactions:** QuickBooks Desktop's **Trans #** groups the lines. QuickBooks Online has no
  transaction number, so lines with the same date, type and number make one transaction until its
  debits equal its credits. A transaction that doesn't balance is listed and not posted.
- **Account types** come from the Account Listing. Without it, each account gets a type from its
  name and how it's used, as for IIF. Check the types in step 2.
- **Full import:** invoices, payments, sales receipts, credit memos, bills, vendor credits, bill
  payments, checks, expenses, card charges, card credits, deposits and transfers become LocalBooks
  documents, as for IIF. General journal entries and anything else post as journal entries. The
  reports don't say which invoice a payment paid, so payments apply to the customer's oldest open
  invoices, bill payments to the vendor's oldest open bills, and deposits to undeposited payments of
  the same amount.
- **Several journals** (one per year, say) can be chosen together. With **Trans #**, a transaction
  in two of them is read once; without it, make sure their dates don't overlap.

## 1. Choose a file

1. Go to **Accounting › Desktop Migration Wizard**.
2. Click **Choose a file…**, or drag the file onto the box. For report exports, choose the report
   and its lists together.
3. Wait while the progress bar works through the file; large files take a few minutes. Keep
   LocalBooks open. The file is only read, never changed, and nothing is added to your books yet.

To bring in a different file later, click **Import another file**.

## 2. Review what was read and choose accounts

1. Check the figures: **Transactions**, **Lines**, **Dates**, **Accounts used** and **Balances
   matching the file**. Each account's **Read** balance should equal **File's balance**; a
   difference is shown in red.
2. If the heading says some lines were **not read in full**, open it. Each line is marked
   **Recovered**, **No amount** or **Not read**. Only **Not read** lines can leave something out; if
   their transaction is listed, enter it by hand after posting. Transactions listed as **could not
   be read completely** are never posted.
3. For each source account, choose a LocalBooks account under **Post to this LocalBooks account**.
   Or click **Match existing accounts and create the rest** to map by name and create what's missing.
   Every account must have a LocalBooks account before you can post.
4. If the file has customers, vendors and items, click **Bring in customers, vendors and items**.
   It matches existing records by name and adds the rest.

## 3. Post to your books

1. Choose how to bring in the transactions:
   - **Full import** (recommended when the file has sales documents) makes LocalBooks invoices,
     credit memos, sales receipts, payments, deposits, bills, bill payments, checks, card charges, card credits,
     transfers, estimates and purchase orders, so open invoices and bills, customer and vendor
     balances, and A/R and A/P aging carry over. Anything that can't be a document posts as a
     journal entry and is listed under **not brought in as LocalBooks documents**.
   - **Journal import** turns every transaction into one journal entry. Account balances are the
     same, but invoices and payments aren't recreated.
2. Click **Bring in … transactions…** (Full) or **Post … entries…** (Journal), check the count in the
   confirmation, and confirm.
3. Compare the account balances with a trial balance from the original program.

### Older files that only yield invoice lines

Some older `.qbw` files can't be read as a full ledger. For these the wizard finds invoice line
amounts only, not accounts, customers or payments, and steps 2 and 3 change:

1. In step 2, check **Invoices**, **Invoice lines**, **Total**, **Dates** and **Ignored**. Ignored
   lines are matches outside invoice tables, or amounts that couldn't be decoded.
2. Under **Post to this LocalBooks income account**, choose an account for each line type, or click
   **Create an income account for each**.
3. In step 3, keep **Opening Balance Equity** under **Balance each invoice against** unless your
   accountant says otherwise. The file has no customer payments, so these sales shouldn't sit in
   Accounts Receivable.
4. Optionally set a **Date for invoices without one**; leave it blank to skip them.
5. Click **Post … entries…**, check the count and total in the confirmation, and confirm.

## Undo or tidy up

Every import is listed under **Your imports**:

- **Open** continues an import you haven't posted yet, or reviews one you have.
- **Start over…** puts your books back to the copy saved just before the import was posted. It is
  the way to undo a **Full import**. If no copy was saved, the button is disabled; use **Reverse
  posted entries…** instead where it is offered.
- **Reverse posted entries…** voids everything a journal or invoice-line import posted and
  deactivates the accounts it created, returning your balances to where they were.
- **Delete import** removes the import's staged lines. Your books aren't affected. An import with
  posted entries must be undone first (**Start over…** or **Reverse posted entries…**).
