# Enter and pay bills

A bill is something you owe a vendor and will pay later. It sits in **Accounts Payable** until you
pay it.

## Enter a bill

1. Go to **Expenses › Bills** and click **New bill** (or **+ New › Bill**).
2. Choose the **Vendor**, then enter **Bill no.**, **Bill date**, **Due date** and the **Bill
   total** from the vendor's invoice.
3. Split the bill across expense accounts, one line per category. A line can be an amount, or a
   quantity × rate (for example 2.5 hours at 40.00).
4. The lines must add up to the bill total. When they do, the split shows as fully allocated.
5. Click **Post Bill**.

The **Bills tracker** lists every bill with its due date, open balance and status.

## Pay one or more bills

1. On **Expenses › Bills**, click **Pay bills** (or **+ New › Pay Bills**). Every open bill is
   listed.
2. Tick the bills to pay. Each ticked bill fills in its open balance; change the **Payment** amount
   to pay part of a bill.
3. Choose the **Payment account** (a bank account or credit card) and the **Payment date**.
4. Check **Total payment**, then click **Pay bills**.

Each payment reduces Accounts Payable and the account it was paid from. A partly paid bill stays in
the list with its remaining balance.

> **Tip:** A bill with payments can't be voided until those payments are voided. Void a payment
> from its **Actions** menu.

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
