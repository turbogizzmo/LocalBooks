# Desktop parity, the Mac release

The earlier rounds ([desktop-parity.md](desktop-parity.md), [desktop-parity-2.md](desktop-parity-2.md),
[desktop-parity-2024.md](desktop-parity-2024.md)) read the Windows desktop releases. Some testers
come from QuickBooks for Mac instead, whose screens are built differently: a detail panel beside
every center and form, a calculator tape in amount boxes, customer groups, renaming rules for bank
downloads, an attachments library and a customizable snapshot page. This pass reads the Mac 2024
release and lists only what it has that LocalBooks lacks and the earlier rounds don't already
track. Rows `T53`–`T65` in [`docs/gaps.md`](../gaps.md) track it.

**Sources** (November 2026): the names of menus, windows, form fields, list columns and report
groups in the Mac desktop software LocalBooks replaces (QuickBooks for Mac 2024, compared with
2021, with 2012 skimmed for features since dropped), as a user sees them, and public product
knowledge for behaviour. Nothing from that software is in this repo; everything here is described
in our own words. Each gap was checked against the code (`src/components/`, `app_menu.rs`,
`custom_reports.rs`, `list_reports.rs`, the migrations) before being called missing.

**What the Mac release has, in short.** The menus match the Windows product closely, so most of
it is already tracked. What is its own: the information panel on the customer, vendor, item and
other-name lists and beside each form; customer groups, manual or kept up to date by rules, used
for batch invoices and for scheduled payment and statement reminders; an amount-box calculator
with a tape; renaming rules and matching for downloaded bank transactions; an attachments library
for every file attached anywhere; a report center with search, favorites, recent reports and a
picture of each report; scheduled reports sent by email or saved to a folder; importing
transactions of each type from a spreadsheet; text size buttons on every list and register; and a
snapshot page of widgets that can be added and arranged.

**Left out:**
- Everything the earlier rounds, [preferences.md](preferences.md) or `docs/gaps.md` already track.
  In particular batch invoices and payment reminders (T35), the receipt and bill inbox (T44),
  duplicating records (T48), contacts (T51), merging (T30), item search and Find & Select Items
  (T43), customizable columns (T31), the cash flow forecast (T47) and reorder points (T34).
- Payroll (paycheck forms, the payroll partner's import), card processing and the merchant
  center, payment links on invoices, the bill pay service, online banking sign-up and the vendor's
  categorization service, photo album syncing through a cloud service, and the vendor's feedback,
  guided tour, licence, update and upgrade screens.
- Moving to the vendor's online product or to the Windows product, and the Mac company-file format
  (researched elsewhere).
- Syncing with the macOS Contacts app and the sound effects (left out in preferences.md), the
  standard macOS Speech, Spelling and Substitutions menus, and the scripting dictionary
  (LocalBooks' local MCP server serves the same need).
- Multi-user mode (R15), the accountant's copy (R14), Canadian GST, and what earlier rounds marked
  low and left without a row: Mark as Pending, purchase orders from an estimate, progress invoicing.

Pain = how likely a tester is to notice (H/M/L). Effort S/M/L as in `docs/gaps.md`.

## Ranked gaps

| # | Gap | Where LocalBooks is now | Pain | Effort | Row |
|---|---|---|---|---|---|
| 1 | Matching downloaded bank lines: a downloaded line that agrees with a check, deposit or payment already entered (amount, date, check number) is matched to it instead of being entered twice, with Match / Unmatch by hand, a Show matched switch, and Add several at once. Renaming rules: when a downloaded payee begins with, contains or ends with some text, use a chosen name; a rule is offered when the user changes a payee, and the rules have a list to search, edit and remove. In Windows too, but no round tracked it | Each line is added as a new expense or deposit, or excluded. Category rules are learned silently (`bank_rules`) and have no list; the payee is retyped on each line | H | M | T53 |
| 2 | Reconcile leftovers: a service charge and interest earned (amount, date, account, class) entered when starting, posted with the reconciliation; Undo Last Reconciliation; the bank statement attached to the reconciliation. In Windows too, untracked | The begin step takes the ending date and balance only; a finished reconciliation is locked for good; attachments go on posted entries only | H | M | T54 |
| 3 | A calculator in amount and quantity boxes: typing `+`, `-`, `*` or `/` opens a small tape that keeps the running total, Enter puts the result in the box. Mac users lean on it; the Windows product does the same arithmetic without the tape | Amount boxes take a number only (`parseMoneyToCents`) | M | S | T55 |
| 4 | The information panel: beside the customer, vendor, other-name and item lists, and on each form for the chosen name, a panel with a summary (open balance; for customers active estimates and unbilled time and costs, for vendors POs still to receive), phone and email, notes, and recent transactions. For items: on hand, on order, average cost, value, reorder point, preferred vendor and links to the item's reports. The centers also have a Notes tab with a date stamp and New To Do, and an Emails tab listing what was emailed to the name. Round 1 noted the panel without a row | The lists show name, email and balance; notes are a box on the record; email history is kept per document, not per name | M | M | T56 |
| 5 | Customer groups: named groups of customers, either picked by hand or automatic, where rules on balance, type, terms, rep, state, ZIP code, tax code, credit limit or active status keep the members up to date. The Customer Center switches between customers and groups. A group can be chosen on an invoice (making a batch, T35) and carries scheduled statement reminders (which transactions, which template, the email message) as well as payment reminders (T35) | No groups; T35 assumes them | M | M | T57 |
| 6 | An attachments library: one window listing every attached file with its date, keywords and what it is attached to, with search, preview, save a copy and delete. Files can be attached to customers, vendors, items, accounts, estimates, purchase orders and reconciliations as well as transactions, each marked to go with the form when it is emailed or printed, and can come straight from a scanner | Files attach to posted transactions only (`attachments.journal_entry_id`), one form at a time; nothing lists them all; emailed forms carry the PDF only | M | M | T58 |
| 7 | The report center: a search box over report names, Favorites and Recent groups beside Memorized, a small picture and a one-line description of each report, and the date range chosen in the center before the report runs | A grouped list of names; options are set after the report opens (or first, by preference) | M | S–M | T59 |
| 8 | Sending reports: Email as PDF from any report, list, register or the reminders list, and a Scheduled Reports center: choose reports, how often (daily to quarterly, with start and end), where they go (email as attachments, or a folder, as PDF or spreadsheet, optionally password-protected) and the email message | Reports print, save to PDF or export to Excel; nothing emails them or runs them on a schedule | M | M | T60 |
| 9 | The home page and Company Snapshot: a home page drawn as the usual flow (estimates to invoices to payments to deposits; bills to payments) whose icons open the forms, and a snapshot page of widgets the user adds, removes and arranges: chosen account balances, customers who owe, vendors to pay, income and expense trend, income and expense breakdowns, year over year by account, top customers, items and vendors, notes, to-dos and reminders, each with its own period | The Dashboard has fixed tiles (assets, liabilities, A/R aging), a chart, the trial balance and reminders | M | M | T61 |
| 10 | Importing from a spreadsheet into the open company: customers and vendors, and transactions of each type (invoices, sales receipts, payments, credit memos, bills, bill credits, checks, deposits, card charges and credits) from columns the user maps, with a blank template to fill and the rows that would fail shown before anything posts; plus general journal entries from an accountant's file | Imports read QuickBooks company files, IIF, QIF, GnuCash and QuickBooks' own report exports; there is no import of a spreadsheet the user laid out. Items are T43 | M | M–L | T62 |
| 11 | Text size: larger and smaller buttons (and Cmd/Ctrl + and −) on lists, registers, forms and reports, remembered per kind of window; a color per bank account register | One fixed size; the window zoom is the only way to enlarge | M | S | T63 |
| 12 | Statement charges: Enter Statement Charges, a register per customer where charges are entered without an invoice and billed on the next statement. Round 1 noted it without a row | Statements list invoices, payments and credits only | L | M | T64 |
| 13 | Small additions: print an envelope from a form or a center row (with an envelope layout in Custom Forms); drag to put the chart of accounts, items and customers in a custom order, and switch a list between flat and tree views with Expand / Collapse all | No envelopes; lists sort by column only; the chart of accounts is always a tree, other lists always flat | L | S | T65 |

## Layout corrections

Tracked with the rows above.

- **Money bar on Sales, bill bar on Bills (T61):** the Mac bars also show unbilled time and costs
  and active estimates (sales), open purchase orders and card charges (bills). Each segment can be
  clicked to list those transactions under the bar, with the action that fits (receive payment,
  pay bill, print). LocalBooks' `MoneyBar` is three fixed totals that can't be clicked.
- **Forms (T56):** each form window can show, on its left, the saved forms of that type with a
  search box and sort (date, number, amount, name, due date, open balance), and on its right the
  information panel for the chosen name. Both can be hidden.
- **Customer Center (T56, T57):** a View switch between customers and groups; tabs under the list
  for Transactions, Notes, Emails and attachments.
- **Item list and chart of accounts (T56, T65):** the information panel on the right; a flat or
  tree view; Quantity on PO as a column.
- **Reconcile (T54):** the begin step has the service charge and interest earned rows, each with
  its date and account, under the ending balance.
- **Downloaded transactions (T53):** matched lines are marked, can be hidden, and can be removed
  together once matched.

## What we couldn't see

- **Field order on forms.** As in earlier rounds, the notes above come from how the labels are
  grouped, not from exact positions.
- **The snapshot widgets and graphs.** They are drawn inside web views, so only their titles and
  options were visible; the charts themselves come from public product knowledge.
- **The report pictures.** The Mac release ships a picture of each report for the report center.
  LocalBooks must draw its own (from a synthetic sample company) or show a description only;
  nothing of the vendor's may be reused.
- **The accountant's journal file.** The Mac product imports general journal entries from a file
  the Windows accountant edition writes. We did not find a public description of that format, so
  T62's journal part may end up taking IIF or a spreadsheet instead.
- **What 2012 had that 2024 dropped.** Only report format and header/footer inspectors, which
  preferences.md already leaves out. Nothing else a tester would miss.
- **Help.** The product's help was not read.

## Order of work

T55 and T63 are small, visible and need no schema change, so they go first. T53 and T54 are the
ones testers hit soonest: T53's matching needs the register lookups T7 built, and T54's undo needs
a short design note (a locked reconciliation is unlocked only by a new, audited reversal of the
lock, never by editing it). T57 goes before T35, which builds on its groups. T59 then T60 (the
schedule needs the email set-up the app already has, and only runs while LocalBooks is open). T56
and T61 share the summary queries, so they go together. T58 needs a migration that lets an
attachment point at a record other than a journal entry. T62 needs a design note on column
mapping and reuses the import review screen.
