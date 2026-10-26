# Advanced search (Find)

Testers want more than the ⌘K box: a way to find transactions by date, type, name, account, amount,
number and memo together, as QuickBooks Desktop's **Edit › Find** does.

## What we have

- **⌘K search** (`GlobalSearch.jsx` → `workflows::search`): one text box matched against memo, name,
  number and the exact total. No filters, at most 40 transactions, voids never show, and list hits
  (customer, vendor, product) open the whole list rather than the record.
- **Custom reports** (`custom_reports.rs`): the Custom Transaction Detail engine already filters
  posted lines by account, account type, name, transaction type, class, item, rep, paid status,
  cleared status, line amount, memo, voids and adjusting entries, and can show one row per
  transaction. Find builds on it rather than adding a second query engine.

## How others do it

| App | Shape | Worth copying |
| --- | --- | --- |
| QuickBooks Desktop, Edit › Find | Simple tab (type, name, date, number, amount) and Advanced tab: pick a filter from a long list, each lands in a "Current choices" box | Results with a count and total; **Go To**, **Report** and **Export** buttons; amount as =, ≤, ≥ |
| QuickBooks Online | Header search plus an Advanced search page (type, date, amount, ref no., memo, contact) | Recent transactions when the box is empty |
| Xero | A filter panel on each list (contact, reference, amount range, date range, status) | "Find & recode": act on what you found in bulk |
| Linear, GitHub, Gmail | Filters as removable chips, or typed as `type:bill amount:>500` | Typed tokens and chips build the same filter |

## Design

- **⌘K stays the quick lookup.** It gains an "Advanced search…" row that opens Find with the typed
  text, and (phase 2) understands typed filters (`type:bill vendor:acme amount:>500 after:2025-01-01`).
- **Find page** (Accounting › Find transactions): filters across the top, results below.
  - Results: one row per transaction (Date, Type, Num, Name, Memo, Account, Amount), sortable,
    with the match count and the total of the matches. Clicking a row opens the transaction.
  - Amounts are the transaction's total (as QuickBooks' Find does), not a line amount; a bound
    matches either sign.
  - Voided transactions and their reversals are left out unless "Include voided" is ticked.
- **Backend:** `find_transactions(query)` in `custom_reports/find.rs`. The query carries the
  engine's `Filters` (so later phases only add UI), plus the dates, a document number and a row
  limit. Line filters pick transactions (a transaction matches when any of its lines does);
  number and amount apply to the transaction as a whole.

## Phases

1. **Backend and Find page with the core filters** — text (name, number or memo contains), date
   range, transaction types, customers and vendors, accounts, amount (equal, at least, at most,
   between), number, include voided. Up to 500 rows newest first; the count and total cover every
   match. Opened from the Accounting menu and from ⌘K's "Advanced search…" row.
2. **Typed filters in ⌘K**, parsed into the same query, and Enter with nothing highlighted opens
   Find.
3. **More filters from the engine:** class, product/service, rep, paid status, cleared status,
   account type, 1099 vendors, adjusting entries; plus has-attachment and entered/changed by user
   (from the audit log), which the engine does not have yet.
4. **Act on results:** export (CSV/Excel), "Open as report" (the same filters as a Custom
   Transaction Detail report, one row per transaction), saved searches, and later bulk recode.

## Open questions

- Should ⌘K list hits open the customer/vendor/product record instead of the list? (Separate fix.)
- Find reads every posted line in the date range, as the custom reports do. Fine for desktop-size
  books; if large imported files feel slow, push the date and type filters into the SQL.
- Typed-filter syntax: QuickBooks users won't know it, so it stays optional; the filter fields
  are the primary UI.
