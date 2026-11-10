# Importing a spreadsheet the user laid out (T62)

Bringing in customers, vendors and transactions from a spreadsheet someone built themselves, rather
than from a file another product wrote. This note covers how columns are mapped and how rows get
from the sheet to the books; the gaps row (`docs/gaps.md`, T62) tracks which kinds are done.

## Where it fits

LocalBooks already imports company files, IIF, QIF, GnuCash and QuickBooks report exports. Every one
of them ends the same way: a `Ledger` (accounts, customers, vendors, items, transactions) is
**staged**, the person reviews it in Settings › Import & Migration (accounts mapped, unbalanced
transactions and unreadable lines listed), and only then is it posted, as a batch that can be
undone. A spreadsheet import joins that path instead of posting by itself:

```
file (.csv, .xlsx, .xls, .ods) ─► grid of text ─► mapping (column → field) ─► row check ─► Ledger ─► staging ─► review ─► post
```

The step before staging is the new part: reading a grid whose headings the person chose, and
turning each row into what an IIF file would have said, so the IIF reader's resolver
(`iif_import::assemble`) builds the same documents. An invoice from a spreadsheet is an invoice
like any other import's: posted through the ledger, payable, and undone with its batch.

## Reading the file

- The first sheet of a workbook (the report-export reader's grid), or the CSV (comma or tab).
- The heading row is the first row with at least two non-empty cells. Rows after it that are
  entirely blank are skipped; everything else is a row to import, numbered as the spreadsheet
  numbers it (a workbook's row number, a CSV's line number), so problems can be found in the file.
- The file is read again for each step (preview, check, stage) rather than held between calls,
  so a person who fixes the file and checks again sees the fixed rows.

## Mapping columns to fields

Each kind has a fixed list of fields; some are required.

| Kind | Fields (required in bold) |
|---|---|
| Customers | **Name**, Company, Email, Phone, Billing address, Terms, Notes, Active |
| Vendors | **Name**, Company, Email, Phone, Address, Terms, Tax ID, 1099, Notes, Active |
| Invoices | **Invoice no.**, **Date**, **Customer**, Due date, Item, Description, Quantity, Rate, **Amount** (or Quantity × Rate), **Income account**, Memo |

- **Suggested mapping.** A heading maps to a field when it matches the field's name or one of a
  few common alternatives, ignoring case, spaces and punctuation ("E-mail", "Inv #", "Customer
  name", "Qty", "Price"). Nothing is mapped twice. The person can change any of it, or leave a
  field unmapped.
- **Several columns for one field** are not supported; an address in several columns is mapped
  through its own fields in a later PR if it turns out to be needed. One "Billing address" column
  may hold line breaks.
- **The downloadable template** is a CSV with exactly the field names as headings and one example
  row, so its suggested mapping is complete without any changes.
- **Dates** are read month-first or day-first as the person picks, defaulting to what the column's
  values make unambiguous (the bank import's detection); `2026-03-01` and Excel date cells always
  read.
- **Amounts** are read exactly as text to integer cents (the bank import's `parse_amount_cents`:
  `1,234.56`, `(45.00)`, `-$0.29`); quantities to four decimals. Never through floating point.

## Rows that would fail are listed first

Before anything is staged, every row is checked and each problem is listed with its row number:
a required field empty, a date or amount that doesn't read, an invoice line whose quantity × rate
disagrees with its amount by more than a cent, an invoice whose lines disagree on the customer or
date, a name repeated within a customer or vendor sheet. If any row has a problem, nothing is
staged: the person fixes the file (or the mapping) and checks again. A sheet with no problems is
staged as one batch and continues in the review screen as any other import.

## Staging

A clean sheet becomes a `Ledger` and is staged with `qbw_ledger_import::stage_as`, which records
`source_format = 'SHEET'` on the batch (migration 0100) so the review and the list of imports say
"spreadsheet" rather than guessing "report export" from the `.csv` or `.xlsx` extension. Everything
after that (account matching, adding lists, Journal or Full import, start over, reverse) is the
existing review, unchanged.

## Invoices

Rows with the same invoice number are one invoice, in sheet order. Each becomes an IIF-style
`INVOICE` block: the A/R line for the total, then one split per row crediting its income account
(created in the staged chart with type Income if the books don't have it; the review maps it to an
existing account), carrying item, description, quantity and rate. A customer named on an invoice
who isn't in the books yet is created with the batch, as other imports do. Sales tax, discounts and
payments are not columns in this first version.

## Other transactions (part 2)

Each transaction kind is a short description (`Doc` in `sheet_import.rs`): its IIF type, who it is
with (customer, vendor, or a payee who becomes a vendor), the account its total posts to (fixed,
such as Accounts Receivable, or a column such as Bank account, with Undeposited Funds as a sales
receipt's default), the label and type of its line accounts, and whether the total is a debit or a
credit there. One reader and one builder serve them all.

| Kind | IIF type | Total posts to | Lines post to |
|---|---|---|---|
| Invoices | INVOICE | Accounts Receivable, debit | income, credit |
| Sales receipts | CASH SALE | Deposit to (Undeposited Funds), debit | income, credit |
| Credit memos | CREDIT MEMO | Accounts Receivable, credit | income, debit |
| Deposits | DEPOSIT | Bank account, debit | From account, credit |
| Bills | BILL | Accounts Payable, credit | expense, debit |
| Bill credits | BILL REFUND | Accounts Payable, debit | expense, credit |
| Checks | CHECK | Bank account, credit | expense, debit |
| Card charges | CREDIT CARD | Card account, credit | expense, debit |
| Card credits | CCARD REFUND | Card account, debit | expense, credit |

Rows with the same number are one transaction and must agree on who it is with, the date and the
total's account. Where the number isn't required, a row without one is a transaction of its own.

**Journal entries** take Entry no., Date, Account, Debit, Credit, Memo and Name. Each row has a
debit or a credit, not both; rows are grouped by entry number, must agree on the date, and each
entry's debits must equal its credits, or the entry's first row lists the difference. They go to the
resolver as `GENERAL JOURNAL` blocks; an account the books don't have takes a type from its name,
and the review matches it to one of the books' accounts.

## Sales tax and discounts (part 3)

Invoices and sales receipts take **Sales tax** and **Discount**: amounts for the whole document,
which the rows that give them must agree on. The builder adds two splits to the document's block:
the tax to *Sales Tax Payable* (a current liability) and the discount to *Discounts Given* (income,
the other way from the item lines). The full import already reads a line to a liability account as
the document's sales tax and an income line the other way as its discount, so they land on the
invoice as `tax_cents` and `discount_cents`. The review maps both accounts to the books' own.

## Payments (part 3)

A payments sheet pays invoices and bills that are already in the books, which the staged resolver
can't (it applies payments only within one import). So customer and bill payments are not staged:
`sheet_import/payments.rs` reads the rows, checks each against the books (the invoice by number, or
the vendor's bill by number and vendor; open, home currency, the named customer's, enough left once
the sheet's earlier rows are counted; the account a bank, Undeposited Funds or for bills a card),
and posts every row in one transaction through `invoices::record_payments` and
`expenses::write_bill_payment`, the code Receive Payments and Pay Bills use. Checking a payments
sheet runs the same checks, so the list of problems is complete before anything posts.

## Later

A sales tax rate (rather than an amount) and a payment's early-payment discount are not columns.
