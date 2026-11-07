# Preferences: what QuickBooks Desktop users will look for

People moving from QuickBooks Desktop expect **Edit › Preferences**: a list of categories on the
left, each with a **My Preferences** tab (yours) and a **Company Preferences** tab (everyone's).
This note lists each category's options in our own words, what LocalBooks has, and what is worth
building. It is the plan behind `docs/gaps.md` row T3.

**Sources** (November 2026): the category labels of QuickBooks Desktop Pro 2021 for Windows and of
QuickBooks for Mac 2021, read from installers kept outside the repo (`research-samples/`, never
committed). The Jobs & Estimates, Time & Expenses and Reports company options, and parts of Sales &
Customers, come from public product knowledge, not the installer. A public three-part video tour of
the Preferences window (Steiner Business Solutions, "Guide to Quickbooks Preferences Menu", parts
1–3 on YouTube) confirmed the options per category and added the rows marked *(tour)*.

**Where LocalBooks keeps them.** Company options are keys in `preferences.rs` (Settings ›
Preferences, admins only). Your own options for this computer are in `src/lib/myPreferences.js`
(General) and `DesktopViewPreferences.jsx`. Some settings live on their feature's screen (Reminders,
Email, Sales Tax, Forms); the Preferences page should link to those rather than copy them.

Status: **has**, **partial**, **missing**. M = my preference, C = company preference. Effort S/M/L
as in `docs/gaps.md`.

## General

| Option | Tab | Status | Effort |
|---|---|---|---|
| Enter moves between fields; open name lists while typing; full text of clipped entries on hover | M | has (this round) | – |
| New transactions start on today or the last date entered | M | has (this round) | – |
| Beep when a transaction is recorded | M | has (T3 batch 1) | – |
| Type amounts without a decimal point (1234 → 12.34) | M | has (T3 batch 1) | – |
| Warn before editing a posted transaction; warn before deleting or voiding | M | has (T3 batch 1; nothing is ever deleted, so voiding) | – |
| Bring back messages you chose not to see again | M | has (T3 batch 1: "Don't show this message again" on one-time messages) | – |
| Recall the last transaction for a name, or pre-fill a vendor's usual accounts | M | missing | M |
| When an item changes on a line, keep a typed description or price: ask, always, never | M | has (T3 batch 1) | – |
| Show hours as decimals or hours:minutes | C | has (T3 batch 2) | – |
| Always show four-digit years | C | has (T3 batch 2) | – |
| Don't update a name's details from a transaction | C | missing | S |
| Save before printing | C | missing | S |
| Sign out after a period of no use | C | has (`auto_lock_minutes`) | – |

## Accounting (company)

| Option | Status | Effort |
|---|---|---|
| Show account numbers; show only the lowest subaccount | has (T3 batch 3: statements, trial balance and general ledger) | – |
| Require an account on every line | has (always: a line can't be saved without one) | – |
| Class tracking: on, per line or per transaction, warn when missing | has | – |
| Track expenses by customer or job | partial | M |
| Number journal entries automatically; warn when posting to Retained Earnings | has (numbers always automatic and unique; warning T3 batch 2) | – |
| Copy the first line's memo to the other journal lines (a my preference in the tour) | has (T3 batch 1, My Preferences › Accounting) | – |
| Warn when a date is more than N days in the past or future | has (T3 batch 2; 90 and 30 days) | – |
| Closing date, with a password to change entries before it | has (closing date and passcode) | – |

## Bills, Checking, Payments, Sales & Customers (company unless marked)

| Category | Option | Status | Effort |
|---|---|---|---|
| Bills | Bills due in N days by default | missing (terms only) | S |
| Bills | Warn about a bill number already used for that vendor | has (T3 batch 2) | – |
| Bills | Apply vendor credits and discounts automatically | missing | M |
| Checking | Default account for each form (checks, pay bills, deposits, sales tax) (M) | missing | S |
| Checking | Warn about a duplicate check number | has (T3 batch 2) | – |
| Checking | Date checks when printed; start at the payee | missing | S |
| Checking | Put the vendor's account number in the check memo *(tour)* | missing | S |
| Checking | Bank feed: how renaming rules apply to downloaded payees *(tour)* | partial (rules, no switch) | S |
| Payments | Apply a payment to open invoices automatically; work out the amount from the invoices ticked | partial (no switch) | S |
| Payments | Deposit to Undeposited Funds by default | partial (no switch) | S |
| Sales | Default shipping method, FOB and markup | partial (lists, no default) | S |
| Sales | Warn about a duplicate invoice number | has (always: invoice numbers are assigned and unique) | – |
| Sales | Price levels on or off | partial (always on) | S |
| Sales | Warn before selling to a customer with overdue invoices | missing | S |
| Sales | Billable time and costs: ask, add, or don't add | missing | M |
| Sales | Sales orders on or off, with their own duplicate-number warning and packing-slip template *(tour)* | missing (no sales orders) | L |
| Sales | Don't print lines with a zero amount when an estimate or order becomes an invoice *(tour)* | missing | S |
| Sales | Collections Center on or off *(tour)* | missing (no collections screen) | M |

## Reports, items, jobs, time, tax

| Category | Option | Status | Effort |
|---|---|---|---|
| Reports (M) | Show the report's options before running it | has (T3 batch 3: Profit and Loss, Balance Sheet, aging and sales summaries) | – |
| Reports | Summary reports default to accrual or cash | has (T3 batch 3: Profit and Loss and the Income & Expense graph; the Balance Sheet has no cash basis) | – |
| Reports | Age from the due date or the transaction date | has (T3 batch 3) | – |
| Reports | Show accounts and items by name, description or both | has (T3 batch 3: items in Sales by Item, Inventory Valuation and Stock Status; accounts by number and path, above) | – |
| Reports | Which accounts go in each Cash Flows section | has (T3 batch 3: Preferences › Reports & Graphs) | – |
| Items | Inventory and purchase orders on or off | missing | S |
| Items | Warn about duplicate PO numbers | has (always: PO numbers are assigned and unique) | – |
| Items | Warn or stop when stock would go below zero | missing | M |
| Items | Units of measure (sell by the each, case or pallet) *(tour)* | missing | L |
| Jobs & Estimates | Estimates on or off; rename job statuses (Pending, Awarded, In progress, Closed, Not awarded); progress invoicing | partial / missing | S–M |
| Jobs & Estimates | Warn about duplicate estimate numbers | has (always: estimate numbers are assigned and unique) | – |
| Time & Expenses | First day of the work week (Monday is fixed now); new time billable by default; time tracking on or off | missing | S |
| Time & Expenses | Count reimbursed expenses as income; mark all expenses billable; default markup percent and account; invoice from a list of time and expenses *(tour)* | missing | M |
| Sales Tax | Charge sales tax at all; most common tax item; taxable and non-taxable codes; mark taxable lines with a T on printed forms; owe tax when invoiced or when paid; filing monthly, quarterly or yearly | missing (see R11) | S–M |
| Finance Charge | Charge from the due date or the invoice date; mark charges to print | missing | S |
| Tax: 1099 | Do you file 1099s (on or off); map accounts to 1099 boxes | missing (see R10) | M |
| Reminders (M) | Show reminders when a company opens | partial | S |
| Reminders | Per reminder type: summary, full list or off, and how many days ahead *(tour)* | has (Reminders screen) | – |
| Send Forms | Default delivery method (email, mail, none) for new customers; a message template per form type *(tour)* | partial / missing | S |
| Reports | Default report formatting (fonts, header and footer) *(tour)* | left out: each report draws its own header and footer, and printed reports use one type style; a company-wide format needs a shared report layout first | M |
| Desktop View (M) | Show the Home page when a company opens; color scheme *(tour)* | partial (start page, no themes) | S |

## Already covered elsewhere

Desktop View (One or Multiple Windows, saved desktop, left pane, start page), Finance Charge rates
and account, Multiple Currencies (home currency, daily rates), Reminders (per type, lead days),
Send Forms (mail settings), Integrated Applications (Settings › AI: local access tokens).

## Left out on purpose

Payroll & Employees (no payroll), Service Connection and online payments (no Intuit services),
Calendar (no calendar screen), graph styling, search-index timing, webmail sign-in, and keeping the
app running in the background for a faster start. The Mac product's extra panes (sounds, address
book sync) don't apply either.

## Suggested batches (one PR each)

1. **Finish General (my preferences):** beep, decimal point, edit/void warnings, bring back
   messages, keep typed item details, recall last transaction / vendor account pre-fill.
2. **Data-entry guards (company):** duplicate-number warnings (checks, bills, invoices, POs,
   estimates), far-past/future date warning, require an account, Retained Earnings warning,
   journal auto-numbering, four-digit years, hours format.
3. **Reports and display defaults:** accrual/cash default, aging basis, names vs descriptions,
   account-number and lowest-subaccount switches, report options first, Cash Flows mapping.
4. **Sales, payments and bills defaults:** shipping/FOB/markup defaults, price levels and estimates
   switches, auto-apply and auto-calculate payments, Undeposited Funds default, overdue-customer
   warning, bill due days, auto-apply vendor credits, email-later default.
5. **Tax and time:** sales tax basis and frequency (R11), finance charge basis, work-week start and
   billable defaults, 1099 account mapping (R10), negative-stock warning.

The Preferences page is now the familiar category list, with My and Company tabs, so each option is
where people look for it (T3, with batches 1 and 2). A category whose settings live on their own
screen links there. Warnings before saving share one design: the backend's guards
(`src-tauri/src/guards.rs`) refuse with a typed `NEEDS_CONFIRMATION` error listing each warning,
and the form asks and saves again with **Save anyway** (`src/lib/confirmable.js`).

Batches 1 and 2 are done except **recall the last transaction for a name / pre-fill a vendor's
accounts**, which stays in batch 1's row above as missing.
