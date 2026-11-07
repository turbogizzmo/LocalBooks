# QuickBooks Desktop parity, the 2024 release

Rounds 1 and 2 ([desktop-parity.md](desktop-parity.md), [desktop-parity-2.md](desktop-parity-2.md))
read the 2021 desktop release. Testers moving off QuickBooks Desktop now often come from the 2024
release, so this pass reads that one and lists only what it adds or changes that LocalBooks lacks,
plus a few things round 2 missed. Rows `T40`–`T52` in [`docs/gaps.md`](../gaps.md) track it.

**Sources** (November 2026): the menu, center-menu, form-label, column and message resources of the
QuickBooks Desktop Pro 2024 installer, diffed resource by resource against the same resources of the
2021 installer that rounds 1 and 2 read. Both installers are kept outside the repo in
`research-samples/`, never committed and never run; the files were unpacked into a scratch folder
and deleted afterwards. Everything here is described in our own words. Each gap was checked against
the code (`src/components/`, `app_menu.rs`, `custom_reports.rs`, `list_reports.rs`, the
migrations) before being called missing.

**What changed between 2021 and 2024, in short.** The program files are packed the same way and the
center column definitions are byte-for-byte the same. The changes are: a Category List for items,
with a new item list built around it; credits offered on the invoice; a search box inside reports;
bill and purchase order approvals; uploading bills for review; a bill payment form of its own that
can be printed or emailed; a cash flow dashboard in place of the old cash flow projector; Duplicate
on the account, customer and vendor lists; a list of contacts per name; two new reports (inventory
turnover and invoice profitability); and a long tail of features that are payroll,
paid services or higher editions (below).

**Left out:**
- Everything rounds 1 and 2, [preferences.md](preferences.md) or `docs/gaps.md` already track.
- Payroll (overtime tracking, retirement plan sync, state forms), the vendor's paid services and
  sign-ups (bill pay, shipping labels, beneficial ownership filing, payment links), upsells, update
  and licence prompts, and tools for moving to the vendor's online product.
- Higher-edition features that appear in the shared menus: intercompany transactions across
  several companies, lot and serial numbers, expiration dates, bin locations, inventory sites,
  sales orders linked to purchase orders, assemblies, price rules.
- Customer prepayments on estimates and sales orders, and the link between an estimate and an
  ordinary (non-progress) invoice. 2024 added both, then its Help menu gained commands to turn them
  off for good, turning open prepayments into ordinary credits. A tester won't expect what the
  vendor withdrew; T24's unapplied credit covers the same need.
- The "advanced" vehicle mileage, which records trips from the vendor's phone app through its
  online service. LocalBooks' mileage covers the basic version.
- A panel of Windows settings (printers, updates, display) added to the Help menu.

Pain = how likely a tester is to notice (H/M/L). Effort S/M/L as in `docs/gaps.md`.

## Ranked gaps

| # | Gap | Where LocalBooks is now | Pain | Effort | Row |
|---|---|---|---|---|---|
| 1 | Item categories (new in 2024): a Category List with sub-categories, a category on each item ("Uncategorized" otherwise), the item list showing a category tree beside the items with Move to a different category, the item box on forms filtering by category, and reports filtered or grouped by category. A company preference turns categories on or off | Items have no category and no parent; the item list is one flat table | H | M | T40 |
| 2 | Credits offered on the invoice: when the customer has open credits, the invoice says so, lists them, and asks on save whether to apply them now | A credit memo is applied from the credit memo's side only; the invoice never mentions it | H | S | T41 |
| 3 | Search inside a report: type a word or amount, the report jumps to and highlights each match, with a message when there is none | Reports filter by their options; there is no find within the open report | M | S | T42 |
| 4 | The item record and item list: Subitem of, a purchase description, cost and preferred vendor on each item; the item list's search row (look for text in all fields, name, sales or purchase description, preferred vendor or custom fields), ticking several items for one action, and Import from / Paste from Excel. The item box on forms also has Find & Select Items, to search and tick several items to add at once (present in 2021 too; round 2 missed it) | Name, description, rate, income account, type and stock only; no search box on the item list; items come in only through company-file imports | M | M | T43 |
| 5 | A document inbox: Receipt Management (in 2021 too, missed by round 2) and Upload & Review Bills (new in 2024). Drop in PDFs or photos of receipts and bills, review each one beside a form, and turn it into an expense, check or bill with the file attached | A file can be attached to a form being entered or saved, but there is nowhere to put documents waiting to be entered | M | M | T44 |
| 6 | Bill and purchase order approvals: set up who must approve bills and POs (by amount or vendor), keep drafts, pending and rejected ones out of Pay Bills and the open PO list, an Approval Status on the forms and lists, and a Track and Approve list for approvers | Users and roles exist (#60); nothing waits for approval | M | M–L | T45 |
| 7 | A bill payment form of its own: a template like the invoice's, a print preview, and Email Now to send the stub to the vendor from Pay Bills or the bill payment | Bill payments print only as a check; T32 adds batch printing of stubs, but there is no layout, preview or email | M | S–M | T46 |
| 8 | A cash flow dashboard (Cash Flow Hub, on the Company and Reports menus, replacing the cash flow projector): bank and card balances, money coming in from open invoices and going out to open bills over the next weeks, and a forecast; plus the Cash Flow Forecast report the 2021 menu already had | The Home page shows balances and an A/R aging widget; no forecast of money in and out. Round 1 noted the forecast as low; 2024 makes it a menu item twice over | M | M | T47 |
| 9 | Duplicate on the Chart of Accounts, Customer List and Vendor List: a copy of the record to edit under a new name | Missing (T28 duplicates transactions, T30 merges names; neither copies a list entry) | M | S | T48 |
| 10 | Two new reports: Inventory Turnover by Item (quantity sold, turnover ratio and days, last sale date) and Invoice Profitability by Customer and Detail (per invoice: cost, sales price, difference, markup and margin) | Item and job profitability exist; neither report does. Invoice profitability needs an item cost (T43) | M | M | T49 |
| 11 | Commented reports (in 2021 too, missed by round 2): add a comment to any figure in a report, keep the commented copy and print it with its comments | Reports can be memorized, exported and printed; not commented | L | M | T50 |
| 12 | Manage Contacts: any number of contacts on a customer or vendor, each with a role, and which one is the default for invoices, statements and the like | Two contact names, phone, alt. phone, fax and email (T6) | L | M | T51 |
| 13 | Small additions: Documents › save all attachments to one zip file, and statements that show each transaction's P.O. number | Attachments are opened one at a time; statements show no P.O. number | L | S | T52 |

## Layout corrections

Tracked with the rows above.

- **Item list (T40, T43):** categories in a tree on the left, items on the right; above the items a
  search row (look for, in which field, Search, Reset) and the count of ticked items for batch
  actions; the menus over the list are Item, Activities, Report and Excel. Default columns are
  Name, Description, Type, Category, Account, quantity on hand and Price.
- **Item box on sales and purchase lines (T40, T43):** the first two entries are Add New and Find &
  Select Items (with a category filter), then the items.
- **Invoice (T41):** an Available Credits note beside the totals, listing the customer's other
  credits; saving asks before applying them.
- **Enter Bills, Create Purchase Orders and the open PO list (T45):** a Status (bills) or Approval
  Status (POs) beside the number; the open PO list notes that drafts, pending and rejected ones are
  not shown.
- **Receive Payments:** the totals under the grid speak of selected transactions, not invoices, and
  the empty grid says the customer has no unpaid transactions, since the grid can hold credits and
  charges as well as invoices. Fold into T37's Receive Payments totals.
- **Pay Bills (T46):** a bill payment stub can be emailed as well as printed from the payment.

## What we couldn't read

- **Dialog layouts and field order.** As in round 2, the form layouts are in a binary format we did
  not decode, so the layout notes above come from the labels' grouping and from public product
  knowledge.
- **The web-based screens.** The cash flow dashboard, Upload & Review Bills and the approval setup
  are separate add-ins that draw web pages; only their menu entries, messages and a few labels are
  in the files, so their detail comes from public product knowledge.
- **Which edition gets what.** The menu resources are shared by every edition, so the files don't
  say which features Pro has. We left out what is known to belong to the higher editions (above)
  and kept approvals, categories, inventory turnover and invoice profitability. Some of these may
  be limited to some editions or subscription tiers; that matters little, since testers judge
  LocalBooks against the edition they used.
- **Help.** The help files were not read.

## Order of work

T41 and T42 are small, visible and need no schema change, so they go first, with T48. T40 and T43
change the item record and share one migration and one item-list redesign, so they go together.
T45 needs a short design note first: what an unapproved bill is in the ledger (likely not posted
until approved).
