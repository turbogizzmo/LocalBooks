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
  separate inventory record tables, linked to invoice and bill lines by line id rather than by
  transaction. The reader does not read them yet, so Cost of Goods Sold reads as zero and Inventory
  Asset is off on files with inventory items. Gap R3a.
- **Payroll.** Paychecks and payroll liability payments keep their payroll-item lines in tables the
  reader does not read, so payroll expense and liability accounts read as zero. Gap R3a.
- **Check lines in upgraded files.** In files upgraded from an older QuickBooks version, the column
  list the reader takes from the file's catalog for the check line table does not match how the rows
  are stored (nine column ids are missing), so those rows don't decode. Gap R3b.
- **A handful of unread rows.** A few transactions per file still have a line that could not be read;
  they are listed in step 2 for entry by hand and are left out of posting.

The real backup, which is the case users bring, matches in full.
