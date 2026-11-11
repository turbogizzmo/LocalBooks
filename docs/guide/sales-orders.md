# Sales orders

A sales order records what a customer has ordered before you ship it and bill them. It doesn't
change your books: receivables, income and stock stay as they are until you invoice it. You can
invoice an order all at once or in parts, as you ship.

## Take an order

1. Go to **Sales › Sales Orders** and click **New sales order** (or **Customers › Create Sales
   Orders**).
2. Enter the **Customer**, **Order date** and, if you know it, the **Expected** ship date.
   **Order #** shows the next number; type another to use it (see
   [Invoice numbers](invoices.md#invoice-numbers)). A saved order keeps its number when you change it. Rep, ship
   via, P.O. No., FOB and the addresses work as on an invoice.
3. Add the item lines with quantity and rate. If you charge sales tax, pick the rate; tax is worked
   out again on each invoice.
4. Click **Save sales order**.

A sales order holds item lines priced above zero; add subtotals, discounts and other charges on the
invoice.

### From an estimate

When a customer accepts an estimate and you'll ship it over time, open the estimate's menu in
**Sales › Estimates** and choose **Create sales order**. The order takes the estimate's customer,
item lines, sales tax, rep, ship via and addresses, dated today. A discount, percent charges,
subtotals and lines priced at 0.00 stay on the estimate (the message lists them): add them on the
invoice. The estimate then shows its sales order number, and you invoice it through the order, not
from the estimate.

### Print, save or email an order

From an order's **▾** menu: **Print…**, **Save as PDF…**, **Print Envelope…**, **Email…** or
**Email later** (sent with others from **Sales › Send Forms**). Sales tax shows at the order's rate;
each invoice works it out again for what it ships. Change the order's look in **Settings › Custom
Forms › Sales order**.

## Invoice an order

From the order's **▾** menu in the list:

- **Create invoice for all that is left** opens the invoice form with every open line at the
  quantity still to ship.
- **Create invoice for part…** asks how much of each line ships now (leave a line at 0 to ship it
  later), then opens the invoice form with those quantities.

In the invoice form you can change rates, remove lines and add others (freight, a discount) before
you post. A line can't ship more than is left on the order; change the order first if the customer
ordered more. The order is updated only when the invoice posts.

The list shows each order's **Status**: **Open** (nothing invoiced yet), **Partly invoiced**, or
**Closed** (everything invoiced, or closed by hand), and what is **Not invoiced** yet.

- **Voiding** an invoice made from an order puts its quantities back on the order.
- **Editing** such an invoice keeps the quantities it took. To change how much of the order it
  shipped, void it and invoice again.

## Change, close or delete an order

- Double-click an order (or **▾ › Open**) to change it. Once part of a line is invoiced, the line
  can't go below what was invoiced or be removed, and the customer can't change.
- **Close** an order when the rest won't ship (the customer cancelled it). **Reopen** brings the
  rest back.
- **Delete** removes an order nothing has been invoiced from. It never posted, so nothing in your
  books changes. An order with invoices can only be closed.

## Backorders and stock

Stock leaves when an order is invoiced, not when it is taken. Until then LocalBooks shows what is
promised:

- **On sales orders**: what is left to ship on open orders, for each item.
- **Available**: on hand less what is on sales orders. Below zero, that much is backordered.
- **Backordered** on a line: the part that stock on hand can't cover, oldest orders first.

These appear in **Reports › Inventory Stock Status by Item**, in an item's information panel, and in
the reports below.

## Reports

In **Reports › Sales** and the Report Center, and as tabs at the top of **Sales › Sales Orders**:

- **Open by customer**: every open line under its customer: ordered, invoiced, left, backordered
  and the open amount, with subtotals.
- **Open by item**: the same lines under each item, with on hand, on sales orders and available.
- **Pending sales**: by customer, what has been ordered and not yet invoiced.

Each one prints and exports like any report.

## Turn sales orders off

A business that never takes orders can untick **Use sales orders** in **Preferences › Sales &
Customers › Company Preferences**. Sales Orders then leaves the side bar, and **Customers › Create
Sales Orders** is greyed out. Orders already saved are kept, and invoices made from them don't
change.
