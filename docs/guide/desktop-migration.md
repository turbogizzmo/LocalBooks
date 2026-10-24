# Desktop Migration Wizard: import from QuickBooks or Quicken

The wizard brings in books from:

- a QuickBooks Desktop company file (`.qbw`) or backup (`.qbb`)
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
- A file with lists but no transactions can't be imported on its own yet.

## 1. Choose the file

1. Go to **Accounting › Desktop Migration Wizard**.
2. Click **Choose a file…**, or drag the file onto the box.
3. Wait while the progress bar works through the file; large files take a minute or more. The file
   is only read, never changed, and nothing is added to your books yet.

## 2. Review what was found

1. Check the figures: **Invoices**, **Invoice lines**, **Total**, **Dates** and **Ignored**. Ignored
   lines are matches outside invoice tables, or amounts that couldn't be decoded.
2. For each line type, choose an income account under **Post to this LocalBooks income account**,
   or click **Create an income account for each**.

## 3. Post to your books

1. Under **Balance each invoice against**, keep **Opening Balance Equity** unless your accountant
   says otherwise. The file has no customer payments, so these sales shouldn't sit in Accounts
   Receivable.
2. Optionally set a **Date for invoices without one**; leave it blank to skip them.
3. Click **Post … entries**, check the count and total in the confirmation, and confirm.

## Undo or tidy up

Every import is listed under **Your imports**:

- **Open** continues an import you haven't posted yet.
- **Reverse posted entries…** voids everything the import posted and deactivates the accounts it
  created, returning your balances to where they were.
- **Delete import** removes the import's staged lines. Your books aren't affected. An import with
  posted entries must be reversed first.
