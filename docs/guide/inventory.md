# Track inventory

LocalBooks can keep the quantity on hand and cost of the products you stock. Costs use **moving
average**: each purchase updates the average cost, and each sale moves the cost of what was sold
from **Inventory Asset** to **Cost of Goods Sold**.

## Start tracking a product

1. Go to **Sales › Products & Services**. Add the product first if it isn't listed.
2. On its row, click **Track stock…**, optionally enter a **SKU**, and click **Track stock**.

The product starts at zero on hand, using the **Inventory Asset** and **Cost of Goods Sold**
accounts.

## Record opening stock

1. On the product's row, click **Adjust stock…**.
2. Choose **Add stock**, then enter the **Quantity** and the **Total cost** of those units.
3. Keep **Opening Balance Equity** as the balancing account, set the **Date**, and click **Save &
   Post Adjustment**.

## Buy stock on a bill

1. Go to **Expenses › Bills** and click **New bill**.
2. On a line, choose the product under **Stock item**. The line posts to its inventory account
   automatically.
3. Enter the **Qty** and **Rate**; a stock line needs both, so the cost per unit is known.
4. Post the bill. The quantity and value on hand go up by the line's quantity and amount.

## Sell stock

Sell it on an invoice or sales receipt as usual, choosing the product on the line (the list shows
how many are on hand). Posting also records the cost of the units sold in **Cost of Goods Sold**.

- LocalBooks won't sell more than you have on hand. Receive stock or adjust the quantity first.
- Products with tracked stock can only be sold on home-currency invoices for now.

## Count and correct stock

1. Click **Adjust stock…** and choose **Remove stock** for units lost, damaged or used. They are
   valued at the current average cost, shown before you save.
2. Choose the expense account for the loss (Cost of Goods Sold by default) and click **Save & Post
   Adjustment**.

The **Stock history** in the same window lists every purchase, sale, adjustment and void.

> **Tip:** Voiding a sale returns its units at the cost they left at. A bill can't be voided once
> its units have been sold. Void the sales first, or adjust the quantity instead.
