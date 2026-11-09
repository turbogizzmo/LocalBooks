# Create and send an invoice

An invoice records a sale the customer will pay later. Posting it adds the amount to **Accounts
Receivable** (what customers owe you) and to the income account on each line.

## Create the invoice

1. Click **Sales › Invoices** in the sidebar, or **+ New › Invoice**. The invoice form is at the
   top of the screen.
2. In **Customer**, type the customer's name. Matching customers are listed as you type. For a new
   name, choose **Add as a new customer** (or **Add with details…** to fill in the address and
   terms first); a name you type and post without choosing is added too.
3. Check **Invoice date**, then pick **Terms** (Due on receipt, Net 15, Net 30 or Net 60). The
   **Due date** fills in for you; change it to use custom terms.
4. Fill in the line items:
   - Choose a **Product/Service** to fill in its description, rate and income account, or type
     a **Description** yourself. To add a new product or service on the spot, choose
     **＋ Add new item…** at the end of the list; it is added to Products & Services and fills in
     the line.
   - Enter **Qty** and **Rate**. Decimals are fine (for example 1.5 hours). **Amount** calculates
     itself.
   - Check the **Income account** for each line. The columns run Product/Service, Description,
     Qty, Rate, Class, then the income account (filled in from the item), Amount and Tax, as on
     the desktop sales forms; the same order is on sales receipts, estimates and credit memos.
   - Press **Enter** to move to the next line, or click **Add line**.
5. If you use sales reps, shipping methods or customer custom fields, fill in **Rep**, **Ship
   date**, **Ship via** and the custom fields at the top. The rep and custom fields fill in from
   the customer, and Ship via from **Preferences › Sales & Customers**. These boxes appear only once their lists have entries (see
   [Sales reps, Ship Via and custom fields](customers-and-products.md#sales-reps-ship-via-and-custom-fields)).
6. Check **Bill To** and **Ship To** under the header. They fill in from the customer's billing
   and shipping addresses (a job with no address of its own uses its customer's). Change them for
   this invoice if you need to: the invoice keeps what you typed, and the customer's record stays
   as it is. **Copy Bill To** puts the billing address in Ship To. Fill in **P.O. No.** with the
   customer's purchase order number and **FOB** with the point where the goods become theirs, if
   you use them.
7. Check **Balance due** at the top right, then click **Save & Post to Ledger**.

The invoice appears in the list below with status **○ Open**.

If an Admin turned on **Warn before selling to a customer with overdue invoices** (Preferences ›
Sales & Customers), LocalBooks asks before saving a new invoice or sales receipt for a customer,
or one of their jobs, with invoices past their due date, and says how much is overdue. Click
**Save anyway** to go ahead. When the customer's **Preferred delivery** is Email or Print, **Email
later** or **Print later** is ticked for you.

### Credit limits

When a customer has a **Credit limit** (see
[Add a customer or vendor](customers-and-products.md#add-a-customer-or-vendor)) and an invoice
would take what they owe over it, LocalBooks says how much they owe now and would owe with this
invoice, and asks before saving. Click **Save anyway** to post it, or **Cancel** to go back to the
form. The limit is a warning only; it never stops a sale. What a customer owes includes their
jobs' invoices, less any unused credits. A job without a limit of its own uses its customer's.

### Use the customer's credits

When the customer has credit memos with credit left, the invoice says so under **Balance due**
("Available credits: $150.00 (2 credit memos)"). Hover over it to see each credit memo. When you save
the invoice, LocalBooks asks whether to apply the credits to it now. Click **Apply credits** to use
them, oldest first, up to the invoice's total, or **Cancel** to leave them for later. Applying a
credit posts nothing, because the credit memo already reduced Accounts Receivable; it counts toward
what's paid on the invoice, the same as applying it from **Sales › Credit Memos & Refunds**. Credits
are in the home currency, so an invoice in another currency doesn't offer them.

### Take a payment with the invoice

When the customer pays all or part of the invoice as you write it (a deposit at the counter, or
payment on delivery), tick **Payment received** next to **Save & Post to Ledger**. Enter the amount
(it starts at the invoice total), the payment method, a reference number and the account it's
deposited to. When you save, LocalBooks records the payment together with the invoice, dated the
invoice date: both are saved or, if the payment can't be (more than the invoice total, or an account
money can't be deposited to), neither is. The invoice comes back **Paid** or **Partial**, and the
payment is an ordinary one: it shows in **Receive Payments** and on the customer's history, goes to
**Make Deposits** when it went to Undeposited Funds, and is voided from there if the check bounces.
**Payment received** is offered on new home-currency invoices; to take a payment on an invoice in
another currency, from an estimate or after saving, use **Receive Payments**.

### Service dates and the template

- **Service Date**: click **Service dates** beside the Description heading of an invoice or sales
  receipt to add a Service Date box to each line, for when the work was done (a visit, a delivery,
  a week of work); LocalBooks remembers the choice on this computer, and a saved form whose lines
  have dates always shows them. Lines with one print it in a **Service
  Date** column; the column is left off a form whose lines have none, and Settings › Custom Forms
  can turn it off altogether.
- **Template**: the header's **Template** box picks the form style the document prints, saves as
  PDF and emails with (Settings › Custom Forms). Left on **Default style** (or **Customer's
  style**, when the customer has one), it follows the customer as before. Estimates have it too.

### Addresses on sales receipts, estimates and credit memos

Sales receipts (**Sold To**), estimates and credit memos have the same address, P.O. No. and FOB
boxes, filled in the same way. Each document keeps its own copy: changing a customer's address
later leaves documents already saved as they were. Converting an estimate to an invoice, or
editing an invoice, carries its addresses over. Invoices saved before this version had no copy
and print the customer's current billing address.

A saved credit memo or refund receipt opens as its printed form, with its Credit To address, P.O.
No., lines and total; **Print**, **Save as PDF** and **Email** use the **Credit memo** layout in
Settings › Custom Forms.

## Subtotals, discounts, charges and groups

The **Product/Service** list on invoices, sales receipts and estimates also offers:

- **Subtotal**: adds up the lines above it, back to the previous subtotal. It posts nothing.
- A **discount item**: takes its amount or percentage off the line right above it. Put a
  **Subtotal** above the discount to take it off several lines at once. You can change the
  amount or percentage (type `10%`) on the line. A discount must follow an item, a group or a
  subtotal, and can't be more than that line.
- An **other charge** with a percentage: works out its percentage of the line above (an amount
  charge is an ordinary line).
- **Group: …**: fills in a heading and a line for each of the group's items, at the customer's
  prices. Change or remove the items as you need; tick **Print as one line** on the heading to print
  the group as one line with its total. A discount right under a group comes off the whole group.

The **Discount** box next to the total still works as before: it comes off everything, after the
line discounts and before tax. A taxable discount line lowers the taxable amount; a non-taxable one
doesn't. Subtotals, discounts, charges and group headings print in their places and come back when
you edit the invoice or turn an estimate into an invoice.

## Invoice in another currency

1. Before posting, pick a **Currency** on the form and enter the exchange rate
   (**1 EUR = ? USD**, for example).
2. The form shows the amount in your home currency as well. Your books record the home-currency
   amount and keep the original alongside.

When the customer pays at a different rate, LocalBooks posts the difference to **Exchange Gain or
Loss** for you. See [Record customer payments](customer-payments.md).

## Print or email it

With the invoice open on the form, use **Print ▾** or **Email ▾** in the toolbar. On a new
invoice, **Print ▾ › Post and print…** and **Email ▾ › Post and email…** post it first. Or:

1. In the invoice list, open the row's **Actions** menu.
2. Choose **Print…** to print it, **Save as PDF…** to save a PDF file, or **Email…** to open a
   new message in your mail app with the PDF attached.

Saved and attached PDFs are named after the form and the customer, for example
`Invoice 1001 · Acme.pdf`. Estimates and sales receipts are named the same way, and **Save as PDF**
in the print dialog suggests that name too.

To email several invoices together, tick **Email later** when you post them, then send them from
**Sales › Send Forms**.

To print several together, tick **Print later** when you post them (or choose **Print later** in a
posted invoice's **Actions** menu); they show *To print* in the list. Then choose **File › Print
Forms › Invoices…** (or **Sales › Print Invoices**), untick any to leave for later, and click
**Print…**. LocalBooks asks **Did they all print?**: the invoices you confirm leave the queue, and
the rest wait for the next batch. The invoice form remembers whether you last left Print later on.

Invoices print with your company's form design. To change the title, colors, logo, columns or
message, see [Design your invoices and estimates](custom-forms.md).

The **Bill To** address prints under the customer's name, with **Ship To** beside it when the
invoice has one. The P.O. No., rep, ship date, shipping method, FOB and customer custom fields
print beside them, above the balance due. Each of these can be turned off in the form design. Item custom fields marked for sales forms print under their lines.
Sales receipts and estimates print them the same way. When the invoice list has invoices with a
rep, it shows a **Rep** column. Converting an estimate to an invoice carries its rep, shipping
details and custom fields over.

## Fix a mistake

Double-click the invoice in the list (or choose **Edit…** in its **Actions** menu) to open it, make
your changes and save. LocalBooks voids the original and posts the corrected invoice in its place;
both stay in the journal and the audit trail. Bills, checks and expenses open the same way from
their lists.

An invoice with payments or credits applied can be edited too. The form says how much is applied
("This invoice has $250.00 in payments or credits applied"), and saving asks once more: the payments,
applied credit memos and any bounced check stay applied to the corrected invoice, so its open balance
is the new total less what was already paid. Nothing about the payments changes, including a payment
already deposited, even in a reconciled bank deposit. Three changes are refused while payments or
credits are applied, and the message says so: a new total below what's applied (void or remove a
payment or credit first, or give the customer a credit memo), a different customer, and, for a
foreign-currency invoice, a different currency or exchange rate.

An invoice can't be edited while it is voided, reconciled, or in a closed period. To cancel an invoice instead,
choose **Void Transaction** in its **Actions** menu. See
[Journal entries and voiding](journal-entries-and-voids.md).

## Bill on the statement instead: statement charges

For customers you bill once a month (a retainer, regular visits, small charges you don't want to
send an invoice for each), enter **statement charges** and send a statement instead.

1. In **Sales › Customers**, open a customer's **Actions** menu and choose **Enter statement
   charges…**. The customer's charge register opens, with every charge entered so far and what is
   still open on each.
2. Under **New charge**, enter the date, pick an item (it fills in the description, rate and
   income account) or type a description, and enter the quantity and rate. Leave **Due date**
   blank to make the charge due on its date.
3. Click **Record charge**.

Each charge posts to **Accounts Receivable** for the customer right away and shows in the A/R
aging by its due date. It prints on the customer's next statement as a **Statement Charge** line
with its description, and never prints as an invoice of its own. In the invoice list it is marked
**SC**; in **Receive Payments** it shows as **Statement charge**, and a payment or credit applies to
it like an invoice. To take one back, void it from the invoice list.

## Charge interest on late invoices

To bill customers interest on invoices past their due date, use **Sales › Assess Finance Charges**.
See **Charge interest on overdue invoices** in [Record customer payments](customer-payments.md).

Next: [Record customer payments](customer-payments.md)
