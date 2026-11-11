# Import accuracy check (gap R3)

How well a QuickBooks Desktop company file (`.qbw`) or backup (`.qbb`) comes into LocalBooks, measured
against the balances the file itself stores for each account. Run 2026-11-09. Customer names and
amounts from the sample files are left out on purpose; only counts are given.

## How to run it

The sample files live outside the repository, on the developer's own disk, and are never committed
(`AGENTS.md`, clean-room rules).
Two ignored tests do the work; build them once with `cargo test --release --no-run`, then:

- `LOCALBOOKS_QBW_CHECK=<file> cargo test --release -- --ignored reads_the_ledger_of_a_real --nocapture`
  reads the file and lists every account whose read total differs from its stored balance.
- `LOCALBOOKS_QBW_CHECK=<file> LOCALBOOKS_QBW_MODE=full cargo test --release -- --ignored posts_a_real_company_file --nocapture`
  imports the file into a fresh test database (journal mode without `LOCALBOOKS_QBW_MODE`), then
  compares each LocalBooks account's balance with the stored balances of the file's accounts mapped
  to it. The books must balance to zero.

Point `LOCALBOOKS_QBW_CHECK` at a copy, not the original. Both tests read the file only.

## Files checked

| Sample | Before this change | After |
|---|---|---|
| A real company backup (`.qbb`, 2006–2025, about 9,400 transactions) | 77 of 77 LocalBooks accounts match in both modes; 112 of 112 file accounts read exactly | Same |
| Textbook sample company, 2022 edition | 348 transactions balance, 501 do not; 43 of 82 stored balances match | 1,054 balance, 5 do not; 87 of 111 match |
| Construction sample company | 1,051 balance, 22 do not; 82 of 115 match | 1,081 balance, 1 does not; 96 of 115 match |
| Textbook chapter file on online banking (small) | 11 transactions read; no invoices (their pages were unread) | 19 read, invoices included; 79 of 87 file accounts match by the table walk |
| Textbook chapter file on payroll (small) | 17 balance; 77 of 84 match | Same (none of its pages fell in the fixed range) |
| Contractor course, chapter 12 | 17 balance, 1 does not; 54 of 60 match | 19 balance, 0 do not; 55 of 60 match |
| Contractor course, starter file | No transactions; 31 of 31 match | Same |

## The −$3,995 bank difference

The figure in the R3 row came from the real backup above, before #30. Three decoder faults caused it,
all fixed in #30 and confirmed again here (77 of 77 accounts match after a full import):

1. **A row repeating the page number.** The page decoder anchors each page by finding its own page
   number in the first sector. A row whose value equals the page number gave a second match, and the
   decoder gave up on the page; it held ten checks. Now each match is tried and the one whose header is
   valid and whose rows chain wins. Regression test (synthetic page):
   `qbw_pages::tests::a_row_that_repeats_the_page_number_does_not_hide_the_page`.
2. **Sector 7 with a cipher base of its own.** On some pages the last sector's cipher base is not the
   one the other sectors imply. The re-solve now derives it from the row lengths the slot directory
   already gives. Regression test: `qbw_pages::tests::sector7_with_its_own_cipher_base_is_solved_from_the_row_lengths`.
3. **Rows damaged near their end.** Their leading columns (transaction, account, amount) are used
   only when they make the transaction balance to the cent. Regression test:
   `qbw_company::tests::damaged_lines_are_used_only_when_they_balance_their_transaction`.

## Fixed here: sector 7 near the sector boundary

The last sector of a page is stored rotated, with a 16-byte trailer at a fixed place; the data bytes
the rotation would put on the trailer are stored where the rotated trailer slot lands. When the
rotation was within 16 bytes of a sector boundary that slot wraps around, and the decoder left the
whole sector unread. About one page in nine of the textbook files fell in that range, taking invoice
and check lines with it. The slot now wraps: only part of it lies below the trailer, and exactly as
many data bytes are displaced into that part. On a textbook file, rows that start in sector 7 and
chain went from 11,687 to 13,032 of 13,208, and unread sectors from 212 to 6. Regression test:
`qbw_pages::tests::sector7_mapping_handles_rotations_next_to_the_sector_boundary`.

## Differences left, and where they show

Every account whose imported balance differs from the file's stored balance is now listed in step 2
of the import (account, the file's balance, the imported balance, the difference), and stays listed
after posting with a notice in step 3. What is left on the files above:

- **Inventory cost lines.** Cost of goods sold and the matching inventory-asset lines are kept in
  separate inventory record tables, linked to invoice, sales receipt, credit memo and bill lines by
  line id rather than by transaction. The reader joins them to their form (R3a, #327; bills in
  #380). A pair whose form line itself can't be read is left out and counted as rejected.
- **Inventory adjustments.** Some files lost one column definition of the adjustment line table
  from their catalog, so every adjustment row read out of step and none was imported. When the
  catalog skips a column id, the reader now tries the row with that column left out first (as
  before), then as each likely column type, and keeps the reading that ends exactly at the row's
  end. On the construction sample this brought in its three adjustments, and Inventory Asset and
  Cost of Goods Sold now differ from the file only by the cost pair of one unreadable invoice line
  ($180) and the lines of the invoices that can't be posted because a line is damaged.
- **Assembly builds.** A build moves value from its component items into the assembled item, all
  within Inventory Asset, so it changes no account balance. LocalBooks does not bring in item
  quantities from a company file yet, so builds are not imported (gap R3c).
- **Payroll.** Paychecks and payroll liability payments keep their payroll-item lines in tables of
  their own; the reader reads them (R3a, #327).
- **Check lines in upgraded files.** In files upgraded from an older QuickBooks version, damaged
  catalog pages hid some column definitions of the check line table; they are re-solved before the
  catalog is read (R3b, #323), and lost columns that remain are guessed (#380, and the sign-off below).
- **A handful of unread rows.** A few transactions per file still have a line that could not be read;
  they are listed in step 2 for entry by hand and are left out of posting.

The real backup, which is the case users bring, matches in full.

## Sign-off, 2026-11-11 (R3)

Every company file in the test kit was read and fully imported with the code on `main` after #380
and #385 plus the fixes below: the real backup and 31 sample files (19 textbook chapter files, the
textbook sample company, 9 contractor-course chapter files, the contractor starter file and the
construction sample). "Read" counts the file's accounts whose total from the decoded lines equals
the balance the file stores; "Full import" counts LocalBooks accounts whose balance after a full
import equals the stored balances of the file accounts mapped to them. An account the file keeps
no balance for (some equity and system accounts) is not compared; the check used to count it as
zero, which showed false differences on two chapter files.

### Fixed in this change

- **Memorized transactions.** The file keeps memorized transactions (templates for entering the
  same transaction again) in the same tables as real ones, marked on the line or the header. The
  reader counted them as real, so every account a template touched came out doubled on the
  chapter file about memorizing transactions. They are now skipped, by the ledger reader and the
  document reader alike (read 70 → 82 of 82).
- **Tables that lost several column definitions.** #380 guesses up to two lost columns per table.
  Some files lost three (check lines on the getting-started chapter) or six (bill lines on the
  chapter about other accounts). Up to three, each row still tries every guess; from four to six,
  the reader works out the table's layout once, on the first row that needs it, and reads the
  other rows with it (all rows of a table share one layout), giving up after three rows that fit
  no guess. Read: getting-started chapter 75 → 83 of 85, other-accounts chapter 85 → 95 of 95,
  textbook sample 101 → 104 of 111, construction sample 110 → 114 of 115.
- **One movement kept as two halves.** Some files keep a customer payment deposited straight to
  the bank as a payment holding only its receivables line and a deposit on the same day holding
  only the bank line; neither balances alone, so both were left out of posting. Two unbalanced
  transactions on the same date whose differences cancel exactly, each the other's only such
  match, are now read as one under the half that names a customer, vendor or number. This put the
  contractor chapter 5 file in full agreement (full import 5 → 7 accounts, none differing).

### Results

| File | Read: stored balances matched | Full import: LocalBooks accounts matching / differing |
|---|---|---|
| Real backup (`.qbb`) | 112 of 112 | 77 / 0 (journal mode too) |
| 11 more sample files (7 textbook chapter and appendix files, contractor chapters 3, 5 and 6, the contractor starter file) | all | all match, none differing |
| Textbook, getting started | 83 of 85 | 19 / 2 |
| Textbook, memorizing transactions | 82 of 82 | 18 / 0 |
| Textbook, other accounts | 95 of 95 | 25 / 0 |
| Textbook, entering bills | 84 of 84 | 17 / 0 |
| Textbook, lists | 85 of 85 | 17 / 0 |
| Textbook, estimating appendix | 83 of 85 | 14 / 3 |
| Textbook, letters appendix | 84 of 85 | 15 / 2 |
| Textbook, customizing forms | 77 of 85 | 9 / 2 |
| Textbook, reports | 82 of 84 | 18 / 2 |
| Textbook, setting up inventory | 71 of 86 | 2 / 3 |
| Textbook, invoicing for services | 83 of 85 | 16 / 2 |
| Textbook, bank accounts | 86 of 87 | 17 / 2 |
| Textbook sample company | 104 of 111 | 63 / 14 |
| Contractor, chapters 10, 11, 12 | 54–55 of 60 | 6–8 / 3–5 |
| Contractor, chapter 4 | 63 of 65 | 2 / 1 |
| Contractor, chapters 7 and 8 | 65 of 68, 58 of 65 | 7 / 5, 5 / 5 |
| Construction sample | 114 of 115 | 76 / 2 |

### What is left, and why

Every remaining difference comes from data the reader can't decode, not from how a decoded
transaction is imported; each one is listed in step 2 of the import (unreadable lines for entry
by hand, accounts that differ). Gap R3d tracks them.

- **Damaged line rows** (most files above): a few rows per file don't decode, or decode with a
  value that fails the checks (unknown account, a date out of range). Their transactions don't
  balance and are left out of posting. Examples: one $35 invoice line (getting started), one bill
  line of $850 (letters appendix), one check line (estimating appendix), 4 to 17 rows on the
  contractor chapter files, 28 on the textbook sample.
- **Inventory cost rows that don't decode** (reports, invoicing for services, textbook sample,
  construction sample): the cost pair of a sold line is incomplete, so it is rejected and Cost of
  Goods Sold and Inventory Asset differ by that pair ($104.85 to $285.00; $180.00 on the
  construction sample).
- **Column types that don't match the rows** (setting up inventory): the file's catalog describes
  the bill and check line tables with column types that don't match how their rows are stored, so
  none of those rows decode (29 lines). This is a different fault from lost columns, which the
  reader now handles.
- **Pages that can't be decoded** (contractor chapter 4): 103 of the file's 1,468 row pages have no
  anchor, and the $5,500 opening-balance deposit to the checking account is on one of them; no
  decoded row holds it.
- **An adjustment line on an account that isn't in the account list** (bank accounts chapter):
  one inventory adjustment's value line posts to an account id the file no longer lists, so the
  adjustment is unbalanced ($699).
