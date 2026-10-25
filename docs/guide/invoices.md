# Create and send an invoice

An invoice records a sale the customer will pay later. Posting it adds the amount to **Accounts
Receivable** (what customers owe you) and to the income account on each line.

## Create the invoice

1. Click **Sales › Invoices** in the sidebar, or **+ New › Invoice**. The invoice form is at the
   top of the screen.
2. In **Customer**, type the customer's name. Existing customers are suggested as you type; a new
   name is added as a customer when you post.
3. Check **Invoice date**, then pick **Terms** (Due on receipt, Net 15, Net 30 or Net 60). The
   **Due date** fills in for you; change it to use custom terms.
4. Fill in the line items:
   - Choose a **Product/Service** to fill in its description, rate and income account, or type
     a **Description** yourself.
   - Enter **Qty** and **Rate**. Decimals are fine (for example 1.5 hours). **Amount** calculates
     itself.
   - Check the **Income account** for each line.
   - Press **Enter** to move to the next line, or click **Add line**.
5. Check **Balance due** at the top right, then click **Save & Post to Ledger**.

The invoice appears in the list below with status **○ Open**.

## Invoice in another currency

1. Before posting, pick a **Currency** on the form and enter the exchange rate
   (**1 EUR = ? USD**, for example).
2. The form shows the amount in your home currency as well. Your books record the home-currency
   amount and keep the original alongside.

When the customer pays at a different rate, LocalBooks posts the difference to **Exchange Gain or
Loss** for you. See [Record customer payments](customer-payments.md).

## Print or email it

1. In the invoice list, open the row's **Actions** menu.
2. Choose **Print…** to print it, **Save as PDF…** to save a PDF file, or **Email…** to open a
   new message in your mail app with the PDF attached.

To email several invoices together, tick **Email later** when you post them, then send them from
**Sales › Send Forms**.

Invoices print with your company's form design. To change the title, colors, logo, columns or
message, see [Design your invoices and estimates](custom-forms.md).

## Fix a mistake

Posted invoices can't be edited. Open the row's **Actions** menu and choose **Void Transaction**,
then create a corrected invoice. An invoice that already has payments can't be voided until the
payments are voided. See [Journal entries and voiding](journal-entries-and-voids.md).

## Charge interest on late invoices

To bill customers interest on invoices past their due date, use **Sales › Assess Finance Charges**.
See **Charge interest on overdue invoices** in [Record customer payments](customer-payments.md).

Next: [Record customer payments](customer-payments.md)
