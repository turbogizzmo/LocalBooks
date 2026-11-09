# Record customer payments

Recording a payment reduces what the customer owes (**Accounts Receivable**) and puts the money in
a bank account, or in **Undeposited Funds** until you deposit it.

## Pay one invoice

1. Go to **Sales › Invoices** and click the open invoice in the list.
2. Enter the **Amount** (the full balance, or less for a partial payment), the **Payment date**,
   **Method** and an optional **Reference no.** such as the check number.
3. Choose **Deposit to**: a bank account, or **Undeposited Funds** if the money goes to the bank
   later together with other payments.
4. Click **Record payment**. The invoice becomes **◐ Partial** or **● Paid**.

The payment date can be earlier than the invoice date, for a customer who paid early or a deposit
taken before you billed the work.

## Apply one payment to several invoices

1. On **Sales › Invoices**, click **Receive Payments** in the toolbar (or use **+ New › Receive
   Payment**).
2. In **Received from**, choose the customer. Their open invoices are listed, oldest first, and
   **Customer balance** shows at the top right.
3. Type the **Amount received**. LocalBooks applies it for you: to an invoice whose balance is
   exactly that amount, otherwise to the oldest invoices first. Adjust the **Payment** on any row
   if the customer told you otherwise, or tick and untick invoices in the first column. To spread
   the amount received over the invoices again, click **Auto Apply** in the toolbar.
4. Click how you were paid: **Cash**, **Check**, **Card** (credit or debit), **Bank / ACH** (ACH,
   e-check or bank transfer), or **More** for **Wire** and **Other**. The chosen tile is
   highlighted.
5. Choose **Deposit to** and **Payment date**, then click **Save & Post Payment** (or **Save** in
   the toolbar). **New** in the toolbar clears the form to start over.

### How payments are applied (Preferences › Payments)

An Admin chooses, in **Preferences › Payments › Company Preferences**:

- **Apply payments:** *an exact match, else oldest first* (the usual), *oldest invoices first*, or
  *not automatically*, where you tick the invoices yourself. **Auto Apply** on the form always
  applies, whichever you choose.
- **Calculate the amount:** when on, the **Amount received** becomes the total of the invoices you
  tick or type a payment on.
- **Deposit to:** when on, payments and sales receipts go to **Undeposited Funds** unless you
  choose another account; when off, they start on your first bank account.
- **Early-payment discounts:** see [Take a customer's early-payment discount](#take-a-customers-early-payment-discount).

### Take a customer's early-payment discount

An invoice on terms with a discount, such as *2% 10 Net 30*, shows the discount it offers and its
last day in **Discount offered**. To let Receive Payments take it:

1. Create an income account for the discounts, such as **Sales Discounts** (an income account
   with a debit balance reduces your sales on the Profit & Loss). Some companies use an expense
   account instead; either works.
2. In **Preferences › Payments › Company Preferences**, choose it as the **Discount account**. To
   have the discount taken without typing it, also tick **Take customers' early-payment discounts
   automatically**.

Receive Payments then shows a **Discount** column. When the payment date is on or before the
discount's last day and no discount has been taken on the invoice yet, it offers the terms'
discount on the invoice's open balance: the whole invoice, or what is left of one already
part-paid (2% of the 150.00 left on a 200.00 invoice is 3.00):

- With the automatic option on, ticking the invoice (or **Auto Apply**, when the money pays the
  rest of it in full) fills in the discount and sets the **Payment** to what is left. A customer
  who pays 490.00 on a 500.00 invoice on *2% 10 Net 30* closes it.
- You can change the discount, up to what the terms offer, or clear it to take none. With the
  automatic option off, type the discount yourself.

The payment posts one entry: the money to **Deposit to**, the discount to the discount account,
and the two together off Accounts Receivable, so the invoice shows as **Paid** and the customer's
statement shows the payment and discount on one line. Only the money goes to Undeposited Funds and
into the bank deposit. Voiding the payment reverses the discount with it, and the invoice offers
the discount again while its date allows.

A payment that took a discount can bounce like any other ([Record a bounced check](#record-a-bounced-check)):
the bank goes down by the money it received, the discount is taken back off the discount account,
and the invoice opens again for the whole amount the payment settled.

Not covered yet: invoices in another currency get no early-payment discount in Receive Payments.

## Payments in another currency

For an invoice in a foreign currency, enter the exchange rate on the payment date. LocalBooks
shows the amount received in your home currency and the exchange gain or loss before you post,
and the final payment always clears the invoice exactly.

## Deposit payments held in Undeposited Funds

When you take a batch of checks to the bank, record one deposit so your books match the bank
statement. See [Bank deposits and transfers](deposits-and-transfers.md).

> **Tip:** To undo a payment, open its **Actions** menu and choose **Void Transaction**. The invoice
> returns to Partial or Open. A payment that is already part of a bank deposit can't be voided
> until the deposit is voided.

## Record a bounced check

When the bank returns a customer's check unpaid:

1. Choose **Banking › Record Bounced Check…** (or **Banking › Bounced Checks**).
2. Under **Check that bounced**, pick the payment. Only payments that reached the bank are listed,
   whether they were deposited straight to the bank or in a deposit from Undeposited Funds. (A
   payment still in Undeposited Funds never reached the bank: void it instead.)
3. Enter the **Date it bounced**, the **Bank fee** the bank charged you and the expense account for
   it, and, if you charge the customer for the returned check, the **Customer fee** and the income
   account for it.
4. Click **Record Bounced Check**.

LocalBooks posts one entry: Accounts Receivable goes back up by the check, and the bank goes down
by the check plus the bank's fee (the fee goes to its expense account). The invoice the check paid
opens again, so it shows in A/R aging and on statements from the bounce date, and the customer can
pay it again. The customer fee goes on a new invoice, *Returned check charge*.

Money from a deposit's **other funds** lines can bounce too: it goes back to the account the
deposit credited.

A **sales receipt** from a customer can bounce as well. It has no invoice to reopen, so LocalBooks
charges it back: the bank goes down by the check (and the bank's fee), and a new invoice,
*Returned check: sales receipt #…*, bills the customer for it, with the customer fee as a second
line. The sale's income is unchanged; the customer pays the new invoice in Receive Payments.
Voiding the bounced check voids that invoice too.

A **foreign-currency payment** bounces at the rates it was recorded at: the bank goes down by the
home-currency amount it received, Accounts Receivable goes back up by what the payment cleared, and
the exchange gain or loss the payment realized is reversed. The invoice opens again for its
foreign-currency amount.

To undo it, open the bounced check's **Actions** menu on the Bounced Checks screen and choose
**Void bounced check**: the bank and the fee go back, the payment counts toward the invoice again,
and the fee invoice is voided (refused if the customer has already paid it). While a bounced check
stands, its payment, the invoice and the deposit it was in can't be voided.

## Charge interest on overdue invoices (finance charges)

Finance charges bill customers interest on invoices they pay late. Each charge is an invoice of its
own: it debits **Accounts Receivable** and credits the income account you choose, shows in A/R aging
and on customer statements (as **Finance Charge #…**), and is paid with **Receive payment** like any
other invoice.

### Set up finance charges

In **Settings › Preferences Console › Finance Charge › Company Preferences**:

- **Annual interest rate**: the yearly percentage, such as `18` or `1.5` (up to three decimals).
- **Minimum charge**: a smaller charge is raised to this amount. Leave it at `0.00` for none.
- **Grace period**: how many days past its due date an invoice may be before it is charged.
  Interest still runs from the due date once the grace period has passed.
- **Finance charge account**: the income account the charges are credited to, for example
  *Finance Charge Income* or *Interest Income*. Finance charges stay off until you choose one.
- **Charge on finance charges**: whether an overdue finance charge invoice itself earns a finance
  charge. Off by default.

### Assess finance charges

1. Go to **Sales › Assess Finance Charges** and choose the **Assessment date** (today by default).
2. Every customer with an overdue balance on that date is listed with their overdue balance, the
   interest worked out, and the **Finance charge** to bill. Click a customer's name to see each
   overdue invoice, the date interest runs from and the number of days.
3. Change any charge you want to (for example to waive part of it), and untick customers you
   don't want to charge.
4. Click **Assess charges**. LocalBooks creates one finance charge invoice per ticked customer,
   dated and due on the assessment date. The invoice list marks them **FC**.

### How the charge is worked out

- An invoice counts as overdue when it was issued on or before the assessment date, its due date
  is more than the grace period before that date, and it still has a balance **on that date**:
  payments and credits dated later don't reduce it.
- Interest runs from the due date, or from the date of the last finance charge on that invoice, so
  no day is charged twice. Voiding a finance charge invoice makes those days chargeable again.
- All the arithmetic is in whole cents. For each customer:

  `charge = Σ (balance × days × annual rate) ÷ 365`, rounded half up to the cent once, on the total.

  Every year counts as 365 days. For example, $1,000.00 overdue for 30 days at 18% is
  1,000.00 × 30 × 0.18 ÷ 365 = $14.794…, so the charge is **$14.79**.
- If the result is below the minimum charge, the minimum is charged instead.
- Only invoices in your home currency are charged.

> **Tip:** To take back a finance charge, void its invoice from **Sales › Invoices**. To waive one
> after the customer has seen it, apply a credit memo to it instead, so the statement shows both.
