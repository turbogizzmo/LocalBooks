# Import from QuickBooks Desktop

LocalBooks can read invoice amounts from a QuickBooks Desktop company file (`.qbw`) with the
open-source OpenQBW reader.

> **Read this first:** only invoice line amounts can be read from QuickBooks files today, not
> customers, payments, bills or account names. Imported invoices post as income against an offset
> account you choose. Always compare the total with an invoice report in QuickBooks before posting.

## 1. Choose the file

1. Go to **Accounting › QuickBooks Import**.
2. Click **Choose QuickBooks file (.qbw)…**, or drag the file onto the box. Compressed backups
   (`.qbb`) and portable files (`.qbm`) must first be restored in QuickBooks to a `.qbw`.
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
