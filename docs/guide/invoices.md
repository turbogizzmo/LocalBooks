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
   - Check the **Income account** for each line.
   - Press **Enter** to move to the next line, or click **Add line**.
5. If you use sales reps, shipping methods or customer custom fields, fill in **Rep**, **Ship
   date**, **Ship via** and the custom fields at the top. The rep and custom fields fill in from
   the customer. These boxes appear only once their lists have entries (see
   [Sales reps, Ship Via and custom fields](customers-and-products.md#sales-reps-ship-via-and-custom-fields)).
6. Check **Bill To** and **Ship To** under the header. They fill in from the customer's billing
   and shipping addresses (a job with no address of its own uses its customer's). Change them for
   this invoice if you need to: the invoice keeps what you typed, and the customer's record stays
   as it is. **Copy Bill To** puts the billing address in Ship To. Fill in **P.O. No.** with the
   customer's purchase order number and **FOB** with the point where the goods become theirs, if
   you use them.
7. Check **Balance due** at the top right, then click **Save & Post to Ledger**.

The invoice appears in the list below with status **○ Open**.

### Credit limits

When a customer has a **Credit limit** (see
[Add a customer or vendor](customers-and-products.md#add-a-customer-or-vendor)) and an invoice
would take what they owe over it, LocalBooks says how much they owe now and would owe with this
invoice, and asks before saving. Click **Save anyway** to post it, or **Cancel** to go back to the
form. The limit is a warning only; it never stops a sale. What a customer owes includes their
jobs' invoices, less any unused credits. A job without a limit of its own uses its customer's.

### Addresses on sales receipts, estimates and credit memos

Sales receipts (**Sold To**), estimates and credit memos have the same address, P.O. No. and FOB
boxes, filled in the same way. Each document keeps its own copy: changing a customer's address
later leaves documents already saved as they were. Converting an estimate to an invoice, or
editing an invoice, carries its addresses over. Invoices saved before this version had no copy
and print the customer's current billing address.

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

An invoice can't be edited while it is voided, reconciled, in a closed period, or has payments or
credits applied: void its payments and remove its credits first. To cancel an invoice instead,
choose **Void Transaction** in its **Actions** menu. See
[Journal entries and voiding](journal-entries-and-voids.md).

## Charge interest on late invoices

To bill customers interest on invoices past their due date, use **Sales › Assess Finance Charges**.
See **Charge interest on overdue invoices** in [Record customer payments](customer-payments.md).

Next: [Record customer payments](customer-payments.md)
