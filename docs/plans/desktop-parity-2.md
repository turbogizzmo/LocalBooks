# QuickBooks Desktop parity, round 2

Round 1 ([desktop-parity.md](desktop-parity.md)) found the gaps testers hit first, and rows T4–T9
built them for 0.11 and 0.12. This round looks again, now that those exist, for what a tester coming
from QuickBooks Desktop will still miss: menu items, dialogs, shortcuts, and form layouts that
differ. Rows `T24`–`T37` in [`docs/gaps.md`](../gaps.md) track it.

**Sources** (November 2026): the menu, form-label, center-column and message resources in the
QuickBooks Desktop Pro 2021 installer, cross-checked against the menus of the QuickBooks for Mac
2021 installer. The installers are kept outside the repo in `research-samples/`, never committed
and never run. Field order on forms comes from public product knowledge, because the dialog layouts
are in a binary format we did not decode. Everything here is described in our own words.

**Left out:** what round 1, [preferences.md](preferences.md) or `docs/gaps.md` already track;
payroll, online banking sign-up and the vendor's own services; and features of the higher editions
(sales orders, assemblies, inventory sites, batch entry).

Pain = how likely a tester is to notice (H/M/L). Effort S/M/L as in `docs/gaps.md`.

## Ranked gaps

| # | Gap | Where LocalBooks is now | Pain | Effort | Row |
|---|---|---|---|---|---|
| 1 | Receive Payments takes an overpayment, a prepayment, or a payment from a customer with no open invoice, and asks whether to keep the extra as a credit, refund it or write it off. The centers show an Unapplied Amount on payments and Remaining Credit on credit memos | Payments above an invoice's balance are refused (`INV_OVERPAYMENT`); customers without open invoices aren't listed | H | M–L | T24 |
| 2 | The rest of the desktop shortcuts: Ctrl+N new, Ctrl+P print, Ctrl+E edit, Ctrl+G go to the other side of a transfer, Ctrl+L open a box's list, Page Up / Page Down for previous / next | `formKeyCommand` (`src/lib/formFlow.jsx`) handles Alt+S/N, Ctrl+D/M and Ctrl+Insert/Delete | H | S | T25 |
| 3 | The Reports menu reaches every built report: customer and vendor balances, open invoices, aging and unpaid bills detail, deposit and check detail, purchases, P&L and balance sheet detail, income by customer, the transaction lists, the journal, adjusting entries and the list reports; plus List and Memorized Reports submenus, QuickReport, Transaction History and Journal, and Process Multiple Reports | `REPORTS` in `app_menu.rs` has a handful under Company & Financial; the rest are only in the Report Center | H | S | T26 |
| 4 | A warning when writing a check to a vendor with open bills (pointing to Pay Bills), and when depositing or recording a sales receipt from a customer with open invoices (pointing to Receive Payments) | Nothing warns; paying twice or leaving bills open is easy | H | S | T27 |
| 5 | Duplicate a transaction from the form, the Edit menu and a list row's menu, with Ctrl+O as on the desktop (free in LocalBooks) | Missing | H | S | T28 |
| 6 | Cash back on Make Deposits: an account, memo and amount kept out of the deposit | Missing | M | S | T29 |
| 7 | Merge two customers, vendors, items or accounts (renaming one to the other's name offers to merge) | Missing; a duplicate name is refused | M | M | T30 |
| 8 | Choose and order the columns of lists, centers, registers, Reconcile and form grids | Fixed columns everywhere | M | M | T31 |
| 9 | File › Print Forms for credit memos, sales receipts, purchase orders, mailing labels, bill payment stubs and 1099s / 1096 | Invoices, checks and statements only; some need T11/T23 layouts first | M | M | T32 |
| 10 | Edit-menu commands for the open list (New, Edit, Delete, Make Inactive, Show All) and the centers' toolbar menus (New Transactions, Print, Excel) | Row menus and an export menu only | M | S–M | T33 |
| 11 | Inventory reorder points with an "inventory to reorder" reminder, and an Inventory Activities submenu | Missing; the Items & Inventory preference text already mentions reorder points | M | S–M | T34 |
| 12 | Batch invoices (one invoice to many customers) and payment reminders (review, send, schedule, by customer group) | Missing | M | M | T35 |
| 13 | Report presets still missing: Sales by Item Detail, Budget Overview, Sales Tax Revenue Summary, Memorized Transaction Listing, To Do Notes, Item Price List, Sales by Ship To Address, Realized and Unrealized Gains & Losses | Missing | M | M | T36 |
| 14 | Menu entries for what already exists: View › One Window / Multiple Windows, Open Window List and Refresh (F5); Lists › Currency List and Vehicle List; Vendors › Sales Tax › Pay Sales Tax; Help › Check for Updates; File › Utilities › Import / Export | Reached from Settings or the sidebar only | L | S | T26 |
| 15 | Copy Line / Paste Line in line grids | Missing | L | S | T25 |

## Layout corrections

Tracked together as T37.

- **Write Checks, Enter Credit Card Charges, Pay Bills, Make Deposits:** show the chosen account's
  ending balance beside the account box.
- **Pay Bills:** Filter By vendor and Sort By above the bill list; under it, a panel for the
  highlighted bill with Go to Bill, Set Discount and Set Credits.
- **Receive Payments:** totals under the grid for amount due, applied, and discounts and credits
  applied, then the over- or underpayment choice (with T24).
- **Enter Bills:** a Discount Date next to Terms and the due date.
- **Sales line grid:** Item, Description, Qty, Rate, Class, Amount, Tax, in that order; the income
  account comes from the item and needn't be a column by default.

## Order of work

T25, T26 and T27 are small and visible, so they go first, together with T28 and T29. T24 changes
how payments post and needs a short design note of its own before code. T37 can go one form at a
time alongside the others.
