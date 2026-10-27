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

## Price levels

A **price level** is a named set of prices for some of your customers, such as *Wholesale* (10%
off everything) or *Contractor* (special prices on a few items). Give a customer a level, and
their new invoices, sales receipts and estimates fill in its prices.

### Set up a price level

1. Go to **Sales › Products & Services** and click **Price levels…** (or **Lists › Price Level
   List** in the menu bar).
2. Click **New price level**, give it a name and choose its **Type**:
   - **Fixed % (every item):** raise or lower every item's standard price by one percentage. Enter
     `-10` for 10% off or `15` for 15% more.
   - **Per item:** tick the items that get their own price, and for each choose **Fixed price** (a
     price you type), **Standard price ±%** or **Unit cost ±%** (for example `30` for cost plus
     30%). Items you don't tick sell at their standard price.
3. Choose how percentage prices are **rounded**: to the cent, to the nearest step (5¢, 10¢, 25¢,
   50¢, $1, $5 or $10; halves go up), always up or down to a step, or up to a price ending in .99.
4. Check the **Level price** column, then click **Create price level** (or **Save changes**).

How a level's price is worked out, in whole cents (the same arithmetic as Change prices):

- A percentage: the base (standard price or unit cost) plus base × percentage, with the change
  rounded to the cent (half a cent rounds away from zero), then the rounding you chose. For
  example, −10% of $19.99 is $19.99 − $2.00 = **$17.99**. Percentages run from −100% to +1,000%,
  so a price is never below zero.
- A fixed per-item price is used exactly as you typed it.
- **Unit cost** is the item's average cost while you have stock on hand, otherwise what you last
  paid. An item with no cost yet sells at its standard price on a cost-based level.

**Make inactive** hides a level from sales forms and stops it filling in prices; customers keep it
as their default, so making it active again brings their prices back. **Delete** removes it, and
customers who had it go back to standard prices. Every change (created, changed, made inactive,
deleted or imported) is recorded in the ledger audit trail with the level before and after, and the
last few are listed under **Changes** on the same screen.

### Give a customer a price level

Open the customer (**Sales › Customers**, then **Edit** on their row) and choose a **Price level**.
**Standard prices** means no level.

### On invoices, sales receipts and estimates

When a company has price levels, the line grid has a **Price level** column.

- Picking a product fills in the **Rate** from the customer's price level (or the standard price).
- Change a line's **Price level** to price that line at another level, or at **Standard**.
- Changing the customer re-prices lines that are still following the customer's level.
- **A rate you type always wins:** it is never replaced by a level's price.

The level only fills in the rate. Documents keep the rate they were written with, so changing a
level later never changes invoices or estimates you've already written.

### Item Price List report

**Reports › Item Price List** (or **Reports › Sales › Item Price List** in the menu bar) lists every
item with its unit cost, its standard price and its price at each level. Tick **Show inactive
items and levels** to include those. It exports to Excel, CSV and PDF like other reports.

### Price levels from QuickBooks

The Desktop Migration Wizard brings price levels in with customers, vendors and items, and gives
the customers it adds their default level. See [the migration guide](desktop-migration.md).

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
