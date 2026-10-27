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

## Pay in another currency

Choose a **Currency** and exchange rate on the expense or check. Your bank account is reduced by the
home-currency amount, and the original amount is kept alongside.

> **Tip:** To undo an expense, check or card credit, open its **Actions** menu and choose **Void Transaction**.
