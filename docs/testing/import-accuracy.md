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
  within Inventory Asset, so it changes no account balance. Builds now come in with item
  quantities (gap R3c, below).
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
by hand, accounts that differ). Gap R3d tracks them; most are fixed in the R3d section below.

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

## Item quantities, 2026-11-11 (R3c)

A full import now tracks each inventory part and assembly the file tracks, and records every
change the file made to its stock as a stock movement on the entry its transaction became. The
check: after a full import, each item's quantity on hand and value in LocalBooks against the
quantity and value the file stores for it (the figures its stock status report shows).

### Where the file keeps stock

- **What it stores per item:** `abmc_item_internal` holds the quantity on hand, the value on hand
  and the average cost, signed as debits are (negative = stock on hand). The assembly's row also
  holds how many are in pending builds.
- **Each change** is a line on the item's inventory asset account that carries a quantity, signed
  as its amount (negative = into stock): purchase lines of bills, item receipts, checks and card
  charges; inventory adjustment lines; and the cost rows of invoices, sales receipts and credit
  memos (kept apart from the form line and linked to it by line id). A bill's own cost rows change
  the value of stock already sold, not its quantity.
- **Assembly builds** are in `abmc_item_assembly_lineitem`: the assembly's row puts it into stock
  and the component rows take them out, all on the inventory asset account; they post nothing in
  the file. The reader turns each build into a transaction whose lines cancel. The file marks no
  build as pending; it keeps the pending quantity on the assembly. The latest builds that add up to
  exactly that quantity are taken as pending and left out, since a pending build moves no stock.
  On the construction sample both builds (15 kits) are pending, and the file's quantities leave
  them out.

How to run it: `LOCALBOOKS_QBW_CHECK=<file> cargo test --release -- --ignored compares_stock_of_a_real_file --nocapture`
compares what the reader reads with what the file stores, per item, and lists each transaction
whose inventory asset lines don't add up to the stock it moves. The full-import check
(`posts_a_real_company_file` with `LOCALBOOKS_QBW_MODE=full`) now also prints each item that
differs after posting and the reason the import gives.

### Results (full import)

The contractor-course files and the real backup track no inventory, and neither does the textbook
appendix on company files. That leaves the construction sample, the textbook sample company (12
items) and 18 textbook chapter and appendix files (6 items each).

| File | Items matching / tracked | What differs, and why |
|---|---|---|
| Construction sample | 12 / 12 | — |
| 11 textbook chapter and appendix files (online banking, estimating, letters, getting started, lists, selling your product, payments, bills, graphs, sales tax, payroll) | 6 / 6 each | — |
| Textbook, memorizing transactions | 5 / 6 | One item's stored row doesn't decode, so the file's own quantity can't be compared; the import gives it 2 on hand, and every account matches |
| Textbook, customizing forms | 5 / 6 | The same item and reason |
| Textbook, reports | 3 / 6 | The same item; two items each have a sale whose cost rows don't decode, so the stock it took out is missing (1 and 3 units) |
| Textbook, invoicing for services | 5 / 6 | One item has a sale whose cost rows don't decode (3 units) |
| Textbook, bank accounts | 5 / 6 | The adjustment on an account the file no longer lists can't be posted (20 units) |
| Textbook, other accounts | 3 / 6 | One bill's lines lost their item and quantity columns from the catalog, so its 7 units ($389.85) can't be given to their items; Inventory Asset still matches |
| Textbook, setting up inventory | 2 / 6 | The bill and check lines don't decode (column types that don't match the rows), so the purchases are missing |
| Textbook sample company | 4 / 12 | Seven items are on transactions that can't be posted (damaged lines); one has a sale whose cost rows don't decode |

Every difference comes from a row the reader can't decode, the same faults gap R3d tracks; none
comes from how a decoded change is imported. After R3d (below) every item on every one of these
files matches. Each differing item is listed in step 2 of the
import, before and after posting, with the file's quantity and value, what was imported and the
reason when the import knows it.

### How the stock is recorded

- Changes are applied in date order. One that would leave quantity and value of different signs
  (LocalBooks refuses such stock: say, a sale recorded before the purchase on the same day) is
  carried into the item's next change. None of the samples needed this.
- A purchase into stock also sets the item's last unit cost, which LocalBooks uses to cost a sale
  beyond what is on hand.
- An item LocalBooks already tracked before the import keeps its stock and is listed; so is an item
  whose inventory account isn't an Other Current Asset account in LocalBooks. A file cost account
  that isn't a Cost of Goods Sold account is replaced by LocalBooks' own.
- A journal import doesn't track stock.

Regression tests on synthetic data: `qbw_stock_import::tests` (a full import with purchases, a
sale, a build and an adjustment; voiding an imported entry puts its stock back; the carry rule),
`qbw_stock::tests` (pending builds, line signs) and
`qbw_company::tests::sold_stock_moves_with_its_cost_rows`.

## Undecoded rows, 2026-11-12 (R3d)

Each row the R3 and R3c sign-offs listed was traced to its bytes on the sample files. Five faults
explained nearly all of them; four are fixed, with synthetic regression tests, and what is left
is described below with the reason it can't be read.

### Fixed

- **Sector 7 when its trailer slot overlaps the stored trailer.** When the last sector's rotation
  puts the 16-byte trailer slot within 16 bytes of the sector boundary, some data bytes land on the
  stored trailer. Each such byte moves on by the rotation again, to where the trailer byte it
  displaced went, until it lands below the trailer, sometimes in several steps. The reader
  placed them in order instead, so a few bytes at the start of sector 7 came out in the wrong
  order. That explained the inventory adjustment "on an account the file no longer lists" (bank
  accounts chapter): its offset line read account 0 and transaction 65546 for account 1 and
  transaction 10. Regression test: `qbw_pages::tests::sector7_mapping_handles_rotations_next_to_the_sector_boundary`.
- **Estimated sector-7 correction rows.** A rotation that no page shows with an all-zero sector 7
  has its correction row estimated by votes, position by position. Every exact row on the sample
  files repeats with the rotation as its period (below half a sector, after the first byte), so
  the votes one period apart are now added together. On the memorizing chapter, unchained rows in
  sector 7 of such pages went from 99 to 23. This fixed the item's stored quantity row, the
  inventory cost rows that didn't decode (reports, invoicing for services, the textbook sample,
  the construction sample) and most damaged line rows. Regression test:
  `qbw_pages::tests::estimated_correction_rows_pool_votes_one_period_apart`.
- **Catalog damage of three more kinds** (setting up inventory, other accounts):
  - a column definition whose type byte is one no column has (`company_id` of the check line table
    stored as type 122): it keeps its name and takes its type from its width;
  - a column lost after the last defined one, which leaves no gap in the ids (`snh_expenses` of the
    bill line table): a row that stores it sets its bit in the null bitmap past the defined
    columns' bits, so the reader adds that many columns and tries each as an id, an amount or a
    code, only after every reading without them has failed;
  - lost definitions whose rows read but can't be placed (`transaction_id` and `account_id` of an
    invoice line table, `item_id` and `quantity_qnty` of a bill line table): the definition comes
    from another table of the same file that has the same neighbouring columns, up to three on
    each side, at the same distances (a run can start at another id: check lines number theirs one
    lower than bill lines). A definition repeated under a damaged id past the table's last column
    (`open_qnty` as column 93 instead of 48) is dropped with the placeholders before it.

  Regression tests: `qbw_records::tests::a_column_with_a_damaged_type_reads_by_its_width`,
  `a_trailing_column_the_catalog_lost_is_found_by_its_bitmap_bit`,
  `a_lost_column_borrows_its_definition_from_tables_that_agree_around_it` and
  `a_lost_column_is_found_where_another_table_numbers_the_run_differently`.

### Results

Same 31 files and real backup as the R3 sign-off, code of this change. "Before" is the R3 sign-off.

| File | Read: stored balances matched (before → after) | Full import: matching / differing (before → after) | Items matching (R3c → after) |
|---|---|---|---|
| Real backup (`.qbb`) | 112 of 112 | 77 / 0 | none tracked |
| Textbook, estimating appendix | 83 of 85 → 85 of 85 | 14 / 3 → 17 / 0 | 6 of 6 |
| Textbook, letters appendix | 84 of 85 → 85 of 85 | 15 / 2 → 17 / 0 | 6 of 6 |
| Textbook, memorizing transactions | 82 of 82 → 85 of 85 | 18 / 0 → 20 / 0 | 5 → 6 of 6 |
| Textbook, other accounts | 95 of 95 | 25 / 0 | 3 → 6 of 6 |
| Textbook, reports | 82 of 84 → 85 of 85 | 18 / 2 → 21 / 0 | 3 → 6 of 6 |
| Textbook, setting up inventory | 71 of 86 → 84 of 86 | 2 / 3 → 13 / 3 | 2 → 6 of 6 |
| Textbook, invoicing for services | 83 of 85 → 85 of 85 | 16 / 2 → 18 / 0 | 5 → 6 of 6 |
| Textbook, bank accounts | 86 of 87 → 87 of 87 | 17 / 2 → 19 / 0 | 5 → 6 of 6 |
| Textbook, customizing forms | 77 of 85 → 84 of 85 | 9 / 2 → 15 / 2 | 5 → 6 of 6 |
| Textbook, getting started | 83 of 85 | 19 / 2 | 6 of 6 |
| Textbook sample company | 104 of 111 → 112 of 113 | 63 / 14 → 71 / 8 | 4 → 12 of 12 |
| Construction sample | 114 of 115 → 115 of 115 | 76 / 2 → 78 / 0 | 12 of 12 |
| Contractor, chapters 7 and 8 | 65 of 68, 58 of 65 → 68 of 68 each | 7 / 5, 5 / 5 → 12 / 0 each | none tracked |
| Contractor, chapters 10, 11, 12 | 54–55 of 60 → 69 of 70 | 6–8 / 3–5 → 12 / 2 | none tracked |
| Contractor, chapter 4 | 63 of 65 → 64 of 65 | 2 / 1 → 2 / 2 | none tracked |
| The other 13 files | all | all match | all |

Contractor chapter 4 shows one more differing account only because its checking account's stored
balance now reads: the account was not compared before, and the difference is the missing deposit
below. The textbook sample's 8 accounts are all one invoice, kept out of posting by its unread line.

### What is left, and why it can't be read

Each of these shows in step 2 of the import: a transaction with an unread line under "could not
be read completely", for entry by hand; the accounts it leaves short under the balances that
differ from the file; and, for pages that could not be decoded, their count, with a note that any
transactions on them are missing.

- **Sector 7 that can't be solved** (setting up inventory: check #2, $156.33; textbook sample:
  invoice 1054, one $600 line). No candidate correction reproduces the lengths of the rows the
  page's directory gives for its last sector, so its bytes are unknown.
- **Sector 7 solved only by estimate** (customizing forms: a $15,000 deposit line; contractor
  chapters 10 to 12: a $10,000 credit memo line). Their rotations (223 and 71) have no all-zero page
  in those files, and the pooled estimate still misreads a few bytes of these rows. Correction rows
  differ from file to file (none of 69 exact rows is the same in two files), so one file can't lend
  another its rows.
- **A run of pages with no anchor** (contractor chapter 4: the $5,500 opening-balance deposit to
  checking). 102 pages, 3135 to 3261, are not decoded. Under no cipher base and step does a page's
  sector 0 contain its own page number, which is what every other page is anchored by, so these
  pages were written some other way that the reader does not know.
- **An invoice line in no decoded row** (getting started: $35). No unread row and no header without
  lines accounts for it, so it is most likely on one of the file's few undecoded pages.
