# Attach receipts and documents

Keep the paperwork behind a transaction with it: the vendor's invoice on a bill, the receipt for an
expense, a signed estimate or contract on an invoice, the deposit slip on a deposit. The files are
kept inside the company file, so they are in every backup and move with the books.

## What you can attach

- Any file up to **25 MB**: a PDF, a scan or photo (JPG, PNG, HEIC), a spreadsheet or a Word
  document.
- As many files as you like on each transaction.
- Files go on these transactions: **bills**, **expenses and checks**, **invoices**, **sales
  receipts** and other sales, **deposits and transfers**, and **journal entries**.
- They also go on **customers** and **vendors** (a W-9, a contract), **items** (a spec sheet, a
  photo), **accounts** (a loan agreement), **estimates**, **purchase orders** and
  **reconciliations** (the bank statement; see [Reconcile an account](reconciling.md)). Open the record
  (double-click it in its list; for an estimate or purchase order open it and click **Attach File**
  on its toolbar) and use **Attach file…** in its Attachments strip. Files on these records are
  saved at once, not with the record's Save.

## Attach files while you enter a transaction

On a new **bill**, click **Attach the vendor's invoice…** under the lines. On a new **expense** or
**check**, click **Attach a receipt…**. Choose one or more files. They are listed on the form, and ×
removes one you picked by mistake.

The files are attached when you post. If the transaction posts but a file can't be read (for
example it was moved, or is larger than 25 MB), LocalBooks says so; the transaction is kept, and
you can attach the file afterwards.

## The Document Inbox

Receipts and vendor bills that arrive before you have time to enter them can wait in
**Expenses › Document Inbox** (also **Vendors › Document Inbox**).

1. Drop the files on the inbox window, or click **Add files…**. PDFs, photos and scans (JPG, PNG,
   HEIC) up to 25 MB each are kept inside the company file, so they're in every backup.
2. Click one to see it beside the list: a PDF shows page by page, a photo as it is. Add a **Note** if
   it needs one (*ask Pat which job*).
3. Click **Enter as bill**, **Expense** or **Check**. That form opens with the file attached and,
   for a PDF with text, what LocalBooks read from it filled in: the **total** (the amount on a line
   such as *Total*, *Amount due* or *Balance due*), the **date**, and the **name** at the top as the
   vendor or payee. Check them, finish the form and post it.

When it posts, the file is attached to the transaction and leaves the inbox. If you close the form
without posting, the file stays in the inbox. **Remove** takes a file out without entering it.

Nothing is sent anywhere to read a file: LocalBooks reads only the text a PDF already contains.
Photos and scanned PDFs have no text, so their fields are left for you to type (there's no OCR).

To bring in paper, scan it with your scanner's own app (or a phone) to a PDF or photo, then drop
the file on the inbox. LocalBooks doesn't drive scanners itself: each system has its own scanner
interface (and some scanners need their maker's driver), so scanning from inside LocalBooks would
work for some scanners and not others.

## Attach files to a posted transaction

1. Find the transaction in its list: **Expenses › Bills**, **Expenses › Expenses & Checks**,
   **Sales › Invoices**, **Sales › All Sales**, **Banking › Deposits & Transfers** or
   **Accounting › Journal Entries**.
2. Open the row's **Actions** menu (▾) and choose **Attach file…**. A strip opens under the row.
3. Click **Attach file…** in that strip and choose the files. They are attached straight away.

## See, save and remove files

A 📎 after the number marks a transaction with files (📎3 means three). Click it, or choose
**Attachments (3)…** in the row's **Actions** menu, to show them under the row:

- Click a **file name** to save a copy where you choose, for example to email it to your
  accountant or open it in another program.
- Click **×** next to a file to delete it. LocalBooks asks first. Deleting is reversible: the file
  is kept, and **Accounting › Attachments Library** can restore it (see below).
- Click **Attach file…** in the strip to add more.
- With two or more files, **Save all…** saves them together in one zip file.

Click the 📎 again to close the strip.

In **Accounting › Journal Entries**, click any entry to open it: its files are listed under its
lines, with **Attach file…**. Every transaction is in the journal, so this works for any of them.

## The Attachments Library

**Accounting › Attachments Library** (also **Company › Attachments Library**) lists every file in
the company file, newest first: when it was added, its name, keywords, what it is attached to and
its size.

- **Search** by file name, keyword, or what it is attached to (a customer's or vendor's name, a
  document number, "invoice", "vendor").
- Click a file to see it on the right: pictures, PDFs (page by page) and text files show a preview;
  for any other kind of file, **Save a copy…** and open it in another program.
- Give it **Keywords** (for example "W-9 2025" or "signed") so it's easy to find, and tick **Send
  with the form when it is emailed or printed** to send it along with the invoice, estimate or
  sales receipt it belongs to. Click **Save details**. A file on an invoice's, sales receipt's,
  credit memo's or bill payment's transaction, or on an estimate, goes with that form; a file on a
  customer goes with their statements. Marked files are attached whether LocalBooks emails the
  form (Settings › Email) or opens the message in your mail app, up to 20 MB together. When the
  form is printed, marked pictures and PDFs print after it, each on its own page; other kinds of
  file are left out of the printout, and LocalBooks says which.
- **Delete…** hides the file from its transaction or record but keeps it. Tick **Show deleted** to
  see deleted files, then **Restore** one to put it back.
- **Remove for good…** (on a deleted file, Admins only) erases it from the company file after a
  warning. It can't be restored, except from a backup made before.

### How long to keep files

Many tax authorities expect you to keep the records behind your books (receipts, invoices, bank
statements) for several years, often six or seven; check what applies to you. LocalBooks never
removes a file unless an Admin removes a deleted one for good, and the files are in every backup.

## Save every attachment at once

**Settings › File & Data Controls › Save All Attachments…** saves every file attached to any transaction
in one zip file, for your accountant or an auditor, or to keep outside LocalBooks. Inside the zip
each transaction has its own folder, named by its date, its kind and its number, such as
`2026-10-01 Bill 118 (entry 42)`; two files with the same name in one transaction are kept as
`receipt.pdf` and `receipt (2).pdf`. Nothing in the company file changes.

## When you edit or void a transaction

- **Editing** a posted transaction keeps its files: LocalBooks voids the original and posts the
  corrected version, and the files move to the corrected version.
- **Voiding** a transaction leaves its files on the voided transaction, so you still have the
  paperwork for the audit trail.

## Who can attach files

When users are on, people with the **View only** role can see and save copies of files but can't
attach or remove them. See [Users and passwords](users-and-passwords.md).

> **Tip:** Attachments make the company file bigger. A few hundred receipts are fine; if you scan
> everything at high resolution, save scans as PDF at 150–200 dpi to keep backups quick.
