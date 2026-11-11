# Purchase orders

A purchase order records what you have ordered from a vendor. It doesn't change your books: no
account moves until the goods or the vendor's bill arrive and you turn the order into a bill, or
receive the items. Purchase orders are part of inventory, so they show only while **Inventory** is
on in **Preferences › Items & Inventory** (see [Turn purchase orders off](#turn-purchase-orders-off)).

## Create a purchase order

1. Go to **Expenses › Purchase Orders** and click **New purchase order**. From the menu bar, choose
   **Vendors › Create Purchase Orders** (also under **Vendors › Inventory Activities**), or use
   **+ New › Purchase Order**.
2. Choose the **Vendor**. For a new vendor, type the name and add it from the list (see [Add a
   customer or vendor](customers-and-products.md#add-a-customer-or-vendor)). The vendor's
   information panel opens beside the form; hide it if you don't need it.
3. **PO date** starts at today (or the date your preferences give new transactions). Enter the
   **Expected** date if the vendor gave you one; it is optional and prints on the order.
4. Add one line per thing you order:
   - **Stock item** (shown once you track inventory items): pick a tracked item and the line takes
     its inventory account and its name as the description. The account can't be changed while an
     item is chosen.
   - **Category**: the expense or asset account the cost will go to when the order is billed. It
     starts at your first expense account.
   - **Description**: what you're ordering.
   - **Customer:Job** (shown once you have customers): the customer or job the line is for. See
     [Purchase orders for a job](bills.md#purchase-orders-for-a-job).
   - **Class** (shown when class tracking is on), or one **Class** for the whole order in the
     header.
   - **Qty** and **Rate**, or an **Amount** on its own. With a quantity and rate, the amount is
     worked out for you.

   Click **＋ Add split row** for more lines, or × to remove one. Every line needs an account and
   an amount above zero.
5. To take a discount or add a charge (such as freight) after a line, click **Add a discount or
   charge**. It works as on a bill: see [Discounts and charges on bills, checks, credits and
   orders](bills.md#discounts-and-charges-on-bills-checks-credits-and-orders). The **Total** at the
   top includes them.
6. Type a note in **Message to vendor** under the lines, if you like. It prints at the bottom of the
   order and becomes the memo of the bill made from it.
7. Click **Save & New** or **Save & Close**. To print it right away, choose **Print › Save and
   print…** in the form's toolbar instead. **Clear** empties the form without saving, and **Close**
   leaves it.

LocalBooks numbers each order with the next PO number when it is saved; you can't type your own.
Saving says what the order totals and reminds you that nothing posted.

The purchase order has no ship-to address and no field for the vendor's address: the printed order
shows your company, the vendor's name, the dates, the lines and the total.

### If the order needs approval

When an approval rule covers the order (for example, every purchase order of $500.00 or more),
saving doesn't record it. LocalBooks says it was sent for approval, and it isn't an open order until
an Admin approves it under **Expenses › Approvals**. See [Approvals for bills and purchase
orders](bills.md#approvals-for-bills-and-purchase-orders).

## Find and open saved orders

The list on **Expenses › Purchase Orders** shows **PO no.**, **Date**, **Vendor**, **Expected**,
**Memo**, **Status** and **Total**. Click a column head to sort by it. The **Show** box next to the
heading picks which orders appear: **Open** (the default), **Billed**, **Closed** or **All**.

- **Open**: nothing has been billed yet; it can be received, billed or closed.
- **Billed**: turned into a bill with **Create Bill**.
- **Closed**: closed by hand, or closed by LocalBooks once every line has been received.

Double-click an order (or choose **Open** in its **▾** menu) to see it as it prints. The toolbar
above it has:

- **Previous** and **Next** to step through your orders by date, and **Find** to search all
  transactions. **New** goes back to a blank order.
- **Duplicate** to start a new order copied from this one (see [Duplicate an
  order](#duplicate-an-order)).
- **Print** to print it, and **Attach File** to keep the vendor's quote or confirmation with it
  (see [Attach receipts and documents](attachments.md#what-you-can-attach)).
- **Create Bill** to turn it into a bill (see [Turn an order into a bill](#turn-an-order-into-a-bill)).
- **Close PO** to close it without billing, or **Reopen** on a closed order.
- **Edit** and **Delete** (see [Change or delete an order](#change-or-delete-an-order)).

While a new order has unsaved entries, **Previous** and **Next** are greyed out: save or clear it
first.

## Print or save an order as a PDF

- **One order:** in its **▾** menu choose **Print or save as PDF**, or open it and click **Print**.
  The print dialog opens; choose your printer, or save as a PDF from there.
- **Several orders:** choose **File › Print Forms › Purchase Orders…** (see [Print other forms in
  a batch](invoices.md#print-other-forms-in-a-batch)).

Each discount or charge prints under the line it applies to, with its percent if it has one.

LocalBooks doesn't email purchase orders. Save the order as a PDF and attach it to an email in your
mail program.

## Turn an order into a bill

When the vendor's bill arrives for an order you haven't received items on:

1. Choose **Create bill…** in the order's **▾** menu, or open it and click **Create Bill**.
2. Enter the **Bill date** (today to start) and the **Due date**. The due date starts 30 days after
   the bill date and moves when you change the bill date; type another if the vendor's bill says
   so. It can't be before the bill date.
3. Enter the **Vendor's bill no.**, if any.
4. Click **Create bill**.

LocalBooks posts the bill at once, for the order's total: each line keeps its account, item,
description, quantity, rate, class and **Customer:Job**, and the discounts and charges come over
too. The bill debits the lines' accounts and credits **Accounts Payable**, like a bill you enter by
hand. Its memo is the order's message, or *PO #* and the number when there is none. The order
becomes **Billed** and can't be billed again.

A bill made from an order is never held for approval, even if it matches a bill rule: the order was
already approved (or didn't need to be).

Only an **Open** order can be billed this way. Once any of its items have been received, enter the
bill from **Vendors › Enter Bill for Received Items** instead.

## Receive items before the bill

When the goods come before the bill, receive them with **Vendors › Receive Items** so the stock is
on hand. LocalBooks closes the order once every line has come in. How receiving works, and how you
then enter the bill, is in [Receive items before the
bill](bills.md#receive-items-before-the-bill).

## Close or reopen an order

When you no longer expect the rest of an order (the vendor can't supply it, or you cancelled),
choose **Close without billing** in its **▾** menu, or open it and click **Close PO**. A closed
order leaves the open-order reports and can't be received or billed. **Reopen** brings it back as
an open order.

## Change or delete an order

**Edit…** in an open order's **▾** menu (or **Edit** on the open order) puts it back on the form:
change the vendor, dates, lines, discounts and charges, and save. It keeps its PO number, and
nothing posts, because a purchase order never touches your books. A closed order must be reopened
first.

**Delete…** removes an order after you confirm. Delete any files attached to it first.

An order that has been billed, or that items were received against (even on a receipt that was
voided later), can't be changed or deleted: close it, or make a new order. Purchase orders are never
voided.

## Duplicate an order

To order the same things again, choose **Duplicate** in an order's **▾** menu (or open it and
click **Duplicate**). A new order opens with the same vendor, lines, discounts and charges, and
message, dated today; the expected date is left blank. Change what you need and save it: it takes
the next PO number. To change a saved order, duplicate it, fix the copy and close the original.

## What a purchase order does to your books

Nothing. Saving, closing or reopening an order posts no journal entry and changes no balance:
Accounts Payable, your expense accounts and stock on hand stay as they are. The books change only
when you:

- turn the order into a bill (debit the lines' accounts, credit Accounts Payable), or
- receive its items (debit Inventory Asset or the line's account, credit Items Received Not
  Billed), and later enter the bill for them.

## Open purchase order reports

In **Reports › Purchases** (and the Report Center):

- **Open Purchase Orders**: one row per open order, grouped by vendor, with the expected date and
  amount (see [Open Purchase Orders](reports.md#open-purchase-orders)).
- **Open Purchase Orders Detail**: every line of the orders still open, by vendor.

In **Reports › Jobs**, **Open Purchase Orders by Job** lists the same open lines under the customer
or job on each line. See [Lists and other reports](reports.md#lists-and-other-reports).

Inventory reports also count what is on order: **Inventory Stock Status by Vendor** shows each
item's quantity on open purchase orders.

## Turn purchase orders off

Purchase orders belong with inventory. Turning off **Inventory** in **Preferences › Items &
Inventory** (see [Preferences](preferences.md#items--inventory)) takes **Purchase Orders** and
**Receive Items** out of the side bar. Orders you already saved are kept, and you can still open,
print, bill and close them on **Expenses › Purchase Orders**, but LocalBooks won't save a new one or
duplicate an old one until inventory is on again.
