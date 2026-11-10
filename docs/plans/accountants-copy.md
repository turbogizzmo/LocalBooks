# Accountant's Copy (gaps row R14)

Today the client and the accountant can't both work on the books: one sends a backup and stops
working until it comes back, or one side's work is lost. This note sets out a copy the accountant
works on while the client keeps going, and how the accountant's changes come back.

## The dividing date

The client chooses a **dividing date** (usually the end of the period the accountant is reviewing).
While a copy is out:

- the **client** can't post anything dated on or before the dividing date (the accountant owns
  that period), and keeps working after it;
- the **accountant**, in the copy, can't post anything dated after it.

Both are enforced where every posting already checks the closing date (`closing::check_open`), so
no form or import can get round them.

## The round trip

1. **Client: Create Accountant's Copy…** (Admin). A verified snapshot of the books
   (`backup::snapshot`) is written where the client chooses. Inside the copy, `accountants_copy_info`
   records the copy's token, the dividing date, and the last journal entry and account ids at that
   moment. The client file records the copy as outstanding (`accountants_copies`, one at a time).
2. **Accountant** opens the copy in LocalBooks (it shows a banner with the dividing date), and works:
   journal entries (adjusting or not) and new accounts. **Export Changes for Client…** writes a
   changes file: the copy's token, the dividing date, the accounts added since the copy was made,
   and the journal entries posted since then that still stand (an entry the accountant voided in
   the copy, and its reversal, are left out). Accounts are named by number and full name, so the
   file means the same thing in the client's books.
3. **Client: Import Accountant's Changes…** (Admin). The file must carry the token of the copy
   that is out. A preview lists the accounts and entries. **Apply** creates the accounts, posts each
   entry through `ledger::post_entry_in_tx` (keeping its date, memo, adjusting flag and lines; the
   memo notes it came from the accountant), marks the copy returned, and keeps the **audit record**:
   who imported it and when, the file's SHA-256, the entries posted and the accounts created. It all
   happens in one write transaction, so it applies completely or not at all. The entries also enter
   the ledger's own hash-chained audit trail like any posting.
4. **Cancel the copy** lifts the client's lock without importing anything (the copy is then useless:
   its changes are refused).

## What travels, and what doesn't

Journal entries and new accounts travel. Other things the accountant might do in the copy
(invoices, bills, reconciliations, edits to existing accounts, voids of the client's entries) don't:
the changes file can't express them safely against books that kept moving. The guide says so, and
the export lists what it left out. This matches the review's "middle step" (an export of the
accountant's journal entries, with a dividing line).

## Not in this change

An External Accountant user role and a closing-date exceptions report (also on the R14 review
notes) stay as follow-ups.
