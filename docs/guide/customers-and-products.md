# Customers, products and services

## Add a customer or vendor

1. Go to **Sales › Customers** (or **Expenses › Vendors**).
2. In the first row of the list, type the name in **New customer name** (or **New vendor name**)
   and, optionally, an email address.
3. Click **Add**.

The list shows each customer's open balance in Accounts Receivable, or what you still owe each
vendor. Use **Find a customer** to filter the list. Customers are also added automatically when you
post an invoice for a new name.

To add a company name, billing address, phone, payment terms or notes, open the customer's
**Actions** menu and choose **Edit…**. The billing address prints under **Bill to** on invoices
and estimates, and the email address is filled in when you choose **Email…** on one of them.

## Add a product or service

Products and services make invoices and sales receipts quicker to fill in.

1. Go to **Sales › Products & Services**.
2. In the first row, enter the name in **New product or service**, a **Description on sales
   forms**, the **Rate**, and the **Income account** it belongs to.
3. Click **Add**.

Choosing the product on an invoice or sales receipt then fills in its description, rate and income
account. You can still change them on the form.

To keep count of a product you stock, see [Track inventory](inventory.md).

## Change many prices at once

**Change prices…** sets new sales prices for a group of products and services in one step, for
example to raise everything 5% at the start of the year.

1. Go to **Sales › Products & Services** and click **Change prices…**.
2. Under **Show**, pick which items to list: all of them, only **Inventory** (stock you track), or
   **Services and non-inventory**. Tick **Inactive items** to include those too.
3. Tick the items to change, or use the box in the header to tick everything shown.
4. Describe the change:
   - **Raise by** or **Lower by**, and an amount: a percentage (such as `5` or `2.5`) or a
     **fixed amount** (such as `1.00`).
   - **Based on** the **Current price**, or the **Unit cost** (to set prices as a markup on cost).
   - **Round**: leave prices to the cent, round them to the nearest step (5¢, 10¢, 25¢, 50¢, $1,
     $5 or $10; halves go up), always **up** or **down** to a step, or **up to a price ending in
     .99**.
5. Check the **New price** column. Items that won't change say why in **Note**.
6. Click **Apply price changes** and confirm.

How the new price is worked out, in whole cents:

- A percentage: the base price plus base × percentage, with the change rounded to the cent (half a
  cent rounds away from zero). For example, 10% up from $19.99 is $19.99 + $2.00 = **$21.99**.
- A fixed amount: the base price plus or minus the amount.
- Then the rounding you chose.
- A **unit cost** is the item's average cost while you have stock on hand, otherwise what you last
  paid for it. Services and products whose stock you don't track have no cost, so a change based on
  cost leaves them alone.
- A price that would fall below zero is left as it is.

Invoices, estimates, sales receipts and other documents you have already written keep the prices
they were written with; only new documents use the new prices. Each change is recorded in the
ledger audit trail, with every item's old and new price and who made it, and the last few are listed
under **Earlier price changes** on the same screen.

## Sales reps, Ship Via and custom fields

These three lists are optional. Until you add something to them, nothing about them shows on your
forms. Manage all three in **Sales › Reps, Ship Via & Fields**.

### Sales reps

A sales rep is a set of initials (up to 5 characters, for example `JS`) and, optionally, a full
name.

1. In **Sales Reps**, type the **Initials** and the name, then click **Add**.
2. To give a customer a default rep, open the customer's **Actions** menu, choose **Edit…** and
   pick the **Sales rep**. New invoices, sales receipts and estimates for that customer fill the
   rep in. You can change it on each form.

Once a rep is on a customer or a form, you can rename it or make it inactive, but not delete it.
An inactive rep stays on the forms that already have it and isn't offered for new ones.
**Reports › Sales by Rep Summary** totals invoices and sales receipts by rep.

### Ship Via

Add the carriers and services you ship with, such as `UPS Ground` or `Local delivery`, under
**Ship Via**. Once the list has an entry, invoices, sales receipts and estimates get a **Ship
date** and a **Ship via** box.

### Custom fields

Custom fields let you keep your own details on a record, such as a customer's region, a vendor's
account number or an item's color. Each list (customers, vendors and items) can have up to 15
fields. A label can have up to 31 characters and a value up to 100.

1. Under **Custom Fields**, choose **Customers**, **Vendors** or **Items**.
2. Type a **Label** and click **Add**.
3. Fill in the value on each record's **Edit…** window, under **Custom fields**.

Tick **On sales forms** for a customer or item field to print it:

- **A customer field** appears as a box on new invoices, sales receipts and estimates. It fills in
  from the customer, and you can change it on that form only. The form keeps the value it was
  issued with, even if you change the customer's value later.
- **An item field** prints under each line that sells the item, as *Label: value*.

Deleting a field also clears it on every record. If a field has been printed on a form, you can't
delete it. Untick **On sales forms** instead.

To turn any of these off on printed forms, see [Design your forms](custom-forms.md). Imports from
QuickBooks bring in all three lists (see [Desktop Migration Wizard](desktop-migration.md)).
