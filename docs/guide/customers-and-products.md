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
