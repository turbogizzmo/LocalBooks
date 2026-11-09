# Record expenses, write checks and record card credits

Use an expense or check for money that has already left your account. (For bills you'll pay
later, see [Enter and pay bills](bills.md).)

## Record an expense

1. Go to **Expenses › Expenses & Checks** and click **New expense** (or **+ New › Expense**).
2. Enter the **Payee**, then choose the **Payment account**: a bank account, or a credit card for
   card purchases.
3. Set the **Payment date** and **Payment method**.
4. Split the amount across one or more expense categories, one line each. The **Total** adds up the
   lines.
5. Click **Save & Post Expense**. The expense reduces the payment account and adds to each category.

Checks, card charges and card credits have the same **Customer:Job** column on each line as bills:
choose a customer or job to count that line as the job's cost in the
[job costing reports](reports.md#job-costing-reports). Vendor credits have it too.

## Write a check

1. On the same screen click **Write check** (or **+ New › Check**).
2. Enter the **Payee**, choose the **Bank account** and **Check date**. **Check no.** is filled in
   with the next number for that account, after its checks and any bill payment checks; change it
   if you need to.
3. Add the category lines and click **Save & Post Check**.

Beside the bank account (or the card, on a card charge or credit), **Ending balance** shows the
account's balance now, with everything posted to it so far; it updates after each save.

To keep the receipt with an expense or check, click **Attach a receipt…** before you post, or
choose **Attach file…** in a posted row's **Actions** menu. See
[Attach the vendor's invoice](bills.md#attach-the-vendors-invoice).

## Print checks later, in a batch

If you print checks on check stock, write them now and print them together later.

1. On **Write check**, tick **Print later**. **Check no.** shows *To print*: the check posts now,
   and gets its number when it prints. The box stays ticked for your next check until you untick
   it. An unnumbered check already posted can be queued from its **Actions** menu (**Print later**).
   Checks from **Pay Bills** with **Print later** ticked wait in the same queue, one per vendor;
   they keep their payment date when printed.
2. Choose **File › Print Forms › Checks…** (or **Expenses › Print Checks**).
3. Pick the **Bank account**. Check that **First check number** matches the number printed on the
   first check in the printer; the checks take numbers in order from it. Untick any you don't
   want to print now, then click **Print…**.
4. When printing finishes, LocalBooks asks **Did they all print?** Choose **Yes, all printed**, or
   **Some didn't** and untick the checks that jammed or printed wrong.

Only the checks you confirm keep their numbers and leave the queue; the rest stay queued without a
number, to print again. **Cancel** saves nothing. A check's number can't be changed once it is
printed, and a number already used on the account is refused.

## Record a credit card credit

Use a card credit when a vendor gives money back onto a credit card: a refund for a return, a
billing correction, or a credit for a cancelled order. It is the reverse of a card charge.

1. On the same screen click **Card credit** (or **+ New › Credit Card Credit**).
2. Enter the **Payee** (the vendor giving the credit), choose the **Credit card** it goes to and the
   **Credit date**.
3. Add a line for each category the credit goes back to, usually the ones the original charge
   used. The **Credit amount** adds up the lines.
4. Click **Save & Post Credit**. The card's balance goes down by the credit, and each category goes
   down by its line.

Card credits are listed with expenses and checks as **Card credit**, with a negative amount, and
as **Credit Card Credit** in the card's register, the general ledger and the journal. Expense
reports such as **Profit & Loss** and **Expenses by Vendor** subtract them. Edit or void one from
its **Actions** menu, like an expense.

A card credit only goes to a credit card account. A refund into a bank account is a
[deposit](deposits-and-transfers.md) instead.

When you import a QuickBooks company file or IIF file with **Full import**, its credit card credits
(`CCARD REFUND` in IIF) come in as LocalBooks card credits.

## Type a check or deposit into a register

A bank or credit card register (**Accounting › Account Registers**, or double-click the account in
the Chart of Accounts) ends in a blank entry row, as a checkbook does:

1. Enter the **Date**, the check **Number** (a bank register fills in the next one), the **Payee**
   and the **Account** it goes to, and a **Memo** if you like.
2. Type the amount in one of the two columns: **Payment** or **Deposit** in a bank register,
   **Charge** or **Payment** in a credit card register.
3. Click **Record**, or press **Enter** once the row is complete. **Restore** clears the row.

Each row is recorded as the transaction its form would make: a payment from a bank account is a
check, a deposit is a deposit, a charge is a credit card expense, and a payment or refund on a card
is a card credit. They show up in Expenses & Checks or Deposits & Transfers like any other, and you
edit or void them there or from the register's row menu.

Two accounts make a different transaction:

- **Another bank account** records a **transfer** between the two: out of this account when the
  amount is in **Payment** (or **Charge**), into it when it is in **Deposit** (or **Payment** on a
  card). No check number is used.
- **Undeposited Funds**, on a bank register's **Deposit** side, deposits payments waiting there: the
  one whose amount matches, or else all of them dated up to the row's date when together they make
  the amount. If neither matches, LocalBooks says so; choose the payments in **Make Deposits**.

To spread one row over several accounts, click **Splits…**: a line grid opens under the row, with
an account, memo, class and (for payments and charges) customer or job on each line. The row's
amount is the total of the lines. Click **Splits…** again to go back to one account.

A name typed in a deposit's **Received from** that is one of your customers or vendors becomes the
deposit's Received From: the register shows it as the payee, and the deposit is listed in that
name's transactions and in reports by name. A name on neither list is kept at the start of the
deposit's memo instead. Deposits and card charges have no number.

### Find, filter and view a register

- **Go To…** finds the next transaction by number, payee, memo or amount. **Next** and **Back**
  move through the matches.
- **Dates** shows **All**, **This month** or **This year**; type a **From** and **To** date for any
  other range. A register that starts after its first transaction opens with the balance forward.
- **2-line** shows each transaction on two lines: date, number, payee and amounts on the first;
  type, account, class and memo on the second. LocalBooks remembers your choice.
- **Print…** (or **File › Print…**, or **Ctrl+P** in the register) prints the register for its dates: every
  transaction in date order with the balance forward and the ending balance. **Export** saves the
  same rows to CSV or Excel.
- **QuickReport** (**Ctrl+Q**) opens the account's transactions as a report for the register's
  dates. Click a transaction, then press **Ctrl+H** for its
  [Transaction History](reports.md#quickreport-transaction-history-and-transaction-journal) or
  **Ctrl+Y** for its Transaction Journal.

The register shows each document's own number in **No.** (the check, invoice or bill number);
hover over it for the journal entry number. Printing a register is not in LocalBooks yet: use
**Export** to save it as a CSV or Excel file.

## Pay in another currency

Choose a **Currency** and exchange rate on the expense or check. Your bank account is reduced by the
home-currency amount, and the original amount is kept alongside.

> **Tip:** To undo an expense, check or card credit, open its **Actions** menu and choose **Void Transaction**.
