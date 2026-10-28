# Set up your company

LocalBooks keeps your books in a single company file on your computer. These steps set up the
details that appear on invoices and reports and decide how your fiscal year works.

## Create a new company

**File › New Company…** opens a short setup assistant, modeled on QuickBooks' own:

1. **Company information.** Enter the company name, and if you like the legal name, Federal ID
   (EIN) and address. Choose the **Income tax form used** (sole proprietorship, single- or
   multi-member LLC, partnership, corporation, S corporation, nonprofit, or other) and the
   **First month in fiscal year**.
2. **Industry and accounts.** Choose the industry closest to yours from the same 33 industries
   QuickBooks offers. LocalBooks lists the accounts it suggests for that industry, numbered
   QuickBooks-style (10100 Checking, 11000 Accounts Receivable, 60000 Advertising and Promotion…).
   The ones most businesses like yours need are already checked; check any others you want and
   uncheck the rest. **Default** puts back the suggested set. The equity accounts follow your tax
   form, for example Owners Draw and Owners Equity for a sole proprietor, or Capital Stock and
   Shareholder Distributions for an S corporation.
3. **Expert settings.** Choose whether customers are charged sales tax and whether you track
   inventory. Each adds the accounts it needs.

Click **Save As…** to choose where to save the company file; it opens straight away. If you'd
rather start from LocalBooks' standard chart of accounts and set things up yourself, click
**Skip — use the standard chart** instead. Either way, you can add, rename or remove accounts
later in the Chart of Accounts.

## Enter your company details

1. In the left sidebar, open **Settings › Preferences Console**. (Click **Settings** to show or
   hide its pages: one for each Settings tab.)
2. Under **Company**, fill in **Company name**, **Legal name**, **Address**, **Email**, **Phone**
   and **EIN**. Each field saves as soon as you click away from it; a green **✓ Saved** confirms it.
3. Click **Upload logo…** to add your logo. It prints on invoices, estimates and reports.

## Choose your accounting settings

1. Under **Accounting & tax**, set **First month of fiscal year**. Reports such as "This fiscal
   year" and the Balance Sheet's Retained Earnings follow this.
2. Set **Currency symbol** (for example `$` or `€`) and **Home currency** (for example `USD`).
   The home currency locks once you record a foreign-currency transaction.
3. Choose your **Tax form** (Schedule C, 1065, 1120-S, 1120 or 990). The **Tax calendar** below then
   lists your federal due dates.

> **Tip:** The tax calendar is a reminder, not tax advice. Weekends move to Monday, but holidays,
> extensions and state deadlines are not included.

## Design your forms and set up email

1. Open **Settings › Custom Forms** to choose how invoices and estimates look: title, font, colors,
   logo placement, columns, and the message and terms printed on them. See
   [Design your invoices and estimates](custom-forms.md).
2. Emailed forms open in your own mail app with the PDF attached. To send them from LocalBooks
   instead, enter your email provider's SMTP settings in **Settings › Email**.

## Check your chart of accounts

A company set up with the assistant starts with the accounts you chose for your industry. One
created with **Skip** starts with LocalBooks' standard chart of accounts (bank, receivables,
payables, equity, income and common expenses). You choose these accounts on every form, so you can
start recording straight away. Accounts a feature needs, such as Sales Tax Payable or Inventory
Asset, are added the first time you use that feature.

## Make sure backups are on

1. Under **Automatic backups**, keep **Daily backup** turned on. A backup is taken when LocalBooks
   starts, at most once a day.
2. Set **Keep** to how many daily backups to keep (1–60).

## Sort a list

Click a column heading in a list (invoices, bills, estimates, checks, customers, products, the
journal and others) to sort by it; click it again to reverse the order. An arrow shows which column
is sorted.

Lists of transactions start with the newest at the top. LocalBooks remembers the order you choose
for each list on this computer; click the sorted heading a third time to go back to newest first.

## Work in more than one window

LocalBooks handles windows the way QuickBooks Desktop for Windows does. Forms, registers and reports
open as windows *inside* the LocalBooks window, so you can keep a report beside the invoice you're
entering, all in one program window. The menus use QuickBooks' names.

### Windows inside LocalBooks

- Choosing a page in the sidebar or the menu bar opens it in a window, or brings forward the window
  that already shows it. **Edit…** on an invoice, bill, check or expense, or a report chosen in the
  Reports list, opens it in a window of its own. Opening the same one again brings its window forward.
- Each window has a title bar with **Minimize**, **Maximize** (or **Restore Down**) and **Close**.
  Drag the title bar to move it; drag an edge or corner to resize it; double-click the title bar to
  maximize or restore it.
- A **maximized** window fills the work area. Its buttons move up to the right end of the toolbar,
  where QuickBooks puts them on the menu bar, and a new window opens maximized too.
- **Minimized** windows wait as small title bars along the bottom of the work area. Click one to bring
  it back.
- Every window shows the same books: when you save a change in one, the others update straight away.
  If a window has a form with entries you haven't saved, LocalBooks asks before it closes.

| Keys | What they do |
|---|---|
| **Ctrl+F6** or **Ctrl+Tab** | Go to the next window (add **Shift** for the one before) |
| **Esc** or **Ctrl+F4** | Close the active window (⌘W on a Mac) |

### One Window or Multiple Windows

As in QuickBooks, choose **View › One Window** or **View › Multiple Windows** (also in **Edit ›
Preferences › Desktop View**, under **View**):

- **Multiple Windows** (the default, as in QuickBooks): windows float in the work area, side by side
  or overlapping.
- **One Window**: one window shows at a time, filling the work area; the others wait behind it.
  Switch with the **Window** menu or the **Open Window List**.

The choice is kept with each company.

### The Window menu and the Open Window List

- **Window › Close All** closes every window (each asks first if it has unsaved entries).
  **Cascade**, **Tile Vertically** and **Tile Horizontally** arrange the open windows in the work area;
  **Arrange Icons** lines the minimized ones up along the bottom.
- Below those, the **Window** menu lists the open windows, numbered, the active one ticked. Choose one
  to bring it forward.
- **View › Open Window List** shows the open windows in a pane beside the sidebar, the active one
  highlighted (minimized ones in italics). Click a window to bring it forward. Choose the menu item
  again (or **✕** on the pane) to hide it.

### A second LocalBooks window

**Window › New Window** (**Ctrl+Shift+N**, ⇧⌘N on a Mac) opens a second LocalBooks window on the same
company, for example on a second screen. It starts on the page you're looking at and has windows of
its own inside it.

### Reopen your windows with the company (saved desktop)

When you open a company, LocalBooks reopens the windows you had inside it: their screens, forms,
registers and reports, where they were, which were minimized or maximized, and where the LocalBooks
window itself was. Choose how in **Edit › Preferences › Desktop View**, under **Desktop**, the same
choices as QuickBooks:

| Choice | What reopens |
|---|---|
| **Save when closing company** (the default) | The windows open when you close the company, quit, or open another company |
| **Save current desktop** | Saves the windows open now; they reopen every time, whatever is open when you close |
| **Don't save the desktop** | Nothing: the company opens on the Home page |
| **Keep previously saved desktop** | Shown after you save a desktop: keeps it as it is |

On a Mac, **Window › Workspace** has the same choices in QuickBooks for Mac's words: **AutoSave**
(save when closing), **Manual** (keep the saved workspace) and **Save Workspace**.

The desktop is kept for each company file and, when the file has users, for each user, on this
computer. If a saved screen no longer exists, or a transaction in it was deleted, that window is
left out and the rest still open.

To open a company once without its windows, hold **Alt** (**Option** on a Mac) while you click
**Open**, or choose **File › Open Company Without Restoring Windows…**. The company opens with no
windows; your saved desktop is kept for next time unless you then close the company with **Save when
closing company** chosen.

### Choose the left pane and where you start

Also in **Edit › Preferences › Desktop View**, for this computer:

| Setting | Choices |
|---|---|
| **Left pane** | **Full menu** (the default): every page, grouped as Sales, Expenses, Banking and so on. **Open windows only**: just the windows open now, like QuickBooks' Open Window List in the left icon bar; open pages from the menu bar or the search box, and click **Menu** at the top of the pane to bring the full menu back. **Hidden**: no left pane, for the most room. **View › Show or Hide Left Icon Bar** switches between the full menu and hidden. |
| **Start on** | **Last open windows** (the default): the desktop chosen under **Desktop**, or the Home page when none is saved. **Home page**: the Home page alone. **This screen**: one screen you pick, such as Invoices or a register. **No windows**: an empty desktop; choose a page to begin. |

**Start on** takes effect the next time a company opens. Holding **Alt** (**Option** on a Mac) while
clicking **Open** still opens with no windows, whatever you chose.

## Find things in the menu bar

The menu bar is laid out like QuickBooks Desktop's, so the commands you know are where you expect
them. Each one opens the screen, and often the form, in a window inside LocalBooks (or brings forward the window that already shows it):

| Menu | What's in it |
|---|---|
| **File** | New, open, close and back up company files; **Utilities › Import / Convert…** (the Desktop Migration Wizard) and Condense Data; print and send forms |
| **Edit** | Undo, cut, copy and paste; Use Register; **Find…** (Find Transactions); Preferences |
| **View** | Show or hide the left icon bar (the left pane); Open Window List; One Window or Multiple Windows |
| **Lists** | Chart of Accounts, Item List, Sales Tax Codes, Classes, customers and vendors, sales reps and Ship Via, form templates, memorized (recurring) transactions |
| **Company** | Home Page, users and passwords, closing date, bank feeds, budgets, general journal entries, reclassify, currency adjustment |
| **Customers** | Customer Center, estimates, invoices, sales receipts, statements, finance charges, receive payments, credit memos and refunds, Change Item Prices |
| **Vendors** | Vendor Center, enter and pay bills, vendor credits, sales tax, purchase orders, 1099s |
| **Employees** | Weekly timesheet and single time activity |
| **Banking** | Write checks, credit card charges, registers, deposits, transfers, reconcile, bank feeds |
| **Reports** | Report Center and each report, grouped as in QuickBooks: Company & Financial, Customers & Receivables, Sales, Jobs, Vendors & Payables, Purchases, Inventory, Banking, Accountant & Taxes, Budgets |

The classic shortcuts work from anywhere:

| Shortcut | Opens |
|---|---|
| **Ctrl+I** (⌘I) | Create Invoices |
| **Ctrl+W** | Write Checks (Windows and Linux; on a Mac ⌘W closes the window) |
| **Ctrl+R** (⌘R) | Use Register |
| **Ctrl+J** (⌘J) | Customer Center |
| **Ctrl+T** (⌘T) | Memorized Transaction List (Recurring Transactions) |
| **Ctrl+F** (⌘F) | Find Transactions |

**Ctrl+A** stays Select All, so open the Chart of Accounts from **Lists**. On Windows, press **Alt**
to reach the menus from the keyboard (for example **Alt+U** for Customers).

Next: [Create and send an invoice](invoices.md) · [Back up, restore and open company files](backups-and-company-files.md)
