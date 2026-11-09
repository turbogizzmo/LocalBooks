# Condensing old years (gaps row R18)

R18 asked for old years to condense to opening balances, with an archive copy, and for reports
from the archive to still tie out. Condense Data (#107, migration 0063, `src-tauri/src/condense.rs`)
already does this. This note records the design that was chosen, why it differs from "opening
balances only", and where each part of the acceptance test is checked, so the row can be closed.

## What was asked, and what was built

| R18 asks for | Built as | Checked by |
|---|---|---|
| Old years condense | Every closed transaction dated on or before the cut-off is replaced by one summary journal entry per month, with a line per account and class | `condense::tests::condensing_harbor_lane_keeps_every_balance_and_open_item` |
| …to opening balances | Monthly summaries rather than one opening entry: the condensed file keeps month-by-month Profit and Loss and Balance Sheet for the old years, and its balance at the cut-off is the opening balance of the next day | the same test, comparing balances per account, class and month |
| Open items carry forward | Unpaid invoices and bills, unused credits, undeposited payments, uncleared bank and card lines, drafts and foreign-currency entries are kept whole at their original dates | the same test (aging and open items match) |
| An archive copy | `archive-through-<cut-off>-<time>.db` in the backups folder: the full history, opened read-only, with its own audit trail; plus a `pre-condense-<time>.db` copy to undo | `condense::tests` (archive opens read-only; audit verifies in both) |
| Reports from the archive still tie out | Before the condensed copy replaces the books it is compared with the archive: balances per account, class and month; A/R and A/P aging and inventory value at the cut-off and today; cleared balances and uncleared lines; every list. Any difference stops condensing | `condense::verify` and the harbor-lane test |

## Why monthly summaries and not one opening-balance entry

A single opening entry would leave the old years with nothing to report from in the current file:
last year's Profit and Loss, budget-versus-actual and prior-year comparisons would be empty, so
the user would have to open the archive for any year-over-year report. Monthly summaries keep those
reports exact in the current file (only the transaction detail moves to the archive), cost a few
hundred rows a year, and keep the balance at the cut-off the same as an opening entry would.

Starting a fresh file from opening balances was also considered and rejected: it would have to
replay every list, setting and open document into a new schema (about a hundred tables), while
condensing a copy of the existing file only removes rows and adds summaries, under the same
triggers and audit trail.

## Limits

- Condensing never rewrites the open books in place: it builds the condensed copy beside them and
  swaps it in only if the books did not change meanwhile.
- A new table must be classified in `condense::TRANSACTION_TABLES` or `KEPT_TABLES`; the test
  `every_table_is_either_a_list_or_a_transaction` fails until it is.
- Attachments of summarized transactions are a transaction table, so they leave with them and stay in the archive.

User guide: [Condense data](../guide/backups-and-company-files.md#condense-data-archive-closed-years).
