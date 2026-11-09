# Changelog

What changed in each LocalBooks release. A release's section here becomes its release notes on
GitHub and in the app's update window (see `.github/workflows/release.yml`), so write it for the
people using LocalBooks.

## [Unreleased]

- **Notes and Emails on the information panel.** A customer's or vendor's notes can be edited from
  its panel, with **Date Stamp** to start a dated line, and a customer's **Emails** tab lists every
  invoice, estimate, sales receipt and statement emailed to them.
- **An information panel for customers, vendors and items.** Click a name in the Customers or
  Vendors list, or an item in Products & Services, to see its summary beside the list: open
  balance, active estimates and unbilled costs, open purchase orders, phone, email, notes and
  recent transactions, or for an item on hand, on order, average cost and value. Invoices, bills,
  checks and expenses show it for the chosen name. It can be hidden, and its figures match the
  reports.
- **Service dates, a template on the form, and printed credit memos.** Invoice and sales receipt
  lines take a Service Date that prints in its own column; the Template box on a sales form picks
  the form style it prints with; and credit memos and refund receipts now print as a form of their
  own, with a Credit memo layout in Settings › Custom Forms.
- **Email reports as PDF, and schedule them.** Every report, list and register (and the Reminders
  list) can be emailed or saved as a PDF from its Export menu. Reports › Scheduled Reports… sends
  chosen reports every day, week, month or quarter by email, or saves them to a folder as PDF or
  Excel, while LocalBooks is open; a run missed while it was closed happens when it next opens.
- **Statement charges.** Enter statement charges… on a customer's row keeps a register of charges
  billed on the customer's next statement instead of an invoice. Each posts to Accounts
  Receivable, shows in the aging, and is paid in Receive Payments like an invoice.
- **Import customers, vendors or invoices from your own spreadsheet.** In the Desktop Migration
  Wizard, choose a .csv or Excel file (or download a template), match its columns to LocalBooks'
  fields, and check it: every row that would fail is listed with its row number before anything is
  staged. Then review and post it like any other import; invoices become real invoices, owed by
  their customers.

- **Lists in your own order.** Drag rows in the chart of accounts, the item list and the customer
  list into the order you want; it's kept in the company file. Each can be shown as a tree (with
  Expand all and Collapse all) or flat with full names.
- **Print envelopes.** Print Envelope… on an invoice or sales receipt, on a form's row, and on a
  customer's or vendor's row prints an envelope with your return address and theirs. Set the
  envelope's size, font and position in Settings › Custom Forms › Envelope.
- **Early-payment discounts on invoices in another currency.** Receive Payments offers and takes
  the discount on a foreign-currency invoice in its own currency. The discount leaves Accounts
  Receivable at the invoice's exchange rate, so A/R clears exactly, and only the money received
  carries an exchange gain or loss.
- **Early-payment discounts on part-paid invoices, and bounced discounted payments.** Receive
  Payments offers an invoice's early-payment discount on what is left of a part-paid invoice, and a
  payment that took a discount can be recorded as a bounced check: the discount is reversed and the
  invoice opens again for the whole amount.
- **Print Pay Bills' checks later.** Pay Bills has **Print later**, as Write Checks does: the payments
  post now and their checks, one per vendor, wait in **File › Print Forms › Checks…**, where they
  take their numbers when they print.
- **Bounced sales receipts and foreign-currency payments.** Record Bounced Check now lists sales
  receipts, which it charges back to the customer on a new invoice, and payments in another
  currency, which bounce at the rates they were recorded at (the exchange gain or loss they
  realized is reversed).
- **Save every attachment at once.** Settings › File & Data › Save All Attachments… saves every
  file attached to a transaction in one zip file, a folder per transaction; a transaction with
  several files has **Save all…** too.
- **P.O. No. on statements.** Printed customer statements can show a P.O. No. column with the
  customer's purchase order number on each invoice and credit (Settings › Custom Forms ›
  Statement).
- **Company-file import clears payments moved out of Undeposited Funds.** A full import left
  payments waiting to be deposited when the file's deposit moved them into an account that isn't a
  bank (such as receipts written off to an expense). It now posts that deposit as a transfer out of
  Undeposited Funds that clears those payments; balances still match the file.
- **Print a register, and transfers from its entry row.** A register has **Print…** (also
  **File › Print…**, Ctrl+P), which prints its transactions for the dates chosen with the balance
  forward and ending balance. In the entry row at the bottom, choosing another bank account records
  a transfer, and choosing Undeposited Funds on the deposit side deposits the payments waiting
  there.
- **Save vouchers as PDF and email them.** Saved bills, checks, expenses, credits, deposits and
  journal entries have **Print ▾ › Save as PDF…**, and **Email** sends the voucher's PDF to the
  vendor or customer named on it, through your mail app or LocalBooks' email server, as invoices
  are sent.
- **A report center that's easier to search.** Type to find a report by name or by what it shows,
  star reports into Favorites, see your Recent reports, read a line about each report, and choose
  the dates reports open on before you run them.
- **Renaming rules for downloaded payees.** When a downloaded description begins with, contains or
  ends with some text, a renaming rule names its payee. LocalBooks offers a rule when you change a
  payee, and **Rules…** in Bank Feeds lists them to search, edit and remove, along with the category
  rules learned from your approvals.
- **Downloaded bank transactions match what you already entered.** A downloaded line that agrees
  with a check, deposit, bill payment or transfer already in the books (amount, date, check
  number) is matched to it on import instead of waiting to be added twice. Lines entered later
  show a suggested match; **Find a match…** matches by hand, **Show matched** lists matched lines
  with **Unmatch**, and ticked lines can be added several at once.
- **Arithmetic in amount and quantity boxes.** Type `+`, `-`, `*` or `/` in any amount, rate or
  quantity box and a small tape shows each number and the running total; Enter puts the answer in
  the box (`100+25.50*2` gives 151.00). Amounts round to the cent and quantities to four decimals.
- **Larger or smaller text.** A↑ and A↓ on the toolbar, View › Larger Text and Smaller Text, or
  Cmd/Ctrl + and − change the text size of lists, registers, forms or reports, each kind on its
  own, and LocalBooks remembers it. An account register can also be given a color.
- **Sales lines in the familiar order.** On invoices, sales receipts, estimates and credit memos
  the line columns now run Item, Description, Qty, Rate, Class, then the income account (which the
  item fills in), Amount and Tax.
- **Receive Payments shows its totals.** Under the open invoices, a Totals row and a summary of
  the amount due, applied, discounts applied and what stays open; an amount received that doesn't
  match what's applied shows as unapplied before you save.
- **Ending balance and discount date on forms.** Write Checks, card charges and credits, Make
  Deposits and Pay Bills show the chosen account's ending balance beside the account box, and
  Enter Bills shows a **Discount date** next to the due date when the terms offer an early-payment
  discount.
- **The menu bar reaches more screens.** Lists › Currency List, Company › Email Setup, AI Access
  (MCP) and Audit Log, Customers › Invoice for Time & Expenses, Reports › Memorized Reports ›
  Memorized Report List, and Reports › Transaction History / Transaction Journal for the
  transaction open on a form.
- **Transaction History and Journal on forms.** A saved invoice, bill, check, expense, journal
  entry, sales receipt, credit, deposit or transfer has **History** and **Journal** on its toolbar,
  with **Ctrl+H** and **Ctrl+Y**, as registers already did; in the journal they act on the entry
  you clicked.
- **Duplicate a transaction.** On a saved invoice, bill, check, expense, card credit or journal
  entry, **Duplicate** on the toolbar (**Ctrl+O**, or **Edit › Duplicate Transaction**), or on its
  row in the list, starts a new unsaved copy dated today: an invoice or journal entry gets the next
  number when saved, a check the next check number, and a bill keeps its terms with its number left
  blank. On Windows and Linux, Ctrl+O now duplicates and **File › Open or Restore Company…** moves to
  **Ctrl+Shift+O**; on a Mac ⌘O still opens a company and Control+O duplicates.
- **More desktop shortcuts.** On forms, **Ctrl+L** opens the list of the box you're in, and
  **Ctrl+Alt+Y** / **Ctrl+Alt+V** copy a line and paste it above the line you're on (also Edit ›
  Copy Line and Paste Line). In a register, **Ctrl+E** opens the selected transaction to edit and
  **Ctrl+G** goes to the register of its other account; on a saved transfer, **Go To Transfer**
  (Ctrl+G) does the same. Page Up / Page Down, Ctrl+N, Ctrl+P and Ctrl+D now also work on saved
  sales receipts, estimates, credits, purchase orders, deposits and transfers.
- **Deposits have a Received From name.** Each **Add funds** line of a bank deposit can name the
  customer or vendor the money came from, and a name typed in a register's deposit row is matched
  to your customers and vendors instead of only being kept in the memo. The register shows the
  name as the payee, and the deposit is listed in that name's transactions and reports by name.
- **Keep the statement with a reconciliation.** Attach the bank or card statement (a PDF or a
  picture) to a reconciliation in progress or a finished one; save a copy from Reconcile's history.
- **Undo Last Reconciliation.** In Banking › Reconcile, the newest completed reconciliation can be
  undone: its transactions are uncleared so the statement can be reconciled again. The
  reconciliation stays on record and the undo, with an optional reason, goes into the Audit Log.
- **Service charge and interest earned when you reconcile.** Starting a reconciliation now takes
  the statement's service charge and interest earned (amount, date, account and class). Each posts
  as a journal entry and is already ticked as cleared.
- **Cash back on deposits.** A bank deposit can keep part of the money as cash: choose the account
  it goes to (such as Petty Cash), a memo and the amount, and the bank gets the rest. The deposit's
  printed voucher shows it.
- **A warning before paying around open bills or invoices.** Writing a check or entering an
  expense to a vendor with unpaid bills now asks first, since it doesn't pay those bills (Pay Bills
  does). A sales receipt from a customer with open invoices asks too, since Receive Payments is
  what pays an invoice. **Save anyway** goes ahead; each warning can be turned off in Preferences ›
  Bills and Preferences › Sales & Customers.
- **A voided payment comes off the customer's statement.** Voiding a customer payment left it on
  the statement with nothing taking it back, so the statement's balance was short by the payment
  (A/R, aging and the invoice were right). The void now shows as its own **Void** line under the
  customer. Voids of any transaction now carry their customer's or vendor's name in registers and
  in reports by name, so a voided bill nets to zero under its vendor instead of leaving a line
  under "No name".
- **Open saved sales receipts, estimates, credits, purchase orders and deposits.** Double-click one
  in its list (or choose **Open**), or press **Previous** on its form, to see it where the form
  goes, shown as it prints. **Previous** and **Next** step through the others, and its toolbar has
  **Void**, **Print**, **Email** and **Attach File**, plus **Apply** and **Refund** on credits,
  **Create Invoice** on an estimate and **Create Bill** or **Close PO** on a purchase order.
  Credit memos, refund receipts, vendor credits, deposits and transfers now print, as a voucher of
  the accounts they posted to.
- **Previous, Next, Print and Attach on bills, checks and journal entries.** The form toolbar's
  **Previous** and **Next** now step through saved bills, checks, expenses, card credits and
  journal entries (by date, then number), as they already did on invoices. **Print** prints the
  saved check, or a voucher of a bill, expense or journal entry showing the accounts it posted to,
  and **Attach File** shows and adds files on a saved one. A blank new bill no longer counts as
  having unsaved changes, and closing a journal entry with entries not saved now asks first.
- **A Home page that maps the work.** The Dashboard now opens on **Home**: estimates, invoices,
  payments and deposits, and purchase orders, bills, bill payments and checks, drawn in the order
  the work goes, each one click away, with a red count where work is waiting (overdue invoices,
  bills due, payments to deposit, to-dos). Banking and company shortcuts and your bank, card,
  receivable and payable balances sit beside it. The totals, chart, trial balance and Reminders
  moved to the **Insights** tab.
- **Duplicate customers, vendors and accounts.** **Duplicate…** on a row of the Customers, Vendors
  or Chart of Accounts list makes a new record filled in from that one, to save under a new name.
  A customer or vendor copy leaves out its account and resale numbers, tax ID and history.
- **Find in a report.** Every report has a **Find in report** box beside Export: matching cells are
  shaded, Enter steps through them, and an amount matches however it is written (`1250` finds
  `1,250.00`).
- **Invoices offer the customer's credits.** When a customer has credit memos with credit left, the
  invoice shows the available credits under Balance due, and saving it asks whether to apply them
  now, oldest first. Before, a credit could only be applied from the credit memo.
- **Moving from QuickBooks: deposits split between accounts.** A deposit that went partly into one
  bank account and partly into another used to come in as a journal entry, which left its payments
  listed in Make Deposits as still waiting to be deposited. It now comes in as a deposit into the
  account that got the most, with the rest shown as taken out of it, and its payments are marked
  deposited. The books were right either way.

## [0.13.0]

- **Every report is in the Reports menu.** The detail reports (P&L Detail, Balance Sheet Detail,
  A/R and A/P Aging Detail, Open Invoices, Unpaid Bills, customer and vendor balances, purchases,
  check and deposit detail, the journal, adjusting entries and more) and the list reports (phone and
  contact lists, Item and Account Listings) now have menu entries, not only the Report Center. A
  **List** submenu gathers the list reports, and **Memorized Reports** opens Process Multiple
  Reports.
- **Lists › Vehicle List, Vendors › Sales Tax › Pay Sales Tax… and Help › Check for Updates…** open
  those screens directly.
- **More form shortcuts.** On an open form, **Ctrl+N** starts a new one, **Ctrl+P** prints it, and
  **Page Up** / **Page Down** open the previous and next, as the toolbar buttons do.

- **Receive Payments takes early-payment discounts.** An invoice on terms such as *2% 10 Net 30*
  that is paid in full by the discount's last day can now take its discount right on Receive
  Payments, in a new **Discount** column you can change or clear. Choose the account it goes to
  (an income account such as Sales Discounts) in **Preferences › Payments**, and tick **Take
  customers' early-payment discounts automatically** to have it filled in. The payment and the
  discount post together and close the invoice; voiding the payment reverses both. This matches
  what Pay Bills already does for vendor discounts.
- **A customer's or vendor's transactions add up correctly.** The panel below the Customers and
  Vendors lists used to add invoices and payments into one total, which meant nothing. Choose one
  type, such as Invoices, to see its total; with All types the panel counts the transactions.
- **The "Saved customer" note goes away** when you pick or open another customer or vendor, instead
  of staying above the list.
- **A damaged company file no longer closes LocalBooks while unlocking.** If the part of the file
  that holds one password's copy of the encryption key is damaged, LocalBooks now says so instead of
  quitting, and other users' passwords and the recovery key still open the file.

## [0.12.0]

- **Bill To and Ship To on sales forms.** Invoices, sales receipts (Sold To), estimates and credit
  memos now have Bill To and Ship To boxes, filled from the customer and editable on each form, plus
  **P.O. No.** and **FOB**. Each document keeps its own copy of the addresses, so changing a
  customer's address later leaves saved documents as they were. They print and email with the
  form; the form designer can turn Ship To and P.O. No. off.
- **A fuller customer record.** Customers now have a shipping address next to the billing
  address, a contact and an alternate contact, alternate phone, fax, website, account number,
  credit limit and resale number. A job without its own addresses uses its customer's.
- **Credit limit warning.** When an invoice would take a customer over their credit limit,
  LocalBooks shows what they owe now and would owe, and asks before saving. It never blocks the
  sale.
- **Row menus stay on screen.** A row's ▾ actions menu near the bottom of the window now opens
  upward instead of running off the edge.
- **Subtotals, discounts, other charges and groups on sales forms.** On invoices, sales receipts
  and estimates, choose **Subtotal** in the Product/Service list to add up the lines above it. A
  new item type, **Discount**, takes an amount or a percentage off the line above (an item, a group
  or a subtotal) and can lower the taxable amount. **Other charge** items add shipping, handling or
  a percentage fee. **Groups** (Products & Services › Groups…) fill in several items at once and can
  print as a single line. They print in place, come back when you edit, and carry from an estimate
  to its invoice; discount items show in Sales by Product/Service.
- **Edit an invoice or bill that's already part paid.** An invoice with payments or credit memos
  applied, or a bill with payments or vendor credits applied, now opens for editing instead of
  asking you to void the payments first. The form tells you how much is applied, and saving keeps
  it all applied to the corrected version, so the open balance is the new total less what was
  paid. Deposits and reconciliations aren't affected. The edit is refused, with a message saying
  why, if the new total would be less than what's applied or the customer or vendor changes.
- **Defaults for selling, getting paid and paying bills.** New Company Preferences in Sales &
  Customers, Jobs & Estimates, Payments, Bills and Send Forms:
  - Receive Payments applies an amount to the invoice with exactly that balance, or else the
    oldest invoices first. You can choose oldest first only, or tick invoices yourself, and ticking
    invoices now works out the amount received. A tick column is new on the form.
  - Payments and sales receipts go to Undeposited Funds unless you turn that off. **Sales receipts
    used to start on your first bank account; they now start on Undeposited Funds.**
  - A new bill is due after the vendor's terms. For a vendor without terms, it is due 10 days after
    the bill date, or the days you set. **Bills used to be due in 30 days whatever the vendor's
    terms.**
  - Pay Bills can use each vendor's open credits on the bills you tick, oldest credit first, and
    take the early-payment discount a bill's terms offer when paid in time, then pay only the rest.
  - Your usual Ship Via fills in on new invoices, sales receipts and estimates.
  - A warning before selling to a customer with overdue invoices (off until you turn it on).
  - Turn off estimates or price levels if you don't use them, rename the five job statuses, and
    give jobs a status on the customer's Edit….
  - Each customer has a **Preferred delivery** (Email, Print or None) that ticks Email later or
    Print later on their new forms; new customers take the Send Forms default.
  - Change Item Prices starts from the default markup when you price from cost.

- **No more "database is locked" when several things save at once.** On a slow disk, a save
  that had to wait for another one could give up after five seconds; it now waits its turn.
- **Report preferences.** Preferences › Reports & Graphs now sets whether Profit and Loss opens on
  the accrual or cash basis, whether A/R and A/P Aging count from the due date or the transaction
  date, and whether items show by name, description or both. Accounting › Company Preferences can
  hide account numbers in reports or show only a subaccount's own name.
- **Choose where accounts go on the Statement of Cash Flows.** Preferences › Reports & Graphs lists
  each balance sheet account with its section (operating, investing or financing); move one and the
  statement follows.
- **See a report's options first.** Turn on **Show the report's options before running it**
  (Reports & Graphs › My Preferences) and a report asks for its dates before it runs.
- **Preferences look the way you expect.** Settings › Preferences (Edit › Preferences) now lists
  the familiar categories on the left: Accounting, Bills, Checking, Desktop View, General, Sales &
  Customers and the rest, each with **My Preferences** and **Company Preferences** tabs. Settings
  that live on their own screen, like sales tax rates or email, are linked from their category.
- **Warnings before saving, with Save anyway.** LocalBooks now asks before saving a check whose
  number was already used on that bank account, a bill whose number was already used for that
  vendor, a transaction dated more than 90 days ago or 30 days ahead, or a posting to Retained
  Earnings. Click **Save anyway** to record it as entered. Admins change the days, or turn each
  warning off, in the Checking, Bills and Accounting company preferences.
- **Type amounts without a decimal point.** Turn on **Automatically place a decimal point**
  (Preferences › General › My Preferences) and `1234` becomes 12.34; the box shows 12.34 as soon
  as you leave it.
- **More General preferences.** Beep when a transaction is recorded; warn before editing a posted
  transaction or voiding one (both on, and now each can be turned off); bring back messages you
  turned off with **Don't show this message again**; and choose whether a description or price you
  typed stays when you change the item on a sales line (ask, always or never).
- **Journal entry memos fill in.** A new journal entry line takes the first line's memo
  (Preferences › Accounting › My Preferences).
- **Two-digit years and decimal hours.** Company Preferences › General can show dates as 10/13/26
  and hours as 1.50 instead of 1:30.
- **Sales tax on the cash basis, and your filing period.** In Preferences › Sales Tax, choose
  whether you owe sales tax when you invoice or when you are paid, and whether you file monthly,
  quarterly or yearly. The Sales Tax report and Pay sales tax follow the basis (on cash, an invoice
  counts as it is paid), open on your filing period, and Reminders shows the next return. Printed
  invoices, sales receipts and estimates can mark taxable lines with a **T**.
- **Bill expenses to customers.** Tick **Bill?** on a check, expense or bill line for a customer,
  then invoice those costs from **Sales › Bill Costs**, with a markup. Preferences › Time &
  Expenses can tick Bill? for you, count reimbursements as income and set the default markup.
- **1099 boxes.** Map expense accounts to 1099-NEC and 1099-MISC boxes in Preferences › Tax: 1099;
  the 1099 Summary and Detail now show each box. Payments made from 2026 use the new $2,000
  threshold ($600 before). Don't file 1099s? Turn them off and the 1099 fields and reports go away.
- **Work week, billable time and finance charges.** Weekly timesheets can start on any day, new
  time can start billable, and finance charges can run from the invoice date instead of the due
  date.
- **Inventory switches.** Warn, stop or say nothing when a sale would take stock below zero, and
  turn inventory and purchase orders off if you don't use them (Preferences › Items & Inventory).
- **Checking preferences.** Put the vendor's account number (a new **Account no.** on the vendor)
  in check memos, re-date checks to the day they print, and choose the account Write Checks, Pay
  Bills and Make Deposits open with.
- **Recall a vendor's last bill or check.** Turn on **Automatically recall information**
  (Preferences › General › My Preferences) and a new bill or check for a vendor you have paid before
  starts with the same accounts, and amounts if you like.
- **Terms, Payment Method, types and customer messages lists.** Lists › Customer & Vendor Profile
  Lists now has a Terms list (with early-payment discounts such as *2% 10 Net 30*), a Payment
  Method list (*Visa*, *Check*…), Customer, Vendor and Job Types, and Customer Messages. Customers
  and vendors get a default term and a type. On invoices and bills the Terms box fills in from the
  customer or vendor and sets the due date; Receive Payments and Pay Bills show the discount
  offered and its last day. A customer message picked on an invoice, sales receipt or estimate
  prints on it. The Terms and Payment method boxes end in **＋ Add new…**.
- **Safer Excel imports.** A damaged or deliberately malformed `.xls` report file can no longer
  crash LocalBooks or make it run out of memory while importing (the Excel reader is updated).
- **Type checks and deposits straight into a register.** Bank and credit card registers end in a
  blank row: enter the date, number, payee, account, memo and an amount, then click **Record** (or
  press Enter). A bank payment becomes a check, a deposit a deposit, a card charge an expense and a
  card payment a card credit, just as their forms would make them. **Restore** clears the row and
  **Splits…** spreads it over several accounts.
- **Find and filter in a register.** **Go To…** finds a transaction by number, payee, memo or
  amount; **Dates** shows all, this month, this year or your own range; **2-line** puts the account
  and memo on a second line. The **No.** column now shows each check's, invoice's or bill's own
  number.
- **QuickReport, Transaction History and Transaction Journal.** Press **Ctrl+Q** in a register, or
  on an account, customer, vendor or item in its list, for a report of its transactions. Click a
  transaction and press **Ctrl+H** to see what is linked to it (an invoice's payments, a payment's
  deposit, a bill's payments) or **Ctrl+Y** for the debits and credits it posted.
- **Filter a customer's or vendor's transactions.** The transactions panel under the Customers and
  Vendors lists can show one transaction type and a date range.
- **Print checks and invoices later, in a batch.** Tick **Print later** on Write Checks or on an
  invoice and it waits in **File › Print Forms › Checks…** or **Invoices…**, marked *To print* in
  the list. Print the batch from there: checks take numbers in order from the first number on your
  check stock, and LocalBooks asks **Did they all print?** so only the ones that came out right
  keep their numbers. The forms remember whether you last left Print later on.
- **Record a bounced check.** **Banking › Record Bounced Check…** takes a deposited customer check
  the bank returned: the invoice it paid opens again, the bank goes down by the check and the
  bank's fee (posted to the expense account you choose), and a returned-check fee you charge the
  customer goes on a new invoice. Voiding the bounced check puts it all back.
- **Pay down a credit card.** **Banking › Pay Down Credit Card…** pays a card from a bank account,
  starting at what the card owes. It posts as a transfer.
- **Phone lists, contact lists and more reports.** A new **Lists** group in the report list has the
  **Customer** and **Vendor Phone Lists** and **Contact Lists**, the **Item Listing** and the
  **Account Listing**. Also new: **Balance Sheet Summary**, **Profit & Loss YTD Comparison**, **Net
  Income Detail**, **Sales by Rep Detail**, **Average Days to Pay Summary**, **Unpaid Bills by Job**,
  **Open Purchase Orders Detail**, **Time by Job Summary** and **Detail**, **Time by Name**, **Time by
  Item**, the **Physical Inventory Worksheet** (print it and write in the counts) and the **Audit
  Trail**, showing who posted, voided or changed what and when.
- **Collapse and expand report headings.** Click ▾ beside a heading to show only its total, or
  **Collapse all** to see just the totals. Reports print the way they're shown.
- **Process Multiple Reports.** Under **Memorized reports**, tick several reports and display them
  together, print them in one go (each on a new page), or save them as one Excel workbook with a
  worksheet per report.
- **Imports keep your classes.** Books moved from QuickBooks now arrive with their class list,
  subclasses included, and every transaction line keeps its class, so Profit & Loss by Class
  matches the original. This works for QuickBooks Desktop company files and backups (`.qbw`,
  `.qbb`), IIF files (the class list and the CLASS column), report exports with a Class column,
  and exporting from QuickBooks still installed on Windows. Classes that were hidden come in
  inactive. When an import brings classes, **Track classes on transactions** turns on, and the
  message after posting says so.
- **Save & Close, Save & New and Clear on every form.** Invoices, sales receipts, estimates, credit
  memos, bills, checks, expenses, card credits, vendor credits, purchase orders, deposits, transfers
  and journal entries now end in these three buttons. **Alt+S** saves and closes the form's window;
  **Alt+N** saves and leaves a blank form for the next one.
- **Date keys in every date box.** Press **t** for today, **+** or **-** to move a day, **w** / **k**
  for the first or last day of the week, **m** / **h** for the month and **y** / **r** for the year.
  Typing a date still works. In a check number box, **+** and **-** step the number.
- **Form shortcuts.** On an open form, **Ctrl+D** voids it (after you confirm), **Ctrl+M** memorizes
  it, and **Ctrl+Insert** / **Ctrl+Delete** insert or delete a line. They're also in the **Edit**
  menu, and the toolbar buttons show their keys when you hover over them.

## [0.11.0]

- **Six more reports.** **Profit & Loss by Month**, **Transaction List by Customer**,
  **Transaction List by Vendor**, **Expenses by Vendor by Month**, **Transaction Detail by Account**
  and **Voided Transactions** are in the report list. Like the other detail reports, each can be
  customized, saved, exported and printed, and a click on a line opens its transaction.
- **Add a new vendor right from a bill or check.** The Vendor box on bills, checks, expenses,
  purchase orders and vendor credits lists matching vendors as you type. When the name isn't on the
  list, choose **Add as a new vendor**, or **Add with details…** to fill in the address and terms
  too.
- **Add a new customer or item right from a sale.** The Customer box on invoices, sales receipts,
  estimates and credit memos works the same way, with **Add as a new customer**. The
  Product/Service column ends in **＋ Add new item…**, which adds the product or service and fills
  in the line.
- **See all of a vendor's or customer's transactions.** Click a name in the Vendors or Customers
  list and its bills, checks, invoices and payments show underneath, newest first, with a total.
  Double-click one to open it.
- **Open a transaction from a register.** Double-click a row in an account register to open the
  bill, check, expense or invoice itself; other entries open in the journal.
- **Double-click to edit an invoice, bill, check or expense.** Double-clicking a row in the
  Invoices, Bills or Expenses & Checks list opens it to edit, as **Edit…** in its menu does. When it
  can't be edited (it's voided, or a payment or credit is applied), LocalBooks says why.
- **Classes are easier to find and add.** With **Track classes on transactions** on (Settings ›
  Preferences › Class tracking), the Class column now shows on every form even before you've made
  your first class, and the class list ends in **＋ Add new class…** so you can add one while you
  enter a bill or invoice. The guide has a new **Track classes** section.
- **Sort registers and Reconcile.** Click a column heading in an account register or in Reconcile
  to sort by date, number, payee, amount or cleared status.
- **Pressing Enter moves between fields.** A new **General** section in **Settings › Preferences**
  lets Enter move to the next field, as Tab does. It also has: open name lists as you type, show
  clipped text in full on hover, and start new transactions on today's date or the last date you
  entered. In Preferences, Enter now saves the box and moves to the next one.
- **Recent companies in the order you opened them.** The No Company Open screen shows when you last
  opened each company file, newest first, instead of when the file last changed.
- **A toolbar on every transaction form.** Invoices, sales receipts, estimates, credit memos,
  bills, checks and expenses, vendor credits, purchase orders, deposits, transfers, journal
  entries and Receive Payment have a row of large buttons across the top: **Find**, **New**,
  **Save**, **Void**, **Print ▾**, **Email ▾**, **Attach File** and **Memorize**, showing only what
  each form can do. On an invoice, **Previous** and **Next** step through your invoices, and
  **Print ▾ › Post and print…** posts and prints in one step.
- **Payment method tiles.** Receive Payment and sales receipts show how you were paid as large
  tiles (**Cash**, **Check**, **Card**, **Bank / ACH**, and **More** for Wire and Other) instead of
  a drop-down. Receive Payment also shows the customer's balance at the top right and has an
  **Auto Apply** button.
- **The sidebar says where the company file is.** Its footer ends in **local**, **network** or
  **cloud** (a Dropbox, iCloud or other synced folder), instead of always saying offline.

## [0.10.3]

- **Windows inside LocalBooks, as in QuickBooks Desktop.** Invoices, bills, registers, reports and
  every other screen open as windows inside the one LocalBooks window. Move, resize, minimize and
  maximize them, or line them up with **Window › Cascade**, **Tile Vertically** and **Tile
  Horizontally**. **Window › Close All** closes them, asking first about anything not saved.
  **Ctrl+F6** moves to the next window and **Esc** closes the one in front.
- **One Window or Multiple Windows.** **View › One Window** shows one screen at a time, filling
  the space; **View › Multiple Windows** lets them float side by side. **View › Open Window List**
  lists the open windows beside the menu, and the Window menu lists them too.
- **Your windows come back.** The windows open when you close a company reopen with it, where they
  were. Choose what is saved in **Edit › Preferences › Desktop View** (Save when closing company,
  Save current desktop, Don't save the desktop), or on a Mac in **Window › Workspace**. To open a
  company once without them, hold Option (Alt on Windows) while clicking **Open**, or choose
  **File › Open Company Without Restoring Windows…**. When the file has users, each person's
  windows are their own: signing in reopens yours, not the last person's.
- **Choose the left pane and where you start.** In **Desktop View**, the left pane can show the
  full menu, only the open windows, or nothing. **Start on** opens a company on your last windows,
  the Home page, a screen you choose (say, Invoices), or no windows at all.
- **What's new guide.** **Help › What's new** lists the newest features release by release, in
  plain words, each with a link to its steps.
- **Attachments guide.** A step-by-step guide to attaching receipts and vendor invoices, saving a
  copy and removing files, and what happens to files when a transaction is edited or voided.
- **Links inside the in-app guides open the right section.** A link to part of another guide now
  opens that guide at the section, instead of showing as plain text.

## [0.10.2]

- **Early payments.** A customer payment can be dated before its invoice (a customer who paid
  early, or a deposit taken before you billed the work), and a bill can be paid before its date,
  as QuickBooks allows.
- **Check numbers and printing in Pay Bills.** Paying bills from a bank account writes one check
  per vendor, numbered from the account's next check number, and can print them. Write Checks
  continues the same numbering, so a number is never used twice.
- **Attach the vendor's invoice to a bill.** Attach files to a new bill, expense or check before
  you post it, or to any posted bill, expense, invoice, sale or deposit from its row menu. A 📎
  marks rows with files, and attached files now stay with a transaction when you edit it.
- **Click a report to see the details (QuickZoom).** Click an amount on the Profit and Loss or
  Balance Sheet to open the account's transactions for that column's dates. Click a name on an
  aging summary to list its open invoices or bills, or a line on Sales by Customer, Sales by
  Product/Service or Expenses by Vendor Summary to open its detail report.
- **Newest first.** Lists of transactions start with the newest at the top, and each list
  remembers the order you choose.
- **Clearer help when the books won't open.** If LocalBooks can't open your company file at
  launch, it now says in plain words what usually causes it and what to do, with **Try again** and
  **Check for updates** buttons.

## [0.10.1]

- **Mileage tracking.** **Employees › Mileage**: add vehicles and the mileage rates you use (with
  the dates they start), then record trips by odometer or total miles. Bill trips to a customer or
  job on an invoice, and see Mileage by Vehicle and Mileage by Job reports with the deductible
  amount.
- **Settings in the sidebar.** **Settings** now opens like the other groups, with each section
  listed. The Desktop Migration Wizard moved to **Settings › Import & Migration**, and **File ›
  Utilities › Import / Convert…** opens it too.
- **Fixed:** on Windows, **Condense Data** and **Restore from Backup** could stop with "cannot
  remove … being used by another process" and leave your books as they were. They now work
  reliably.

## [0.10.0]

- **Clean Up Books.** **Accounting › Clean Up Books** gathers QuickBooks Accountant's cleanup tools:
  apply leftover credit memos and vendor credits to open invoices and bills, clear payments stuck in
  Undeposited Funds, write off old invoices in a batch, review list changes for a date range, and
  compare inventory with the Balance Sheet. Every fix shows its totals and asks before posting.
- **Graphs.** **Reports › Graphs**: Income and Expense, Net Worth, Accounts Receivable, Accounts
  Payable, Sales, and Budget vs. Actual. Each uses the same numbers as its report; click a bar or
  slice to open the detail.
- **Reminders and To Do.** A **Reminders** panel on the Dashboard (and **Company › Reminders**) shows
  overdue invoices, bills coming due, to-dos, recurring transactions due, emails waiting to send,
  upcoming tax dates and an overdue backup. Choose what it shows in Settings. Add to-dos from
  **+ New › To Do** or from a customer or vendor.
- **Price levels.** Give customers their own prices: a fixed percentage up or down on everything,
  or set prices per item. Set a customer's default level, and sales forms fill in the right rate
  (a rate you type always wins). The **Item Price List** report shows every level. Price levels
  come in from QuickBooks and IIF imports.
- **Condense Data.** **File › Utilities › Condense Data…** keeps your company file lean: it saves
  a verified **archive** with the full history, then replaces closed years with monthly summary
  entries, keeping anything still open (unpaid invoices and bills, uncleared bank lines). Your
  balances, aging and inventory are checked to match before the condensed file is used, and the
  archive opens read-only any time. Admins only; a closing date is required.
- **QuickBooks for Mac 2010–2012 files** import directly in the Desktop Migration Wizard.
- **Export from an installed QuickBooks (preview).** If QuickBooks Desktop still runs on your
  computer, the Desktop Migration Wizard can ask it for your company's data (AppleScript on a Mac,
  Intuit's QuickBooks SDK on Windows). LocalBooks only reads; it never changes anything in
  QuickBooks. This hasn't been tried against every QuickBooks version yet, so tell us how it goes.

## [0.9.0]

- **QuickBooks-style menu bar.** New **Lists, Company, Customers, Vendors, Employees, Banking** and
  **Reports** menus (plus **View**, and more items under **File** and **Edit**) open each screen and
  entry form, and **Reports** lists every report by group, as in QuickBooks Desktop. The classic
  shortcuts work: **Ctrl+I** invoice, **Ctrl+W** write checks (Windows and Linux), **Ctrl+R**
  register, **Ctrl+J** Customer Center, **Ctrl+T** memorized transactions and **Ctrl+F** Find
  (⌘ on a Mac). On Windows, **Alt** reaches the menus from the keyboard. **Window › New Window** is
  now on every platform.
- **New Company setup assistant.** **File › New Company…** asks for your company details and tax
  form, then suggests a chart of accounts for your industry from the same 33 industries QuickBooks
  offers, numbered QuickBooks-style, with equity accounts that follow your tax form and optional
  sales tax and inventory accounts.
- **Find Transactions.** **Accounting › Find Transactions** (or **Edit › Find…**, or **Advanced
  search…** under the search box) filters by text, dates, number, amount, transaction type, customer
  or vendor, and account.
- **Sort any list** by clicking a column heading; click again to reverse it.
- **Saved and emailed forms are named after the customer**, for example `Invoice 1001 · Acme.pdf`,
  and **Save as PDF** in the print dialog suggests the same name.
- **Open a company without restoring windows**: hold **Option** or **Alt** while clicking **Open**,
  or choose **File › Open Company Without Restoring Windows…**. The company opens on a plain Home
  page with other windows closed, which helps when a page won't load.
- Includes 0.8.1: open a QuickBooks `.qbw`, `.qbb` or `.qbm` file directly from **File › Open or
  Restore Company…**.

## [0.8.1]

- **Open QuickBooks files directly.** **File › Open or Restore Company…**, **Open Company File…**
  and **Restore from Backup…** now let you pick a QuickBooks `.qbw` company file, `.qbb` backup or
  `.qbm` portable file, where before only LocalBooks files could be chosen. LocalBooks creates a new
  company named after the file and opens the Desktop Migration Wizard on it. The QuickBooks file is
  only read.
- **Settings › File & Data Controls** has a **Desktop Migration Wizard…** button, so the QuickBooks,
  Quicken and GnuCash import is next to Open and Restore as well as under **Accounting**.
- QuickBooks files named in capitals (such as `Company.QBB`) can be chosen in every file picker,
  including the Desktop Migration Wizard's.

## [0.8.0]

- **Customize reports.** The new detail reports have a **Customize…** button: choose the dates,
  accounts, names, transaction types, class, item and more; pick and order the columns; and group or
  subtotal by account, name, item, class, type or period. Click any line to open its transaction.
  **Save as…** keeps a customized report under its own name (a memorized report) to run again.
- **About 24 new detail reports**, matching QuickBooks: Profit and Loss Detail, Balance Sheet Detail,
  A/R and A/P Aging Detail, Open Invoices, Unpaid Bills, Customer and Vendor Balance Summary and
  Detail, Sales by Customer and by Product/Service Detail, Income by Customer, Expenses by Vendor
  Detail, Purchases by Vendor and by Item, Check Detail, Deposit Detail, 1099 Detail, Transaction
  List by Date and Journal. All of them export to Excel, CSV and PDF.
- **Job costing.** Bill, expense, check, card and vendor credit lines have a new **Customer:Job**
  column, so costs can be charged to a job. **Reports › Jobs** has Job Profitability, Job Estimates
  vs. Actuals, Item Profitability, Item Estimates vs. Actuals, Profit and Loss by Job, Estimates by
  Job, and Progress Invoices vs. Estimates. Time isn't costed yet, because time entries have no cost
  rate.
- **Accountant & Taxes reports.** Mark a journal entry as an **Adjusting entry** and see them in the
  Adjusting Journal Entries report. Reprint any past reconciliation (Previous Reconciliation Detail
  and Summary), and see what changed after it with Reconciliation Discrepancy (for reconciliations
  you finish from now on). Assign **tax lines** to accounts (Schedule C, 1065, 1120-S, 1120) for the
  Income Tax Summary and Detail; QuickBooks IIF imports bring them in where the file names them.
  Also new: the **Collections** report and **Open Purchase Orders**.
- The tester page has a step-by-step **Move your books** guide: pick the program you're leaving and
  see just your steps.

## [0.7.0]

- **Export to Excel.** Every report, register, list and the audit log can now be saved as an Excel
  workbook, laid out like QuickBooks' own export: real numbers and dates, bold totals, frozen
  headers, and indented accounts.
- **Import PDF bank statements.** Bank Feeds can read the transaction table from a PDF bank or card
  statement. You confirm the columns, and a balance check shows whether the statement adds up. PDFs
  that are only scanned images can't be read.
- **Import from GnuCash.** The Desktop Migration Wizard reads GnuCash books (XML or SQLite) into
  LocalBooks, including customers, vendors, invoices and bills.
- **Smarter Bank Feed suggestions.** When no rule matches, LocalBooks suggests a category and payee
  from similar past transactions, with a confidence level and the transaction it looked like.
  Nothing is approved automatically, and nothing leaves your computer.
- **Daily exchange rates (optional).** Turn on **Look up daily exchange rates** (Settings ›
  Preferences Console › Accounting & tax) to have foreign-currency rates filled in from the European
  Central Bank. Only the currency codes and the date are sent. A rate you type, or one already
  saved, is never replaced.
- **Pay bills by ACH file.** In Pay Bills, choose **Pay by ACH file** to record the payments and
  save a NACHA file to upload to your bank. Vendor bank details are encrypted like tax IDs. Check
  your first file with your bank before relying on it.
- **License notices.** Help now lists the open-source components LocalBooks uses. QuickBooks® is a
  registered trademark of Intuit Inc.; LocalBooks is independent and not affiliated with, endorsed
  by, or sponsored by Intuit.
- Behind the scenes: every release is now checked automatically on macOS, Windows and Linux before
  it's offered as an update. The checks cover the update signatures, upgrading company files from
  the previous version, and launching the app.

## [0.6.0]

- **Import QuickBooks reports.** No working QuickBooks? Every QuickBooks (Desktop for Windows,
  Desktop for Mac and Online) can still export its **Journal** or **General Ledger** report and its
  account, customer, vendor and item lists to Excel or CSV, even in view-only mode. Drop those files
  into the Desktop Migration Wizard and LocalBooks rebuilds your books from them. See the guide for
  the exact report settings.
- **Moving from QuickBooks for Mac.** The migration guide now explains how to bring Mac books over:
  export a copy for QuickBooks for Windows, with no password, and import that.
- **Credit card credits.** Enter a vendor's refund onto a credit card (Expenses & Checks › Card credit,
  or + New). Card credits in QuickBooks, IIF and QIF imports now come in as card credits instead of
  journal entries.
- **Finance charges.** Set a rate, minimum charge and grace period (Settings › Preferences Console ›
  Finance charges), then use **Assess Finance Charges** to add late fees to overdue invoices.
- **Change Item Prices.** Raise or lower the prices of many products and services at once, by an
  amount or a percentage, with a preview before you apply. Every change is in the audit trail.
- **Sales reps, Ship Via and custom fields.** Optional lists for sales forms: a sales rep per
  customer and document (with a Sales by Rep Summary report), shipping method and ship date, and up
  to 15 custom fields for customers, vendors and items, which can print on your forms. They come in
  from QuickBooks and IIF imports too. Nothing changes until you add an entry.
- **New reports:** Profit and Loss and Balance Sheet Previous Year Comparison, Balance Sheet Budget
  vs. Actual (budgets can now include balance sheet accounts), Inventory Valuation Summary and
  Detail, Inventory Stock Status by Item, and Missing Checks.
- **Fixed:** on macOS, "Are you sure?" questions (such as making a new recovery key or deleting a
  budget) cancelled themselves without asking. They now ask inside LocalBooks.
- **Fixed:** editing an invoice that had sales tax, after sales tax was turned off, removed its tax.
- **Fixed:** a View only or Standard user could briefly see Admin settings when LocalBooks reopened
  on the Settings page. Changes were always refused.

## [0.5.1]

- **Tax IDs from newer QuickBooks files.** QuickBooks keeps vendor tax IDs encrypted with its own
  key, which LocalBooks can't read. Books imported before 0.5.0 could show that unreadable text as a
  vendor's tax ID (and on the 1099 report). It is now removed automatically when you open the file,
  or at your next sign-in if users are on. Re-enter those IDs from the vendors' W-9 forms. Real tax
  IDs and notes are never touched.

## [0.5.0]

- **Users and passwords (optional).** Turn them on in Settings › Users & Passwords: everyone signs
  in with their own password and a role (Admin, Standard or View only). Save the recovery key you
  are shown; it is the only way back in if every Admin forgets their password. LocalBooks signs
  everyone out after 15 minutes without activity (an Admin can change this), and can remember your
  sign-in on your own Mac. Files without users work exactly as before.
- **Encrypted tax IDs.** With users on, vendor tax IDs (SSNs and EINs) are encrypted and shown as
  `***-**-6789` to anyone not allowed to see sensitive data. The 1099 report masks them on screen;
  its CSV has them in full for people who may see them.
- **Accountability.** The audit trail now records who did what, including changes to users and
  passwords.
- **Fixed:** a foreign-currency payment could be refused after earlier partial payments on the same
  invoice were voided.
- Behind the scenes: much stronger automated testing of the ledger (thousands of random
  transaction sequences checked against the accounting rules) and saved copies of every report to
  catch unintended changes.
