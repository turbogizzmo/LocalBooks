# Customers, products and services

## Add a customer or vendor

1. Go to **Sales › Customers** (or **Expenses › Vendors**).
2. In the first row of the list, type the name in **New customer name** (or **New vendor name**)
   and, optionally, an email address.
3. Click **Add**.

The list shows each customer's open balance in Accounts Receivable, or what you still owe each
vendor. Use **Find a customer** to filter the list. Customers are also added automatically when you
post an invoice for a new name.

You can also add a customer or vendor while you enter a transaction. In the **Customer** box on an
invoice, sales receipt, estimate or credit memo, or the **Vendor** box on a bill, check, expense,
purchase order or vendor credit, type the name. If nobody matches, the list offers
**＋ Add "…" as a new customer** (or vendor), added straight away, and **Add with details…**, which
adds the name and opens its record for the address, terms and (for vendors) 1099 status.

### Copy a customer or vendor

To add a name that shares most of another's details (a second location, a sister company), open
the row menu (▾) on its line in the Customers or Vendors list and choose **Duplicate…**. Type a name for the
copy, which starts as "… (copy)", and click **OK**. LocalBooks adds the copy and opens it so you can
change what differs. The copy has the original's contact details, addresses, terms, types, sales
rep, price level, tax settings, credit limit and custom fields, and no transactions. It leaves out
what belongs to one name only: the account and resale numbers, and a vendor's tax ID. A copy of a
job stays a job of the same customer, so its name starts with the customer's
(`Acme:Bathroom`).

To copy an account, choose **Duplicate…** on its row in the Chart of Accounts. The **New account**
window opens with its type, parent and description filled in; give it a name and number and click
**Add account**.

### The information panel

Click a name in the Customers or Vendors list, or an item in Products & Services, and its
information panel opens beside the list:

- **Customers:** the open balance, active estimates (pending or accepted, not yet invoiced),
  unbilled costs and unbilled mileage, then phone, email, notes and the five most recent
  transactions (click one to open it).
- **Vendors:** the balance you owe, open purchase orders, phone, email, notes and recent
  transactions.
- **Items:** the sales price and preferred vendor and, for an item whose stock is tracked, on hand,
  on order (with the next expected delivery), average cost, value and reorder point (marked
  **time to reorder** when on hand is at or below it). Set the reorder point and preferred vendor on
  the item's record (see [Reorder point and preferred vendor](inventory.md#reorder-point-and-preferred-vendor)).

The figures are the reports' own: the open balance matches A/R or A/P Aging and the balance
reports, open purchase orders the Open Purchase Orders report, unbilled costs Sales › Bill Costs,
and the item figures Inventory Valuation and Stock Status.

The panel has tabs. **Notes** shows the customer's or vendor's notes (the same Notes as on its
record) to read and change: **Date Stamp** starts a new first line with today's date, and **Save**
keeps them. For a customer, **Emails** lists every form emailed to them, newest first: invoices,
estimates, sales receipts and statements, with the date, who it went to and the subject.

Invoices, sales receipts, estimates, credit memos, bills, checks, expenses, vendor credits and
purchase orders show the same panel beside the form once you choose a customer or vendor on it. Click **Hide info** (or **×** on the panel) to hide it, and **Show info**
(or the slim **Info** strip beside a form) to bring it back; LocalBooks remembers the choice for
each list and for forms.

### See a customer's or vendor's transactions

Click a name in the Customers or Vendors list. Its transactions (invoices, payments, bills, checks
and so on) show in a panel under the list, newest first, with a total. Click a heading to sort,
tick **Show voided** to include voided ones, and double-click a transaction to open it.
Double-click the name itself to edit the customer or vendor.

To narrow the list, choose a transaction type (only invoices, say, or only bill payments) and a
date range: **This month**, **This quarter**, **This year**, **Last year**, or your own **From** and
**To** dates. The count and total cover what is shown. Click a transaction and press **Ctrl+H** for
its [Transaction History](reports.md#quickreport-transaction-history-and-transaction-journal) or
**Ctrl+Y** for its Transaction Journal; **QuickReport** (or **Ctrl+Q** on the name) opens the same
transactions as a report.

To add a company name, address, phone, payment terms, a customer or vendor type or notes, open
the customer's **Actions** menu and choose **Edit…**. A customer's record also holds:

- **Billing address (Bill To)** and **Shipping address (Ship To)**. **Copy billing** puts the
  billing address in the shipping box. New invoices, sales receipts, estimates and credit memos
  fill their Bill To and Ship To from these. A job (a name like `Acme:Kitchen`) whose boxes are
  blank uses its customer's addresses.
- **Contacts**: as many people as you need, each with a **Role** (pick one such as *Billing* or
  *Accounts payable*, or type your own), **Email**, **Phone** and **Mobile**. Click **Add
  contact** for another, ↑ and ↓ to reorder, × to remove. Tick **Invoices** on the contact for
  invoices: their name prints in Bill To and Ship To on new sales forms, under the company name,
  and emailed invoices, estimates, sales receipts and credit memos go to their email (the
  customer's own email when they have none). Tick **Statements** on the contact for statements:
  their name prints on the customer's statements and emailed statements go to them. One contact
  can do both. A vendor's record has contacts too, with a **Main** contact.
- **Alt. phone**, **Fax** and **Website**.
- **Account no.**, the customer's account number.
- **Credit limit**. An invoice that would take the customer over it asks before saving (see
  [Credit limits](invoices.md#credit-limits)). Leave it blank for no limit.
- **Resale no. / tax ID**, for a resale certificate or the customer's tax ID.

The email address is filled in when you choose **Email…** on one of their forms. Changing a
customer's address doesn't change documents already saved; each keeps the address it was saved
with.

### Preferred delivery and job status

A customer's **Preferred delivery** (on **Edit…**) decides how their new invoices and estimates
start: **Email** ticks **Email later** (when the customer has an email address), **Print** ticks **Print later** (invoices), **None** leaves
both unticked. A new customer takes the choice in **Preferences › Send Forms › Company
Preferences**, however it is added.

A job (a name such as `Acme:Kitchen`) can also have a **Job status**: Pending, Awarded, In
progress, Closed or Not awarded. The status shows next to the job in the Customers list. An Admin
can rename the five statuses in **Preferences › Jobs & Estimates**, for example *Awarded* to
*Won*.

## Customer groups

In **Sales › Customers**, switch **View** to **Groups** to see and make customer groups.

- **Pick the customers**: tick the customers that belong; the group keeps exactly that list.
- **Automatic**: add rules, and the group holds every customer who meets all of them, worked out
  from their details each time you look, so it changes by itself when a balance, address or other
  detail does. A rule can look at the **open balance** (at least or at most an amount), **customer
  type**, **terms**, **sales rep**, **state** or **ZIP code** (from the last line of the billing
  address, such as `Salem, OR 97301`), **tax code** (or tax exempt), **credit limit** (over it, or
  has one) and **status** (active or inactive).

Click a group to list its customers right now; double-click it to change it.

### Batch invoices

To bill many customers the same thing (a monthly fee, a seasonal service), click **Batch
invoices…** in **Sales › Customers**, or choose **Batch invoice this group…** on a group.

1. Pick a **Customer group**, tick customers by hand, or both. Inactive customers are left out.
2. Set the **Invoice date**, and **Due after (days)** for customers without terms of their own
   (customers with terms are due on their terms).
3. Enter the lines once, as on an invoice. The dialog shows each invoice's total and the total of
   all of them.
4. Click **Create invoices**.

Each customer gets an invoice of their own, posted to Accounts Receivable. They are created
together: if one can't be (an inactive item, say), none are. Each waits where the customer's
**Preferred delivery** says: **Sales › Send Forms** for Email, **File › Print Forms** for Print.

### Payment reminders

On a group, tick **Payment reminders: every … days, remind about invoices at least … days
overdue**. When a round is due and the group has overdue invoices, **Reminders** lists **Payment
reminders for** the group with the amount overdue. Click it (or choose **Payment reminders…** on
the group) to review the overdue invoices: untick any to leave out, then click **Send reminders**.
Each ticked invoice is queued in **Sales › Send Forms**, which emails it with its invoice message,
and the next round is due after the days you set.

### Statement reminders

Tick **Remind me to send this group's statements every … days** on a group. When they are due
(and on the first day, if they have never been sent), **Reminders** lists **Statements for** the
group. Click it to open **Sales › Customer Statements** with the group chosen: its customers with a
statement in the period are ticked. Print or email them, then click **Mark sent** to start the wait
for the next ones. In Customer Statements you can also pick any group under **Customer group**.

## Add a product or service

Products and services make invoices and sales receipts quicker to fill in.

1. Go to **Sales › Products & Services**.
2. In the first row, enter the name in **New product or service**, a **Description on sales
   forms**, the **Rate**, and the **Income account** it belongs to.
3. Click **Add**.

Choosing the product on an invoice or sales receipt then fills in its description, rate and income
account. You can still change them on the form.

To keep count of a product you stock, see [Track inventory](inventory.md).

## Discounts, other charges and groups

Besides services and parts, an item can be a **Discount** or an **Other charge**, and items can be
put together in a **Group**. Choose the **Type** in the first row of Products & Services (or in an
item's **Edit…** window).

- **Discount**: enter an amount (`5.00`) or a percentage (`10%`) as its rate. On a sales form it
  comes off the line above it: an item, a group or a subtotal. Its account is the one the discount
  is debited to; *Discounts Given* is the usual choice. Tick **Tax** if the discount lowers the
  taxable amount.
- **Other charge**: shipping, handling, a rush fee. An amount works like any item; a percentage
  (`3%`) is worked out on the line above it.
- **Group**: click **Groups…**, then **New group…**. Give it a name, a description for sales forms
  and its items with their quantities. Tick **Print as one line** to print the group as a single
  line with its total instead of each item. A group holds services, parts and fixed charges, not
  discounts or percent charges, and can't share a name with an item.

Discounts can't be tracked in stock or given price-level prices. How each one works on a form is in
[Create and send an invoice](invoices.md#subtotals-discounts-charges-and-groups).

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
     Choosing **Unit cost** fills in the **Default markup** from **Preferences › Sales &
     Customers** when you haven't typed an amount yet.
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

A business that doesn't use them can untick **Use price levels** in **Preferences › Sales &
Customers**. Levels then disappear from customers and forms, and every form uses standard prices;
the levels you made are kept for when you turn them back on.

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

## Customer & Vendor Profile lists

**Lists › Customer & Vendor Profile Lists** opens six short lists you can fill in as you need them
(they are also under **Sales › Terms, Methods & Types**): Terms, Payment Method, Customer Type,
Vendor Type, Job Type and Customer Message. Each one works the same way:

1. Type the new entry in the first row and click **Add**.
2. Double-click an entry (or choose **Edit** in its **Actions** menu) to rename it.
3. Choose **Make inactive** to stop offering an entry. It stays on the records and forms that
   already have it. Tick **Show inactive** to see it again and make it active.

An entry can be deleted only while nothing uses it. You don't have to open the lists first: the
Terms, Payment method, type and Customer message boxes all end in **＋ Add new…**, which adds an
entry and picks it.

### Terms

A term says when an invoice or bill is due, and can offer a discount for paying early. A new
company starts with **Due on receipt**, **Net 15**, **Net 30** and **Net 60**.

- **Net days** is the number of days from the invoice or bill date to its due date.
- **Discount %** and **Discount days** are optional. For example, *2% 10 Net 30* is 2 percent off
  if paid within 10 days, otherwise the full amount is due in 30 days. The discount days can't be
  more than the net days.

Give each customer and vendor a default term in **Edit…** › **Terms**. On a new invoice or bill
the **Terms** box fills in from the customer or vendor (Net 30 when they have none) and sets the
due date. Choose another term and the due date follows it; you can still type a different due
date. The invoice or bill keeps the term it was entered with, and printed invoices show it.

When a term has a discount, **Receive Payments** and **Pay Bills** show the discount offered on
each invoice or bill, and the last day to take it. Printed invoices show it too. LocalBooks
doesn't yet take the discount for you: to give or take it, record the payment for the reduced
amount and enter the difference as a credit memo or vendor credit.

Changing a term's days changes the due dates of new invoices and bills only. Ones already entered
keep their due dates.

### Payment Method

Payment methods are the names in the **Payment method** boxes on Receive Payments, sales
receipts and expenses, such as *Visa*, *MasterCard* or *Check*. Each method is one of six kinds
(check, cash, credit card, ACH / bank transfer, wire or other), which you choose when you add it.
A new company has one method of each kind. You can rename a method that's in use, but not change
its kind.

### Customer, Vendor and Job Types

Types group your records, for example *Retail* and *Wholesale* customers, *Supplier* and
*Contractor* vendors, or *Remodel* and *New build* jobs. Pick a customer's or vendor's type in
**Edit…**. A job (a customer under another customer, such as `Acme:Kitchen`) also has a **Job
type**.

### Customer Message

A customer message is a line printed at the foot of an invoice, sales receipt or estimate, such as
*Thank you for your business.* Pick it in the **Customer message** box on the form. A message can
be up to 101 characters.

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

To fill in the same carrier on every new form, choose it as **Usual shipping method** in
**Preferences › Sales & Customers**. You can still change it on each form.

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
