# Desktop Migration Wizard: import from QuickBooks or Quicken

The wizard brings in books from:

- a QuickBooks Desktop company file (`.qbw`) or backup (`.qbb`)
- QuickBooks Desktop for Mac, through a backup saved for QuickBooks for Windows (see **From
  QuickBooks Desktop for Mac** below)
- a QuickBooks **IIF export** (`.iif`)
- a Quicken **QIF export** (`.qif`), which many banks and other programs also produce

> **Read this first:** newer company files and IIF and QIF exports come in as the full ledger:
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
- **IIF transactions:** a full import makes invoices, credit memos, sales receipts, payments,
  deposits, bills, vendor credits, item receipts, bill payments, checks, card charges, transfers,
  estimates and purchase orders. Anything else posts as a journal entry. IIF doesn't record which
  invoice a payment paid, so, like QuickBooks, LocalBooks applies each payment to that customer's
  oldest open invoices, and each bill payment to that vendor's oldest open bills. A deposit is
  matched to undeposited payments of the same amount.
- **QIF transactions:** a full import makes checks (when they have a check number), expenses, card
  charges, deposits and transfers. Opening balances post against Opening Balance Equity.
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
only, not transactions, so use the Windows backup to move your books.

## 1. Choose a file

1. Go to **Accounting › Desktop Migration Wizard**.
2. Click **Choose a file…**, or drag the file onto the box.
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
     credit memos, sales receipts, payments, deposits, bills, bill payments, checks, card charges,
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
