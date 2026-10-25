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

## Apply one payment to several invoices

1. On **Sales › Invoices**, click **Receive payment** (or use **+ New › Receive Payment**).
2. Choose the customer. Their open invoices are listed, oldest first.
3. Type the **Amount received**. LocalBooks applies it to the oldest invoices first; adjust
   **Amount to apply** on any row if the customer told you otherwise.
4. Choose **Deposit to** and **Payment date**, then click **Save & Post Payment**.

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

## Charge interest on overdue invoices (finance charges)

Finance charges bill customers interest on invoices they pay late. Each charge is an invoice of its
own: it debits **Accounts Receivable** and credits the income account you choose, shows in A/R aging
and on customer statements (as **Finance Charge #…**), and is paid with **Receive payment** like any
other invoice.

### Set up finance charges

In **Settings › Preferences Console › Finance charges**:

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
