# Customer statements

A statement shows a customer everything in their account for a period: the balance brought
forward, each invoice, payment and credit with a running balance, and the amount due. Make
statements in **Sales › Customer Statements**. They change nothing in your books.

## Make statements

1. Go to **Sales › Customer Statements**. Or choose **Customers › Create Statements…** or **File ›
   Print Forms › Statements…** in the menu bar, or **+ New › Statement**.
2. Set **Statement period from** and **To (statement date)**. They start on the first and last day
   of last month. The statement date is the end of the period: the amount due and the aging are
   worked out as of that day.
3. The **Customers** list shows every customer with a balance on the statement date, or with
   activity in Accounts Receivable during the period, and the balance beside each name. Changing
   the dates refreshes the list and clears your ticks.
4. Click a customer to preview their statement on the right.
5. Tick the customers to send statements to, or tick the box beside **Customers** to tick them all.

The list shows customers, not jobs: a customer's statement includes the invoices, payments and
credits of all their jobs.

## What a statement shows

Each statement has a row for each Accounts Receivable transaction in the period, in date order:

- **Balance forward**: what the customer owed on the first day of the period.
- One row per transaction, with its **Date**, the **Transaction** (for example **Invoice #1042**,
  **Payment #2045** or **Credit Memo #7**), **Details**, the **Amount** (charges positive, payments
  and credits negative) and the running **Balance**.
- **Amount due**: the balance on the statement date.
- The aging of the amount due: **Current**, **1–30 days past due**, **31–60 days**, **61–90 days**
  and **Over 90 days**.

**Details** shows the job's name on a job's rows, and the transaction's memo on the customer's own.
Other rows you may see:

- **Finance Charge #…**: an invoice made by **Assess Finance Charges**.
- **Statement Charge**: a charge entered on the customer's charge register, with its description.
  See [Bill on the statement instead: statement
  charges](invoices.md#bill-on-the-statement-instead-statement-charges).
- **Payment #…**: an overpayment kept as credit, with its reference, the same as any payment.
- **Refund #…**: credit paid back to the customer from **Sales › Credit Memos & Refunds**, with its
  check number when it has one (see [Credit memos and
  refunds](credit-memos-and-refunds.md#refund-credit-that-is-left)).
- **Write-off**: credit written off to income.
- **Bounced Check**: a customer's check that bounced, putting the amount back on the account.
- **Journal Entry**: any other journal entry to Accounts Receivable for the customer.
- **Void**: a voided transaction. The original stays on the statement, and the **Void** row
  reverses it on the same date, so the two add up to zero. A voided payment, for example, shows as
  the payment and then a **Void** row that puts the amount back.

A credit applied to an invoice doesn't add a row: the credit memo already lowered the balance when
it was entered.

### The P.O. No. column

The statement can show the customer's purchase order number beside each invoice and credit memo
that has one. Turn on **P.O. No. column** in the **Statement** design in **Settings › Custom Forms**
(it is off until you do). See [Design your forms](custom-forms.md#change-the-design), where you can
also turn off the aging summary and rename the **Amount** column.

## Print statements

- **Several at once**: tick the customers and click **Print *n* statements** at the top. Each prints
  with its customer's form style, one after another.
- **One**: click the customer and use **Print…** or **Save as PDF…** above the preview.

A customer with their own form style gets it on their statement (see [Styles: several designs, one
per customer](custom-forms.md#styles-several-designs-one-per-customer)).

## Email a statement

1. Click the customer to preview the statement.
2. Click **Email…** above the preview. The message starts addressed to the customer's statement
   contact, or the customer's own email when they have none.
3. Check the address, subject and message, and send it (see [Email it](custom-forms.md#email-it)).

The statement sent is for the period on screen. Statements can't be marked **Email later**,
because their period is chosen when you send them. A **✉** beside a customer's name shows their
statement for the period on screen (the same end date) was emailed; point at it to see when and to
whom. Change the dates and the mark shows for that period's statement instead. Statements emailed
before version 0.15 have no period recorded, so they show no mark.

### Statement contacts

On a customer's record (**Edit…** in the Customer Center), tick **Statements** on the contact who
deals with statements. Their name prints on the statement under the customer's name, and emailed
statements go to their email. See [Add a customer or
vendor](customers-and-products.md#add-a-customer-or-vendor).

## Statements for a customer group

Choose a group under **Customer group** and its customers who have a statement for the period are
ticked for you. With **All customers** (the default), tick the customers yourself; going back to
**All customers** from a group clears the group's ticks.

When the group has a statement reminder, **Mark sent** appears beside it. Print or email the
statements, then click **Mark sent** to start the wait for the next reminder. Clicking **Statements
for** the group in **Reminders**, or **Statements for this group…** on the group in the Customer
Center, opens this screen with the group chosen. See [Statement
reminders](customers-and-products.md#statement-reminders).
