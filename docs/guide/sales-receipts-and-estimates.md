# Sales receipts and estimates

## Record a sale paid on the spot (sales receipt)

Use a sales receipt when the customer pays at the time of sale, so no invoice is needed.

1. Go to **Sales › All Sales** and click **New sales receipt** (or **+ New › Sales Receipt**).
2. Optionally enter **Customer (optional)**, then set **Sales receipt date** and **Reference
   no.**, and click the **Payment method** tile: **Cash**, **Check**, **Card**, **Bank / ACH**, or
   **More** for Wire and Other.
3. Choose **Deposit to**: a bank account, or **Undeposited Funds** to deposit it later with other
   money.
4. Add the line items as on an invoice and check **Amount received**.
5. Click **Save & Post to Ledger**.
   To post and print or email it in one step, use **Print ▾ › Post and print…** or **Email ▾ ›
   Post and email…** in the toolbar instead.
6. To send the customer a copy later, open the receipt's menu in **All Sales** and choose **Email…**
   (or **Email later**, to send it with others from **Sales › Send Forms**), **Print…** or
   **Save as PDF…**.

## Quote a customer (estimate)

An estimate doesn't affect your books until it becomes an invoice.

1. Go to **Sales › Estimates** and click **New estimate** (or **+ New › Estimate**).
2. Enter the **Customer**, **Estimate date** and **Expiration date**, then add the line items. A
   line can be priced at 0.00 to show something included at no charge; the estimate's total must
   still be above zero.
3. Click **Save Estimate**.
4. To send it, open the estimate's menu and choose **Email…**, **Print…** or **Save as PDF…**. Estimates have
   their own form design: see [Design your invoices and estimates](custom-forms.md).

## Turn an estimate into an invoice

1. When the customer agrees, open the estimate's menu and choose **Mark accepted**. (You can also
   mark it pending or rejected.)
2. Choose **Edit as invoice…**. The estimate's customer and lines load into the invoice form.
3. Change anything you need (hours, prices, extra lines), check the dates, and click **Save & Post to
   Ledger**.

The estimate shows **Converted** with the invoice number once the invoice posts. If you leave the
form without posting, it stays open. **Convert to invoice now** skips the form: it posts the
estimate unchanged, dated today and due in 30 days. See [Create and send an invoice](invoices.md).

Lines priced at 0.00 are left off the invoice, since an invoice line needs an amount. To bring them
over instead (and price them on the invoice), untick **Don't put zero-amount lines on the
invoice** in **Preferences › Sales & Customers**.

Invoicing an estimate in parts (progress invoicing) isn't available yet: an estimate becomes one
invoice. To ship and bill an accepted estimate in parts, make it a sales order instead: choose
**Create sales order** in its menu (see [Sales orders](sales-orders.md#from-an-estimate)).

### Turn estimates off

A business that never quotes can untick **Create estimates** in **Preferences › Jobs & Estimates ›
Company Preferences**. Estimates then leave the side bar and **+ New**, and **Customers › Create
Estimates** is greyed out. Estimates already saved are kept; turn the option back on to see them.
