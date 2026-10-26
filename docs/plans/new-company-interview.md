# New Company setup — plan (modeled on QuickBooks)

Status: **built** (`src-tauri/src/company_setup.rs`, `NewCompanyDialog` in `src/components/CompanyFiles.jsx`).
Decisions: copy QuickBooks' account names, numbers, types and core/recommended flags; LocalBooks writes
its own descriptions; set-up companies use five-digit numbers; Form 990 added to the tax forms.

## Where this comes from

The first draft of this plan was written from memory of QuickBooks' EasyStep Interview. This
version is based on how QuickBooks actually does it, taken from the QuickBooks for Mac 2024
installer in `research-samples/` (gitignored; mounted read-only, never run):

| File in `QuickBooks 2024.app/Contents/Resources/` | What it holds |
|---|---|
| `NewCompanySetup.nib` | The New Company screens: their fields, labels and buttons |
| `Interview.adr` (XML) | The 33 industries, which starter chart each one uses, and each industry's default feature settings |
| `StarterFiles.adr` (XML, 459 KB) | The starter chart for each industry: 1,684 account rows, each tagged **core** or **recommended**. Some industries also include starter items, classes, job types and terms |
| `BusType.adr` (XML) | The equity accounts for each income tax form |
| Help: `pgs/12b-file/ref_setupcoa.htm` | "QuickBooks suggests a default set based on your Industry selection… select or deselect… click Default to go back" |

## How QuickBooks does it

QuickBooks Mac uses three screens and asks no "do you have employees?"-style questions:

1. **Enter your address and tax information.** Company name, legal name, address, country,
   Federal ID, company start date, **Income Tax Form Used**, first month of the fiscal year,
   first month of the tax year.
2. **Choose your industry and chart of accounts.** An Industry menu, then a list of accounts
   with checkboxes: *"We've preselected accounts we think you will need for your type of
   business. You can deselect any of these or select more accounts."* A **Default** button
   restores the suggested set.
3. **Expert settings** (optional tab): *Customers are charged sales tax* (with sales tax items),
   *Customer and/or job estimates are prepared*, *Inventory and purchase orders are used*,
   *Keep track of time*, and *Summary Reports Basis* (cash or accrual). Each industry has its
   own defaults for these in `Interview.adr`. For example, a restaurant defaults to sales tax
   on, estimates off and time tracking on.

How the chart is put together:
- **Industry accounts.** Every industry's starter chart has about 15–35 **core** accounts
  (checked by default) and 20–40 **recommended** accounts (listed, unchecked). Non-Profit is the
  outlier, with 44 core and 108 recommended. A few accounts are subaccounts, such as the
  insurance types under *Insurance Expense*.
- **One shared catalog.** All 33 charts draw from 405 distinct accounts with 5-digit numbers,
  and no number is used for two different accounts. This is the same design as the catalog in
  the first draft.
- **Tax form → equity accounts** (from `BusType.adr`):

  | Tax form | Equity accounts |
  |---|---|
  | Sole proprietorship (1040) | Owners Draw 30800, Owners Equity 32000 |
  | Single-member LLC | Members Draw 30700, Members Equity 32000 |
  | Partnership / LLP (1065) | Partner 1 Equity, Partner 1 Draws, Partner 2 Equity, Partner 2 Draws, Retained Earnings 32000 |
  | Multi-member LLC | Member 1 / Member 2 Equity and Draws, Retained Earnings 32000 |
  | Corporation (1120) | Capital Stock 30100, Dividends Paid 30200, Retained Earnings 32000 |
  | S corporation (1120-S) | Capital Stock 30100, Shareholder Distributions 31400, Retained Earnings 32000 |
  | Nonprofit (990) | Unrestricted Net Assets 32000, Temp. Restricted Net Assets 31500, Perm. Restricted Net Assets 31300 |
  | Other/None | Retained Earnings 32000 |

  The account numbered 32000 is QuickBooks' retained-earnings account, renamed for the entity
  type (*Owners Equity*, *Members Equity*, *Unrestricted Net Assets*).
- **What the starter charts don't include:** bank accounts, A/R, A/P, Undeposited Funds,
  Inventory Asset, Sales Tax Payable and payroll accounts. QuickBooks creates these itself when
  you first use the matching feature.

### The 33 industries

Accounting or Bookkeeping · Advertising or Public Relations · Agriculture, Ranching, or Farming ·
Art, Writing, or Photography · Automotive Sales or Repair · Church or Religious Organization ·
Construction General Contractor · Construction Trades (Plumber, Electrician, HVAC, etc.) ·
Design, Architecture, or Engineering · Financial Services other than Accounting or Bookkeeping ·
Hair Salon, Beauty Salon, or Barber Shop · Information Technology (Computers, Software) ·
Insurance Agency or Broker · Lawn Care or Landscaping · Legal Services · Lodging (Hotel, Motel) ·
Manufacturer Representative or Agent · Manufacturing · Medical, Dental, or Health Service ·
Non-Profit · Professional Consulting · Property Management or Home Association · Real Estate
Brokerage or Developer · Rental · Repair and Maintenance · Restaurant, Caterer, or Bar · Retail
Shop or Online Commerce · Sales: Independent Agent · Transportation, Trucking, or Delivery ·
Wholesale Distribution and Sales · General Product-based Business · General Service-based
Business · Other/None

Examples of each industry's **core income and cost accounts**:

| Industry | Core income / COGS accounts |
|---|---|
| General Service-based | Sales |
| General Product-based | Sales, Shipping and Delivery Income, Freight and Shipping Costs, Merchant Account Fees, Product Samples Expense, Purchases – Resale Items |
| Construction General Contractor | Construction Income, Construction Materials Costs, Subcontractors Expense, Equipment Rental for Jobs, Tools and Small Equipment, Bond Expense, Blueprints and Reproduction, Worker's Compensation Insurance, Other Construction Costs |
| Restaurant, Caterer, or Bar | Food Sales, Food Purchases, Restaurant Supplies, Merchant Account Fees (recommended adds Bar Sales, Catering Sales, Bar Purchases, Cash in Drawer, Employee Tips Payable…) |
| Retail Shop or Online Commerce | Merchandise Sales, Sales Discounts, Merchant Account Fees |
| Transportation, Trucking, or Delivery | Gross Trucking Income, Fuel Surcharge, Fuel for Hired Vehicles, Truck Maintenance Costs, Travel Expenses for Drivers |
| Church or Religious Organization | Tithes/Offerings, Pledges, Building Fund, Mission Offerings, Benevolence Offerings, Youth Group, … |

Accounts that appear in nearly every industry (in 28 or more of the 33): Furniture and Equipment,
Accumulated Depreciation, Advertising and Promotion, Bank Service Charges, Computer and Internet
Expenses, Insurance Expense (with subaccounts), Meals and Entertainment, Office Supplies,
Professional Fees, Rent Expense, Repairs and Maintenance, Telephone Expense, Travel Expense,
Utilities, Interest Income/Expense, and **Ask My Accountant** (QuickBooks' catch-all for
uncategorized amounts).

---

## What LocalBooks will do

The same three screens, with the same choices and defaults.

**Screen 1 — Company information**
- Company name, legal name, address, Federal ID → the existing `company_*` preferences.
- Income tax form: the 8 choices above → `tax_form`. Single-member LLC maps to `SCHEDULE_C`,
  multi-member LLC to `1065`, nonprofit to a new `990`, and Other/None leaves it unset.
- First month of the fiscal year → `fiscal_year_start`.
- Where to save the file (the existing save dialog).
- QuickBooks also asks for the company start date and the first month of the tax year.
  LocalBooks has no setting for either yet, so these are left out.

**Screen 2 — Industry and chart of accounts**
- An Industry menu with the 33 industries.
- An account list grouped by type, with the industry's **core** accounts checked and its
  **recommended** accounts unchecked.
- The tax form's equity accounts are added and checked automatically, and Retained Earnings is
  renamed to match the tax form.
- **Default** restores the suggested set.
- **Skip** keeps today's 35-account chart, for people who'd rather set up their own (or are
  importing from QuickBooks).

**Screen 3 — Expert settings** (optional, filled in from the industry's defaults)
- *Customers are charged sales tax* → `sales_tax_enabled`, plus an optional first rate.
- *Inventory and purchase orders are used* → creates Inventory Asset.
- QuickBooks' other expert settings (estimates, time tracking, report basis) don't apply:
  estimates and time tracking are always available in LocalBooks, and there's no saved
  report-basis setting, so these are left out.

**What LocalBooks adds beyond QuickBooks**
- The 5 system accounts that exist in every LocalBooks company (A/R, A/P, Undeposited Funds,
  Opening Balance Equity, Retained Earnings). They are numbered in QuickBooks' 5-digit
  style. Before building, I'll check the exact numbers against the sample company files that
  ship in the installer.
- A **Checking** account, checked by default, since LocalBooks companies have one today. Its
  name can be changed on screen 1.
- Inventory Asset and Sales Tax Payable are created on first use, the same way QuickBooks
  does it. This needs two small changes: `inventory.rs` `track_product` and `sales_tax.rs`
  `liability_account` currently fail when the account is missing.

### How it works

- **Data:** a catalog file in the repo (`src-tauri/src/company_setup/catalog.json` or a Rust
  table) with the 405 accounts, the 33 industry → account lists with core/recommended flags,
  the 8 tax-form equity sets, and each industry's default expert settings. It's generated once
  from the `.adr` files by a script that stays outside the repo. See open question 1 for what
  gets copied.
- **Backend:** `company_setup.rs` has `starter_chart(industry, tax_form)`, which returns the
  checklist for screen 2. `create_company_file` gains an optional `setup` argument (answers
  plus the chosen account keys). In `Db::create`, one transaction removes the 0002/0018
  starter accounts from the brand-new file, inserts the chosen accounts, renumbers and renames
  the system accounts, and sets the preferences. If it fails, the file is removed, as today.
  With no `setup`, nothing changes from today. The migrations stay as they are, so existing
  company files are unaffected.
- **UI:** `NewCompanyDialog` becomes a three-tab assistant with Back and Next.
- **Tests:** For every one of the 33 industries × 8 tax forms, the result is a valid chart
  (numbers unique, every system account present, subaccounts the same type as their parent).
  Also a `db` test for creating with and without setup, and a WebdriverIO e2e test of the
  assistant.
- **Later (not in this change):** the starter items, classes, job types and terms that some
  industries include (Construction has 38 items, for example), and the Windows-only
  EasyStep questions.

---

## Open questions

1. **How much of Intuit's data to copy.** Account names, numbers, types and the core and
   recommended flags work like an industry-standard list, so copying them is a reasonable
   call. The account descriptions are Intuit's own wording. My recommendation is to copy the
   names, numbers, types and flags, and write our own short descriptions. Alternatively, we
   copy everything, or use only the industry list and build our own charts as in the first
   draft.
2. **Account numbers.** Use QuickBooks' 5-digit numbers for set-up companies, which is familiar
   to people moving from QuickBooks? Skipped companies would keep today's 4-digit numbers.
3. **Nonprofits.** Add `990` to `TAX_FORMS`, with the tax calendar's due date on the 15th day
   of the 5th month?
