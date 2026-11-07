# Desktop parity: what testers will look for next

Testers moving to LocalBooks from QuickBooks Desktop keep running into pieces LocalBooks doesn't have yet ("no
Add new vendor", "can't double-click an invoice", "where are classes?"). This note surveys the rest
of the desktop product, area by area, so we fix them before testers find them. It covers menus,
transaction forms, centers and lists, registers, reports and keyboard shortcuts. Preferences have
their own note, [preferences.md](preferences.md). Rows `T4`–`T9` in `docs/gaps.md` track the
batches below.

**Sources** (November 2026): the list, transaction and report names of the desktop software
LocalBooks replaces, as a user sees them, and public product knowledge for form fields,
behaviours, register details and keyboard shortcuts. Nothing from that software is in this repo;
everything is described in our own words.

**Already done** (#146, #147): the form toolbar (Find, Previous/Next, New, Save, Void, Print,
Email, Attach, Memorize), payment-method tiles, Add new vendor, a vendor's or customer's
transactions, register sorting and drill-down, double-click to edit, and class quick-add.

Pain = how likely a tester is to hit it (H/M/L). Effort S/M/L as in `docs/gaps.md`.

## Top 15 by tester pain

| # | Gap | Where LocalBooks is now | Effort | Batch |
|---|---|---|---|---|
| 1 | Date-box shortcuts: `t` today, `+`/`-` a day, `w`/`k` week, `m`/`h` month, `y`/`r` year | native date inputs (83 places) | M | T4 |
| 2 | Save & Close, Save & New and Clear on every form, with Alt+S / Alt+N | one save button, then the form resets | S | T4 |
| 3 | Add new from the item column on sales and purchase lines | `QuickCreateModal.jsx` exists but isn't used there | S | T5 |
| 4 | Add new customer from the Customer:Job box | missing (T2) | S | T5 |
| 5 | A Terms list (named, with discount days and percent) used on forms and customers | four fixed terms; customers store days only | M | T5 |
| 6 | Bill To / Ship To address blocks, P.O. No. and FOB on sales forms | Rep, Ship Via, Ship Date only | M | T6 |
| 7 | Print later: queue checks and invoices, then File › Print Forms › Checks / Invoices | one-off print | M | T8 |
| 8 | Profile lists: Payment Method, Customer / Vendor / Job Type, Customer Message | Sales Rep and Ship Via only | M | T5 |
| 9 | Type a check or deposit straight into a register (entry row, Record / Restore) | registers are read-only | L | T7 |
| 10 | Filter a customer's or vendor's transactions by type and date | only "Show voided" | S | T7 |
| 11 | Item types: Subtotal, Discount, Other Charge, Group | rate items with optional stock | L | own design note |
| 12 | Edit-menu actions on the open form: Void, Memorize, Insert / Delete line (Ctrl+D, Ctrl+M, Ctrl+Insert, Ctrl+Delete) | toolbar only (#147) | S | T4 |
| 13 | Fuller customer record: second address, contacts, credit limit, account no., type | one address, terms, rep, price level, tax | M | T6 |
| 14 | QuickReport (Ctrl+Q) from a register, account, name or item; Transaction History and Journal (Ctrl+H, Ctrl+Y) | missing | S | T7 |
| 15 | List and detail reports people reach for: Transaction Detail by Account, Transaction List by Customer / Vendor, Phone and Contact Lists, Voided/Deleted, P&L by Month | missing | M | T9 |

## By area

### Menus

- **File:** Print Forms has only Statements; a print-later queue for checks, invoices and labels
  is missing (T8). File Dentist exists in Settings but not as File › Utilities › Verify / Rebuild
  (S). Portable file, Accountant's Copy and multi-user are R14 / R15.
- **Edit:** acting on the open form (Void, Delete line, Insert line, Memorize, Duplicate, Go To
  Transfer) isn't in the menu (T4).
- **View:** no menu item for the Open Window List; no icon-bar placement (low).
- **Lists:** missing the Customer & Vendor Profile lists (T5), Other Names (M), and Fixed Asset
  Items (low). Payroll lists don't fit.
- **Favorites:** no user-chosen favorites menu (low).
- **Company:** missing a Calendar, a Documents center, Write Letters, Export Addresses and the
  cash-flow forecast (low).
- **Customers:** no Income Tracker (a dashboard of unbilled, unpaid and paid amounts) and no
  statement charges. Billing time and costs on an invoice covers mileage only (M).
- **Vendors:** no Bill Tracker; no Receive Items, Item Receipt or Enter Bill for Received Items
  (T8); no Adjust Sales Tax Due (R11).
- **Banking:** no Record Bounced Check, credit-card pay-down shortcut or Loan Manager (T8). Order
  Checks doesn't fit.
- **Reports:** no Process Multiple Reports (T9).

### Transaction forms

- **Save flow:** Save & Close / Save & New / Clear, and Mark as pending (T4; pending is low).
- **Quick add:** Customer:Job, item, terms and payment method (T5).
- **Sales header:** Bill To / Ship To, P.O. No., FOB, a template picker on the form, a Customer
  Message list (T6). Service Date per line (low).
- **Tax:** a tax code per line and per customer, beyond the taxable checkbox (R11).
- **Lines:** item types (Subtotal, Discount, Group, Other Charge, Payment, Sales Tax) need a design
  note of their own (L). Insert line is missing (T4).
- **Payments:** early-payment discounts in Receive Payments and Pay Bills; per-bill discount and
  credit, and filtering bills by due date (M).
- **Checks and bills:** Billable checkbox on cost lines (M); a "to be printed" flag and the print
  queue (T8). Cash back on deposits needs checking.
- **Journal entries:** Name and Billable columns, and Reverse (S).
- **Purchasing:** Item Receipt (M). For estimates: Mark as accepted and Create PO from an estimate
  (low).
- **Buttons:** Transaction History and Transaction Journal (T7).

### Centers and lists

- **Transactions pane:** filters by type, status and date (T7). A Transactions tab across every
  name (M).
- **Info panel:** contact, notes, to-dos, QuickReport and open balance at a glance (M).
- **Customer record:** second address, contacts, type, credit limit, account no., job info with
  status and dates (T6).
- **Lists:** Terms, Payment Method, Customer / Vendor / Job Type and Customer Message (T5); Other
  Names (M); add or edit many list entries at once (M).
- **Items:** Activities and Reports menus per item (M).

### Registers

An entry row with Record / Restore (T7, L), Go To, QuickReport, a one-line view, a date filter and
printing (T7, S each).

### Reports

LocalBooks has the core financial, A/R, A/P, sales, purchase, job, inventory, banking and
accountant reports. Missing (T9), by group:
- **Company:** P&L YTD comparison, P&L by month, Balance Sheet summary, Net Income detail.
- **Customers:** Transaction List by Customer, Phone and Contact Lists, Average Days to Pay.
- **Sales:** Sales by Rep detail, pending sales.
- **Jobs and time:** Time by Job / Name / Item, Unbilled Costs by Job, Job Costs detail.
- **Vendors:** Transaction List by Vendor, Phone and Contact Lists, Unpaid Bills by job.
- **Purchases:** Open PO detail and by job.
- **Inventory:** Physical Inventory Worksheet, stock status by vendor.
- **Accountant:** Transaction Detail by Account, Audit Trail as a report, Voided/Deleted
  transactions, Customer / Vendor / Item / Account listings, Closing Date exceptions.
- **Report features:** columns by month, quarter, class or customer on any summary report;
  collapse and expand rows; Process Multiple Reports.

Payroll reports don't fit.

### Keyboard shortcuts

Date-box keys (T4, the one testers miss most), `+`/`-` to step a check number, Ctrl+N / Ctrl+S /
Ctrl+P, Alt+S / Alt+N, Ctrl+D, Ctrl+M, Ctrl+E, Ctrl+Q, Ctrl+G, Ctrl+H, Ctrl+Y, Ctrl+Insert,
Ctrl+Delete, Ctrl+L to open a field's list, Page Up / Page Down for Previous / Next, and Alt-key
button mnemonics (T4). Ctrl+A stays Select All by design. Ctrl+I, J, T, R, W, F and the window
keys exist.

## Batches (one PR each, built on #147)

- **T4. Form keyboard and save flow (S–M).** A shared date box with the date keys; check-number
  `+`/`-`; Save & Close / Save & New / Clear; Alt and Ctrl shortcuts; Edit-menu items acting on the
  open form.
- **T5. Lists and quick add (M).** Terms, Payment Method, Customer / Vendor / Job Type and Customer
  Message lists (migration, `commands/profile_lists.rs`); Add new in the item column and the
  Customer:Job box (T2); Terms and Payment Method dropdowns; Lists › Customer & Vendor Profile Lists.
- **T6. Sales header and customer record (M).** Bill To / Ship To, P.O. No., FOB, template picker,
  Customer Message, Service Date; second address, contacts, credit limit, account no., type.
- **T7. Registers and QuickReport (M–L).** Register entry row with Record / Restore, Go To,
  one-line view, date filter; QuickReport (Ctrl+Q) from registers, accounts, names and items;
  Transaction History and Journal; type and date filters on a name's transactions.
- **T8. Print queue and banking (M).** "To be printed" on checks and invoices, File › Print Forms;
  check address block; Record Bounced Check; credit-card pay-down; Item Receipt and receiving a PO.
- **T9. Reports fill-in (M).** The missing list and detail reports above (mostly presets in
  `custom_reports.rs`), columns by period or class, Process Multiple Reports.

Item types (Subtotal, Discount, Group, Other Charge) need a design note before any code.
