# Enter and pay bills

A bill is something you owe a vendor and will pay later. It sits in **Accounts Payable** until you
pay it.

## Enter a bill

1. Go to **Expenses › Bills** and click **New bill** (or **+ New › Bill**).
2. Choose the **Vendor**, then enter **Bill no.**, **Bill date** and the **Bill total** from the
   vendor's invoice. The **Due date** fills in from the vendor's terms, or for a vendor without
   terms from **Preferences › Bills › Bills are due** (10 days unless an Admin changes it). Type
   another due date if the bill says so. When the terms offer an early-payment discount (such as
   2% 10 Net 30), **Discount date** beside the due date shows the last day to pay and take it.
3. Split the bill across expense accounts, one line per category. A line can be an amount, or a
   quantity × rate (for example 2.5 hours at 40.00).
4. The lines must add up to the bill total. When they do, the split shows as fully allocated.
5. To charge a cost to a job, choose the customer or job in the line's **Customer:Job** column. It
   doesn't change how the bill posts; the [job costing reports](reports.md#job-costing-reports)
   count it as that job's cost.
6. Click **Post Bill**.

The **Bills tracker** lists every bill with its due date, open balance and status.

## Discounts and charges on bills, checks, credits and orders

Bills, checks and expenses, vendor credits and purchase orders can take a discount or a charge
after any split line, such as 2% off the parts or a freight charge on top:

1. Under the split lines, click **Add a discount or charge**.
2. Choose **Discount** or **Charge**, and the split line it follows (it applies to that line).
3. Pick a Discount or Other Charge **item** to fill in the account, text and amount, or choose the
   **Account** yourself. A discount or charge posts to an income or expense account.
4. Enter an amount, or a percent of the line above such as `2%` (rounded to the cent).

A discount comes off the total and a charge is added to it, so on a bill the split lines, less the
discounts, plus the charges, must add up to the bill total. On a bill or check a discount is
credited to its account and a charge is debited, like the split lines; on a vendor credit or a card
credit both post the other way round. Only one discount or charge can follow each split line, and a
discount can't be more than its line.

A purchase order's discounts and charges print on the order and carry over to the bill made from it
with **Create Bill**. A bill entered from received items (**Enter Bill for Received Items**) doesn't
take them yet; add them on that bill.
To give an Other Charge or Discount item its own account on purchase forms (for example a freight
expense account, while its income account is used on invoices), open the item in **Products and
Services** and choose **Expense account on bills and checks**.

## Attach the vendor's invoice

Keep the vendor's invoice (a PDF, a scan or a photo, up to 25 MB) with the bill:

- **On a new bill:** click **Attach the vendor's invoice…** under the lines before you post. The
  file is attached when the bill posts.
- **On a posted bill:** open the row's **Actions** menu (▾) and choose **Attach file…**.

A 📎 next to the bill number marks a bill with files. Click it, or choose **Attachments…** in the
row's menu, to see them: click a file name to save a copy, or × to remove it. The files are kept in
the company file, so they're in your backups, and they stay with the bill if you edit it.

Expenses, checks, invoices, sales receipts, deposits and journal entries take attachments the same
way.

## Approvals for bills and purchase orders

A company can have bills and purchase orders approved before they count. Under **Expenses ›
Approvals**, an Admin adds **rules**: for example "Approve bills of $1,000.00 or more", "Approve
bills from Harbor Leasing" (every one, any amount), or "Approve purchase orders of $500.00 or more".

When you save a bill or purchase order that matches a rule, LocalBooks doesn't record it. It says
**Sent for approval** and the document waits under **Expenses › Approvals**. A waiting bill isn't
posted: it isn't in Accounts Payable, the aging or **Pay Bills**. A waiting purchase order isn't
an open order, so it can't be received or billed.

An Admin then opens **Approvals** and, for each one:

- **Approve** records it as entered: the bill is posted (Dr expense, Cr Accounts Payable) and can
  be paid; the purchase order becomes an open order. If it can't be recorded any more (for example
  its date is now before the closing date), it stays waiting and the reason is shown.
- **Reject…** asks why and keeps the request with that note. Nothing is recorded. Enter it again,
  corrected, if it should go ahead.

**Waiting**, **Approved**, **Rejected** and **All** show the record of every request, who asked,
who decided and when. Approving and rejecting are Admin tasks (see [Users and
passwords](users-and-passwords.md)); in a company file without users, everyone is an Admin, so
approval is a second look rather than a control.

Only bills and orders entered on their forms are held. Imports, bills made from an approved
purchase order, and edits to a bill that was already approved are recorded straight away.

## Fix a mistake on a bill

Double-click the bill in the list (or choose **Edit…** in its **Actions** menu), make your changes
and click **Save changes**. LocalBooks voids the original and posts the corrected bill in its place;
both stay in the journal and the audit trail.

A bill you've already paid part of, or applied a vendor credit to, opens the same way. The form says
how much is applied, and saving asks once more: the payments and vendor credits stay applied to the
corrected bill, and the checks or ACH payments themselves don't change. The edit is refused if the
new total is less than what's applied (void a payment or remove a credit first) or if the vendor
changes. A voided bill, or one in a closed period, can't be edited.

## Receive items before the bill

When goods arrive from a purchase order before the vendor's bill, receive them so the stock is on
hand and can be sold. Choose **Vendors › Receive Items** (with inventory turned on in Preferences ›
Items & Inventory), pick the open purchase order, and enter what came in: **Receive now** starts at
what's still to come on each line, and you can change it for a short or split shipment. Enter the
date and the packing slip number and click **Receive**.

The receipt posts the goods' cost to the item's Inventory Asset account (or a line's expense account,
for freight or a service) against **Items Received Not Billed**, a liability LocalBooks adds the first
time you receive items. It doesn't post to Accounts Payable, so **Pay Bills** doesn't offer it and
the vendor's balance doesn't change until the bill is entered; **A/P Aging Summary** shows the total
as **Items received, not billed** under its own total. The purchase order shows what has been
received, and closes once every line has come in. Receiving more than was ordered asks first.

When the bill arrives, choose **Vendors › Enter Bill for Received Items** (or **Enter Bill** beside
the receipt), enter the vendor's reference number, bill date and due date, and check each line's
**Billed amount** against the vendor's prices. Saving posts the bill: it clears what the receipt put
in Items Received Not Billed and puts the bill's total in Accounts Payable. When the vendor billed a
different price, the difference goes to the line's expense account, or for stock to Inventory Asset
for the units still on hand and to Cost of Goods Sold for any already sold.

To undo a receipt, click **Void** beside it: its stock leaves inventory. A billed receipt can't be
voided; void its bill first, which makes the receipt wait for a bill again. A bill entered for a
receipt isn't edited like other bills: void it and enter it again from the receipt.

## Pay one or more bills

1. On **Expenses › Bills**, click **Pay bills** (or **+ New › Pay Bills**). Every open bill is
   listed. **Filter by** shows one vendor's bills, and **Sort by** orders them by due date,
   discount date, vendor or amount due. A ticked bill that a filter hides is still paid (the form
   says how many). Click a bill to see its terms, discount and the credits used on it below the
   list; **Go to Bill** opens it.
2. Tick the bills to pay. Each ticked bill fills in its open balance; change the **Payment** amount
   to pay part of a bill.
3. Choose the **Payment account** (a bank account or credit card) and the **Payment date**.
   **Ending balance** beside it shows what the account holds (or a card owes) now. It can
   be earlier than a bill's date if you paid the vendor ahead of the bill.
4. Paying from a bank account writes checks: **Check no.** is filled in with the next number for
   that account. Change it if you need to, or clear it for payments made without a check. Leave
   **Print checks** ticked to print them when you pay, or tick **Print later** to post the
   payments now and print the checks in a batch from **File › Print Forms › Checks…**, where they
   take their numbers (see [Print checks later](expenses-and-checks.md#print-checks-later-in-a-batch)).
5. Check **Total payment**, then click **Pay bills**.

Each payment reduces Accounts Payable and the account it was paid from. A partly paid bill stays in
the list with its remaining balance.

Each vendor gets one check for all their ticked bills, as QuickBooks writes them. With several
vendors, the checks are numbered in turn from the first number, and the memo lists the bill
numbers each one pays. The check numbers show in the **Num** column of reports and in the
**Missing Checks** report, alongside checks you write yourself.

> **Tip:** A bill with payments can't be voided until those payments are voided. Void a payment
> from its **Actions** menu.

### Send the vendor a payment stub

A bill payment has a form of its own, the payment stub: who was paid, the date, the account and
check number, and each bill the payment covers with its amount and what was paid on it. Every bill
paid on the same check (or with the same reference) is on one stub.

- **Right after Pay Bills**, a **Payment stubs** panel lists each vendor you paid, with **Print**,
  **Save as PDF** and **Email** buttons. Email goes to the vendor's email address, with a PDF of
  the stub attached.
- **Later**, open a paid bill's **Actions** menu and choose **Print payment stub**, **Save payment
  stub as PDF** or **Email payment stub** for that payment.

Change how the stub looks in **Settings › Custom Forms › Bill payment stub**
(see [Custom forms](custom-forms.md)).

### Use vendor credits when paying

Turn on **Use vendor credits automatically** in **Preferences › Bills › Company Preferences** and
Pay Bills uses each vendor's open credits on the bills you tick, the oldest credit first. The
**Credits used** column shows how much each bill takes, and **Payment** fills in with what is left.
A bill the credits pay in full needs no payment and no check. Credits aren't used when you pay by
ACH file; apply them from **Expenses › Vendor Credits** first.

### Take early-payment discounts

A bill on terms with a discount, such as *2% 10 Net 30*, shows the discount it offers in **Discount
offered**. To take it automatically, tick **Take early-payment discounts automatically** in
**Preferences › Bills** and choose the **Discount account** (an expense account such as Purchase
Discounts). Then, when the payment date is on or before the discount's last day, Pay Bills takes
the discount off a bill nothing has been paid on yet and pays the rest. The discount is recorded
as a vendor credit to the discount account, used on that bill, so it shows in the vendor's
history and reports like any other credit.

### Set the discount or credits on one bill

Click a bill in Pay Bills and its information shows under the list. **Set Discount** lets you
choose the discount to take on that bill, up to what its terms offer on the payment date (0.00
takes none); it needs a **Discount account** in **Preferences › Bills**. **Set Credits** lists the
vendor's open credits with what is left of each; enter how much of each to use on this bill. Click
**Done** and the bill is ticked, its **Payment** becomes what is left, and the totals update. What
you set replaces what the automatic preferences would take on that bill, works with them turned off
too, and is forgotten if you untick the bill. Neither is offered when paying by ACH file.

## Pay bills by ACH file

Many banks let a business upload a file of payments instead of writing checks. LocalBooks can write
that file, in the NACHA format US banks use, and record the payments at the same time. You upload
the file to your bank's website yourself: LocalBooks never connects to the bank or moves money.

> **Warning: check your first file with your bank.** Before you upload a real payment file, ask your
> bank to test one (most have a test or "pre-note" process for new ACH originators), and compare
> what it shows with what you meant to pay. A wrong routing or account number sends money to the
> wrong place, and an ACH credit is hard to get back. Upload each file **once**: the bank pays every
> file it receives.

### Before the first file

1. **Ask your bank to set you up for ACH origination.** It gives you the details its files need.
2. **Enter those details for the bank account you pay from.** In Pay bills, choose **Pay by ACH
   file**, pick the account, and click **Set up ACH…**:
   - **Immediate destination**: your bank's routing number.
   - **Bank name** as the bank wants it.
   - **Immediate origin**: usually ten characters, such as `1` followed by your EIN.
   - **Company name for the bank**, **Company name on statements** (up to 16 characters; vendors
     see it) and **Company ID** (often `1` and your EIN).
   - **Entry description**, such as `VENDOR PAY` (up to 10 characters; vendors see it).

   Only an Admin can change these.
3. **Add each vendor's bank details.** Open the vendor (**Expenses › Vendors**), and under **Bank
   details (ACH)** click **Add**. Enter the routing number, account number, checking or savings,
   and whether you pay them as a **business (CCD)** or a **person (PPD)**. LocalBooks checks the
   routing number's check digit as you type; take the numbers from the vendor's own form, never
   from an email you weren't expecting.

When users are on, bank details are protected like tax IDs: they are encrypted in the company file,
and people without **Can see sensitive data** see only the last four digits (`*****6789`) and can't
change them or pay by ACH. See [Users and passwords](users-and-passwords.md).

### Make a file

1. In **Pay bills**, choose **Pay by ACH file** and the bank account.
2. Set the **Payment date** (the date the payments are recorded) and **Vendors paid on** (the date
   you ask the bank to pay them, usually the next business day or later; your bank may need more
   notice).
3. Tick the bills. Each vendor shows the last four digits of their account, or **No bank details**.
4. Click **Pay by ACH file…** and choose where to save the file.

LocalBooks records one ordinary bill payment per bill, marked ACH, and writes one file that pays each
vendor once, for all their ticked bills together. The vendor's bank statement shows your company
name and entry description, and the bill numbers. Each payment's reference is the ACH trace number,
which your bank uses if a payment is returned.

Before saving, LocalBooks reads the file back and checks it: every record 94 characters, routing
check digits, batch and file totals, entry hash, and the padding banks expect. A file that fails is
not saved.

### Save a file again

If the upload failed or you lost the file, click **ACH files** on the Bills page and **Save again…**.
You get the identical file, and nothing is paid again in LocalBooks. Only upload it if the bank did
not receive the first one. Every file saved is recorded in the audit trail, with amounts and trace
numbers but no account numbers.

If you void one of a file's payments, the file can no longer be saved again, because it would still
pay that bill. Pay what is left in a new file.
